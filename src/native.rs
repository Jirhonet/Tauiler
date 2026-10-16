//! Win32 boundary. Callbacks run on the message-loop thread and only enqueue observations.
//! Raw handles are revalidated against process/thread identity before mutations. All pointers
//! passed to synchronous Win32 APIs refer to live stack buffers; hook lifetimes are RAII-owned.
use crate::report;
use crate::{
    config::{self, Action, Config, Direction, RuleMode},
    input::Input,
    layout::{Layout, Point, Rect, WindowId},
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::null_mut,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{Console::*, DataExchange::COPYDATASTRUCT, LibraryLoader::*, Threading::*},
    UI::{
        Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*,
    },
};

const WAKE: u32 = WM_APP + 1;
const TRAY: u32 = WM_APP + 2;
// NIN_KEYSELECT is a C macro (NIN_SELECT | NINF_KEY), omitted by windows-sys.
const TRAY_KEYSELECT: u32 = NIN_SELECT | 1;
const STYLE_MASK: u32 = WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
static LOOP_THREAD: AtomicU32 = AtomicU32::new(0);
static CLEANED: AtomicBool = AtomicBool::new(false);
unsafe extern "system" fn console_control(event: u32) -> i32 {
    if !matches!(
        event,
        CTRL_C_EVENT
            | CTRL_BREAK_EVENT
            | CTRL_CLOSE_EVENT
            | CTRL_LOGOFF_EVENT
            | CTRL_SHUTDOWN_EVENT
    ) {
        return 0;
    }
    unsafe {
        PostThreadMessageW(LOOP_THREAD.load(Ordering::Relaxed), WM_QUIT, 0, 0);
    }
    if !matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        for _ in 0..200 {
            if CLEANED.load(Ordering::Acquire) {
                break;
            }
            unsafe {
                Sleep(10);
            }
        }
    }
    1
}
thread_local! {
    static INPUT_STATE:RefCell<Option<Input>>=const {RefCell::new(None)};
    static EVENTS:RefCell<VecDeque<Event>>=const {RefCell::new(VecDeque::new())};
    static THREAD_ID:RefCell<u32>=const {RefCell::new(0)};
    static DRAG_BUTTON:RefCell<Option<bool>>=const {RefCell::new(None)};
    static MANAGED:RefCell<HashSet<WindowId>>=RefCell::new(HashSet::new());
    static DIRTY_QUEUED:Cell<bool>=const {Cell::new(false)};
    static POINTER_WINDOW:Cell<Option<WindowId>>=const {Cell::new(None)};
    static TRAY_MODERN:Cell<bool>=const {Cell::new(false)};
    static NATIVE_TEST:Cell<bool>=const {Cell::new(false)};
    static CREATION_PLAN:RefCell<Option<CreationPlan>>=const {RefCell::new(None)};
}
enum Event {
    TrayMenu,
    Action(Action),
    Dirty,
    Destroy(WindowId),
    DragStart(Point, bool),
    DragMove(Point),
    DragEnd(Point),
    NativeDragStart(WindowId),
    NativeDragEnd(WindowId),
}
unsafe extern "system" fn controller_window_proc(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    if message == WM_COPYDATA {
        return unsafe { creation_request(w, l) };
    }
    if message == TRAY {
        // Shell callbacks may be SENT while GetMessage is running; they must be
        // handled here, not only by looking at messages returned from GetMessage.
        let code = l as u32 & 0xffff;
        let modern = TRAY_MODERN.with(Cell::get);
        if (modern && matches!(code, NIN_SELECT | TRAY_KEYSELECT))
            || (!modern && code == WM_LBUTTONUP)
        {
            queue(Event::Action(Action::Pause));
        } else if code == WM_CONTEXTMENU || (!modern && code == WM_RBUTTONUP) {
            queue(Event::TrayMenu);
        }
        return 0;
    }
    if message == WM_CLOSE {
        queue(Event::Action(Action::Exit));
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}
fn queue(event: Event) {
    if matches!(event, Event::Dirty) && DIRTY_QUEUED.with(|d| d.replace(true)) {
        return;
    }
    let wake = EVENTS.with(|q| {
        let mut q = q.borrow_mut();
        if let Event::DragMove(p) = event
            && let Some(Event::DragMove(last)) = q.back_mut()
        {
            *last = p;
            return false;
        }
        let empty = q.is_empty();
        q.push_back(event);
        empty
    });
    if wake {
        THREAD_ID.with(|id| unsafe {
            PostThreadMessageW(*id.borrow(), WAKE, 0, 0);
        });
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn from_rect(r: RECT) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        width: r.right - r.left,
        height: r.bottom - r.top,
    }
}
fn cursor() -> Point {
    unsafe {
        let mut p = POINT::default();
        GetCursorPos(&mut p);
        Point { x: p.x, y: p.y }
    }
}
fn identity(hwnd: HWND) -> (u32, u32) {
    unsafe {
        let mut pid = 0;
        let tid = GetWindowThreadProcessId(hwnd, &mut pid);
        (pid, tid)
    }
}
fn window_rect(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut r = RECT::default();
        (GetWindowRect(hwnd, &mut r) != 0).then(|| from_rect(r))
    }
}
fn visible_rect(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut r = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut r as *mut _ as _,
            size_of::<RECT>() as u32,
        ) >= 0
        {
            Some(from_rect(r))
        } else {
            window_rect(hwnd)
        }
    }
}
fn class(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), 256);
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }
}
fn title(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 512);
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }
}
fn executable(hwnd: HWND) -> String {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, identity(hwnd).0);
        if process.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 32768];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or("").to_ascii_lowercase()
    }
}
fn rule_mode(hwnd: HWND, c: &Config) -> Option<RuleMode> {
    if c.rules.is_empty() {
        return None;
    }
    let exe = executable(hwnd);
    let name = class(hwnd);
    let text = title(hwnd);
    c.rules
        .iter()
        .find(|r| {
            r.executable
                .as_ref()
                .is_none_or(|v| v.eq_ignore_ascii_case(&exe))
                && r.class.as_ref().is_none_or(|v| v == &name)
                && r.title_contains.as_ref().is_none_or(|v| text.contains(v))
        })
        .map(|r| r.mode)
}
fn cloaked(hwnd: HWND) -> bool {
    unsafe {
        let mut value = 0u32;
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut value as *mut _ as _, 4) >= 0
            && value != 0
    }
}

#[derive(Clone, Debug)]
struct Monitor {
    id: String,
    handle: usize,
    work: Rect,
    full: Rect,
}
fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn callback(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        // EnumDisplayMonitors invokes this synchronously with the live Vec pointer.
        unsafe {
            let mut info: MONITORINFOEXW = zeroed();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(h, &mut info.monitorInfo) != 0 {
                let n = info
                    .szDevice
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(info.szDevice.len());
                (*(data as *mut Vec<Monitor>)).push(Monitor {
                    id: String::from_utf16_lossy(&info.szDevice[..n]),
                    handle: h as usize,
                    work: from_rect(info.monitorInfo.rcWork),
                    full: from_rect(info.monitorInfo.rcMonitor),
                });
            }
            1
        }
    }
    let mut out = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            std::ptr::null(),
            Some(callback),
            &mut out as *mut _ as isize,
        );
    }
    out
}
fn all_windows() -> Vec<WindowId> {
    unsafe extern "system" fn callback(h: HWND, data: LPARAM) -> i32 {
        unsafe {
            (*(data as *mut Vec<WindowId>)).push(h as usize);
        }
        1
    }
    let mut out = Vec::new();
    unsafe {
        EnumWindows(Some(callback), &mut out as *mut _ as isize);
    }
    out
}
fn eligible(hwnd: HWND, c: &Config) -> bool {
    unsafe {
        if IsWindow(hwnd) == 0
            || IsWindowVisible(hwnd) == 0
            || identity(hwnd).0 == GetCurrentProcessId()
            || !GetWindow(hwnd, GW_OWNER).is_null()
        {
            return false;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if style & WS_CHILD != 0
            || ex & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) != 0
            || GetWindowTextLengthW(hwnd) == 0
        {
            return false;
        }
        let name = class(hwnd);
        if [
            "startmenuexperiencehost.exe",
            "searchhost.exe",
            "searchapp.exe",
            "shellexperiencehost.exe",
            "textinputhost.exe",
            "widgets.exe",
            "lockapp.exe",
        ]
        .contains(&executable(hwnd).as_str())
        {
            return false;
        }
        if rule_mode(hwnd, c) == Some(RuleMode::Ignore) {
            return false;
        }
        if [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "#32770",
            "Windows.UI.Core.CoreWindow",
            "XamlExplorerHostIslandWindow",
            "MultitaskingViewFrame",
        ]
        .contains(&name.as_str())
            || c.ignore_classes.contains(&name)
        {
            return false;
        }
        true
    }
}

unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 {
            let data = &*(l as *const KBDLLHOOKSTRUCT);
            if data.flags & LLKHF_INJECTED == 0 {
                let pressed = w as u32 == WM_KEYDOWN || w as u32 == WM_SYSKEYDOWN;
                let d = INPUT_STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .map(|s| s.event(data.vkCode, pressed))
                });
                if let Some(d) = d {
                    // Mask Start after a handled chord in configured-only mode, without a text key.
                    if d.mask_start {
                        // Unassigned menu-mask key avoids changing a held Ctrl/Alt modifier.
                        keybd_event(0xe8, 0, 0, 0);
                        keybd_event(0xe8, 0, KEYEVENTF_KEYUP, 0);
                    }
                    if let Some(a) = d.action {
                        queue(Event::Action(a));
                    }
                    if d.consume {
                        return 1;
                    }
                }
            }
        }
        CallNextHookEx(null_mut(), code, w, l)
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 {
            let data = &*(l as *const MSLLHOOKSTRUCT);
            if data.flags & LLMHF_INJECTED == 0 {
                let p = Point {
                    x: data.pt.x,
                    y: data.pt.y,
                };
                let msg = w as u32;
                let active = DRAG_BUTTON.with(|b| *b.borrow());
                if let Some(right) = active {
                    if msg == WM_MOUSEMOVE {
                        queue(Event::DragMove(p));
                        // Only the initiating button is consumed. Suppressing movement also
                        // suppresses normal cursor updates and makes the drag fight the mouse.
                        return CallNextHookEx(null_mut(), code, w, l);
                    }
                    if msg == if right { WM_RBUTTONUP } else { WM_LBUTTONUP } {
                        DRAG_BUTTON.with(|b| *b.borrow_mut() = None);
                        queue(Event::DragEnd(p));
                        return 1;
                    }
                }
                let enabled =
                    INPUT_STATE.with(|s| s.borrow().as_ref().is_some_and(|i| i.super_down()));
                if enabled && matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN) {
                    let h = GetAncestor(WindowFromPoint(POINT { x: p.x, y: p.y }), GA_ROOT);
                    if !MANAGED.with(|set| set.borrow().contains(&(h as usize)))
                        || IsWindowEnabled(h) == 0
                    {
                        return CallNextHookEx(null_mut(), code, w, l);
                    }
                    INPUT_STATE.with(|s| {
                        if let Some(i) = s.borrow_mut().as_mut() {
                            i.mark_super_handled();
                        }
                    });
                    let right = msg == WM_RBUTTONDOWN;
                    DRAG_BUTTON.with(|b| *b.borrow_mut() = Some(right));
                    queue(Event::DragStart(p, right));
                    return 1;
                }
                if msg == WM_LBUTTONDOWN {
                    let h =
                        GetAncestor(WindowFromPoint(POINT { x: p.x, y: p.y }), GA_ROOT) as usize;
                    if MANAGED.with(|set| set.borrow().contains(&h)) {
                        POINTER_WINDOW.with(|window| window.set(Some(h)));
                        queue(Event::NativeDragStart(h));
                    }
                } else if msg == WM_LBUTTONUP
                    && let Some(id) = POINTER_WINDOW.with(|window| window.take())
                {
                    queue(Event::NativeDragEnd(id));
                }
            }
        }
        CallNextHookEx(null_mut(), code, w, l)
    }
}
unsafe extern "system" fn win_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if !hwnd.is_null() && matches!(event, EVENT_SYSTEM_MOVESIZESTART | EVENT_SYSTEM_MOVESIZEEND) {
        queue(if event == EVENT_SYSTEM_MOVESIZESTART {
            Event::NativeDragStart(hwnd as usize)
        } else {
            Event::NativeDragEnd(hwnd as usize)
        });
        return;
    }
    if object == OBJID_WINDOW && child == 0 && !hwnd.is_null() {
        if event == EVENT_OBJECT_DESTROY {
            if MANAGED.with(|set| set.borrow().contains(&(hwnd as usize))) {
                queue(Event::Destroy(hwnd as usize));
            }
        } else {
            queue(Event::Dirty);
        }
    }
}

struct Resources {
    window: HWND,
    keyboard: HHOOK,
    mouse: HHOOK,
    event: HWINEVENTHOOK,
    move_size: HWINEVENTHOOK,
    mutex: HANDLE,
    tray: NOTIFYICONDATAW,
    icons: HashMap<(u32, bool), HICON>,
    tray_state: Option<(u32, bool)>,
}
impl Drop for Resources {
    fn drop(&mut self) {
        unsafe {
            if !self.keyboard.is_null() {
                UnhookWindowsHookEx(self.keyboard);
            }
            if !self.mouse.is_null() {
                UnhookWindowsHookEx(self.mouse);
            }
            if !self.event.is_null() {
                UnhookWinEvent(self.event);
            }
            if !self.move_size.is_null() {
                UnhookWinEvent(self.move_size);
            }
            if !self.window.is_null() {
                KillTimer(self.window, 1);
                Shell_NotifyIconW(NIM_DELETE, &self.tray);
                DestroyWindow(self.window);
            }
            if !self.mutex.is_null() {
                CloseHandle(self.mutex);
            }
            for icon in self.icons.values() {
                DestroyIcon(*icon);
            }
            SetConsoleCtrlHandler(Some(console_control), 0);
            CLEANED.store(true, Ordering::Release);
        }
    }
}
impl Resources {
    fn new() -> Result<Self, Box<dyn Error>> {
        unsafe {
            let mut r = Self {
                window: null_mut(),
                keyboard: null_mut(),
                mouse: null_mut(),
                event: null_mut(),
                move_size: null_mut(),
                mutex: null_mut(),
                tray: zeroed(),
                icons: HashMap::new(),
                tray_state: None,
            };
            r.mutex = CreateMutexW(std::ptr::null(), 0, wide("Local\\Tauiler.Manager").as_ptr());
            if r.mutex.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                return Err("Tauiler is already running".into());
            }
            LOOP_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
            CLEANED.store(false, Ordering::Release);
            SetConsoleCtrlHandler(Some(console_control), 1);
            let module = GetModuleHandleW(std::ptr::null());
            let name = wide("Tauiler.Controller");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(controller_window_proc),
                hInstance: module,
                lpszClassName: name.as_ptr(),
                ..zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            r.window = CreateWindowExW(
                0,
                name.as_ptr(),
                wide("Tauiler").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                module,
                std::ptr::null(),
            );
            if r.window.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            r.keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0);
            r.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
            r.event = SetWinEventHook(
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_LOCATIONCHANGE,
                null_mut(),
                Some(win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            r.move_size = SetWinEventHook(
                EVENT_SYSTEM_MOVESIZESTART,
                EVENT_SYSTEM_MOVESIZEEND,
                null_mut(),
                Some(win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if r.keyboard.is_null()
                || r.mouse.is_null()
                || r.event.is_null()
                || r.move_size.is_null()
            {
                return Err(std::io::Error::last_os_error().into());
            }
            if SetTimer(r.window, 1, 50, None) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            r.tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            r.tray.hWnd = r.window;
            r.tray.uID = 1;
            r.tray.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
            r.tray.uCallbackMessage = TRAY;
            let icon = number_icon(1, false)?;
            r.icons.insert((1, false), icon);
            r.tray.hIcon = icon;
            let tip = wide("Tauiler • Workspace 1 • Click or SUPER+M to pause tiling");
            r.tray.szTip[..tip.len()].copy_from_slice(&tip);
            if Shell_NotifyIconW(NIM_ADD, &r.tray) == 0 {
                return Err("cannot create system tray icon".into());
            }
            r.tray.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let modern = Shell_NotifyIconW(NIM_SETVERSION, &r.tray) != 0;
            TRAY_MODERN.with(|state| state.set(modern));
            if !modern {
                report!("Tray callback version negotiation failed; accepting legacy callbacks");
            }
            r.tray_state = Some((1, false));
            Ok(r)
        }
    }
    fn update_icon(&mut self, slot: u32, paused: bool) -> Result<(), Box<dyn Error>> {
        let key = (slot, paused);
        if self.tray_state == Some(key) {
            return Ok(());
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = self.icons.entry(key) {
            entry.insert(number_icon(slot, paused)?);
        }
        self.tray.hIcon = self.icons[&key];
        self.tray.szTip.fill(0);
        let tip = wide(&format!(
            "Tauiler • Workspace {slot} • {} • Click or SUPER+M to {} tiling",
            if paused { "Paused" } else { "Active" },
            if paused { "resume" } else { "pause" }
        ));
        let len = tip.len().min(self.tray.szTip.len() - 1);
        self.tray.szTip[..len].copy_from_slice(&tip[..len]);
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.tray) } == 0 {
            return Err("cannot update workspace tray icon".into());
        }
        self.tray_state = Some(key);
        Ok(())
    }
    fn menu(&self, path: &std::path::Path, paused: bool) -> Option<Action> {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            for (id, label) in [
                (
                    1,
                    if paused {
                        "Resume tiling"
                    } else {
                        "Pause tiling"
                    },
                ),
                (2, "Reload configuration"),
                (3, "Open configuration"),
                (4, "Quit Tauiler completely"),
            ] {
                AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
            }
            let p = cursor();
            SetForegroundWindow(self.window);
            let id = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                p.x,
                p.y,
                0,
                self.window,
                std::ptr::null(),
            );
            DestroyMenu(menu);
            PostMessageW(self.window, WM_NULL, 0, 0);
            match id {
                1 => Some(Action::Pause),
                2 => Some(Action::Reload),
                3 => Some(Action::Launch {
                    executable: "notepad.exe".into(),
                    args: vec![path.display().to_string()],
                }),
                4 => Some(Action::Exit),
                _ => None,
            }
        }
    }
}
fn number_icon(slot: u32, paused: bool) -> Result<HICON, Box<dyn Error>> {
    unsafe {
        let size = crate::tray_icon::SIZE as i32;
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size;
        info.bmiHeader.biHeight = -size;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let pixels = match crate::tray_icon::pixels(slot, paused) {
            Ok(pixels) => pixels,
            Err(error) => {
                DeleteObject(color);
                return Err(error);
            }
        };
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
        let mut mask_bits = [0u8; 128];
        for y in 0..size as usize {
            for x in 0..size as usize {
                if pixels[(y * size as usize + x) * 4 + 3] == 0 {
                    mask_bits[y * 4 + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as _);
        if mask.is_null() {
            DeleteObject(color);
            return Err(std::io::Error::last_os_error().into());
        }
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: 1,
            hbmMask: mask,
            hbmColor: color,
            ..zeroed()
        });
        DeleteObject(color);
        DeleteObject(mask);
        if icon.is_null() {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(icon)
        }
    }
}

#[derive(Debug)]
struct Record {
    identity: (u32, u32),
    desktop: u128,
    monitor: String,
    floating: bool,
    minimized: bool,
    visibility_pending: Option<(bool, Instant)>,
    topmost_owned: bool,
    topmost_failed: bool,
    topmost_attempt: Option<Instant>,
    fullscreen: bool,
    paused_fullscreen_rect: Option<Rect>,
    removed_style: u32,
    removed_fullscreen_style: u32,
    style_failed: bool,
    refusals: u8,
    last_attempt: Option<Instant>,
    desired: Option<Rect>,
}
struct Drag {
    id: WindowId,
    right: bool,
    origin: Point,
    start_rect: Rect,
    start_layout: Option<Layout>,
}
impl Drag {
    fn target(&self, p: Point) -> Rect {
        let dx = p.x - self.origin.x;
        let dy = p.y - self.origin.y;
        if self.right {
            Rect {
                width: (self.start_rect.width + dx).max(64),
                height: (self.start_rect.height + dy).max(64),
                ..self.start_rect
            }
        } else {
            Rect {
                x: self.start_rect.x + dx,
                y: self.start_rect.y + dy,
                ..self.start_rect
            }
        }
    }
}
struct NativeDrag {
    id: WindowId,
    start_rect: Rect,
}
#[derive(Clone, Copy)]
struct FrameInsets {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
fn frame_insets(hwnd: HWND) -> Option<FrameInsets> {
    let outer = window_rect(hwnd)?;
    let visible = visible_rect(hwnd)?;
    Some(FrameInsets {
        x: outer.x - visible.x,
        y: outer.y - visible.y,
        width: outer.width - visible.width,
        height: outer.height - visible.height,
    })
}
fn place_cached(hwnd: HWND, target: Rect, frame: FrameInsets) {
    unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            target.x + frame.x,
            target.y + frame.y,
            (target.width + frame.width).max(1),
            (target.height + frame.height).max(1),
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS | SWP_NOSENDCHANGING,
        );
    }
}
#[derive(Clone)]
struct AdoptionContext {
    identity: (u32, u32),
    monitor: String,
    point: Point,
}
fn adoption_context(hwnd: HWND, monitors: &[Monitor]) -> Option<AdoptionContext> {
    let handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) } as usize;
    let monitor = monitors.iter().find(|m| m.handle == handle)?;
    Some(AdoptionContext {
        identity: identity(hwnd),
        monitor: monitor.id.clone(),
        point: visible_rect(hwnd)
            .map(|r| r.center())
            .unwrap_or(monitor.work.center()),
    })
}
fn adoption_monitor<'a>(
    monitors: &'a [Monitor],
    source: usize,
    pointer: Point,
    existing: Option<&AdoptionContext>,
) -> Option<&'a Monitor> {
    if let Some(existing) = existing {
        // Existing windows keep their monitor even if their desktop becomes visible later.
        monitors
            .iter()
            .find(|m| m.id == existing.monitor)
            .or_else(|| monitors.iter().find(|m| m.handle == source))
    } else {
        monitors
            .iter()
            .find(|m| m.full.contains(pointer))
            .or_else(|| monitors.iter().find(|m| m.handle == source))
    }
}
struct CreationPlan {
    config: Config,
    layouts: HashMap<(u128, String), Layout>,
    monitors: Vec<Monitor>,
    current: u128,
    enabled: bool,
    blocked_monitors: HashSet<String>,
    test_process: Option<u32>,
    test_executable: Option<String>,
    test_point: Option<Point>,
    known_windows: HashSet<WindowId>,
}
impl CreationPlan {
    fn target(
        &self,
        request: &crate::creation::protocol::Request,
        executable: &str,
    ) -> Option<Rect> {
        use crate::creation::protocol::{candidate, text};
        if !self.enabled
            || !candidate(request.style, request.ex_style, false)
            || (request.phase == 1 && self.known_windows.contains(&(request.window as usize)))
        {
            return None;
        }
        let name = text(&request.class);
        let title = text(&request.title);
        if self.config.ignore_classes.contains(&name)
            || [
                "Progman",
                "WorkerW",
                "Shell_TrayWnd",
                "Shell_SecondaryTrayWnd",
                "#32770",
                "Windows.UI.Core.CoreWindow",
                "XamlExplorerHostIslandWindow",
                "MultitaskingViewFrame",
            ]
            .contains(&name.as_str())
            || [
                "startmenuexperiencehost.exe",
                "searchhost.exe",
                "searchapp.exe",
                "shellexperiencehost.exe",
                "textinputhost.exe",
                "widgets.exe",
                "lockapp.exe",
            ]
            .contains(&executable)
        {
            return None;
        }
        for rule in &self.config.rules {
            if rule
                .executable
                .as_ref()
                .is_none_or(|v| v.eq_ignore_ascii_case(executable))
                && rule.class.as_ref().is_none_or(|v| v == &name)
            {
                if let Some(expected) = &rule.title_contains {
                    // Many apps have no title until WM_NCCREATE. Fall back rather
                    // than pre-position a window which might be floating/ignored.
                    if title.is_empty() {
                        return None;
                    }
                    if !title.contains(expected) {
                        continue;
                    }
                }
                if rule.mode != RuleMode::Tile {
                    return None;
                }
                break;
            }
        }
        let pointer = Point {
            x: request.point_x,
            y: request.point_y,
        };
        let monitor = self.monitors.iter().find(|m| m.full.contains(pointer))?;
        let pointer = Point {
            x: pointer
                .x
                .clamp(monitor.work.x, monitor.work.x + monitor.work.width - 1),
            y: pointer
                .y
                .clamp(monitor.work.y, monitor.work.y + monitor.work.height - 1),
        };
        if self.blocked_monitors.contains(&monitor.id) {
            return None;
        }
        let mut preview = self
            .layouts
            .get(&(self.current, monitor.id.clone()))
            .cloned()
            .unwrap_or_default();
        let id = request.window as usize;
        preview.insert(id, monitor.work, pointer, None);
        preview
            .tiles(monitor.work)
            .into_iter()
            .find(|(w, _)| *w == id)
            .map(|(_, r)| r)
    }
}

unsafe fn creation_request(sender: WPARAM, parameter: LPARAM) -> LRESULT {
    use crate::creation::protocol::*;
    unsafe {
        if parameter == 0 {
            return 0;
        }
        let data = &*(parameter as *const COPYDATASTRUCT);
        if data.dwData != MAGIC
            || data.cbData as usize != size_of::<Request>()
            || data.lpData.is_null()
        {
            return 0;
        }
        let mut request = std::ptr::read_unaligned(data.lpData.cast::<Request>());
        if request.phase > 1 || request.reserved != 0 {
            return 0;
        }
        let window = request.window as usize as HWND;
        if sender != window as usize
            || request.process == GetCurrentProcessId()
            || identity(window) != (request.process, request.thread)
        {
            return 0;
        }
        CREATION_PLAN.with(|state| {
            let Ok(state) = state.try_borrow() else {
                return 0;
            };
            let Some(plan) = state.as_ref().filter(|p| p.enabled) else {
                return 0;
            };
            if plan.test_process.is_some_and(|pid| pid != request.process) {
                return 0;
            }
            if let Some(point) = plan.test_point {
                request.point_x = point.x;
                request.point_y = point.y;
            }
            let image = executable(window);
            if image.is_empty() || plan.test_executable.as_ref().is_some_and(|v| v != &image) {
                return 0;
            }
            let Some(target) = plan.target(&request, &image) else {
                return 0;
            };
            let pointer = Point {
                x: request.point_x,
                y: request.point_y,
            };
            let Some(monitor) = plan.monitors.iter().find(|m| m.full.contains(pointer)) else {
                return 0;
            };
            let mut dpi = 96;
            let mut dpi_y = 96;
            GetDpiForMonitor(
                monitor.handle as HMONITOR,
                MDT_EFFECTIVE_DPI,
                &mut dpi,
                &mut dpi_y,
            );
            let border_x = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            let border_y = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            let properties = [
                (X, target.x - border_x),
                (Y, target.y),
                (WIDTH, target.width + border_x * 2),
                (HEIGHT, target.height + border_y),
                (
                    POINT_X,
                    pointer
                        .x
                        .clamp(monitor.work.x, monitor.work.x + monitor.work.width - 1),
                ),
                (
                    POINT_Y,
                    pointer
                        .y
                        .clamp(monitor.work.y, monitor.work.y + monitor.work.height - 1),
                ),
                (OWNER, GetCurrentProcessId() as i32),
            ];
            for (key, value) in properties {
                if SetPropW(window, key, value as isize as HANDLE) == 0 {
                    for (key, _) in properties {
                        RemovePropW(window, key);
                    }
                    return 0;
                }
            }
            1
        })
    }
}

struct WorkspaceFocus {
    desktop: u128,
    expires: Instant,
    explicit: bool,
}
impl WorkspaceFocus {
    fn new(desktop: u128, explicit: bool) -> Self {
        Self {
            desktop,
            expires: Instant::now() + Duration::from_secs(2),
            explicit,
        }
    }
    fn ready(&self, desktop: u128, paused: bool, follows_cursor: bool) -> bool {
        self.desktop == desktop
            && self.expires > Instant::now()
            && (self.explicit || (!paused && follows_cursor))
    }
}
struct Controller {
    config: Config,
    path: PathBuf,
    records: HashMap<WindowId, Record>,
    layouts: HashMap<(u128, String), Layout>,
    monitors: Vec<Monitor>,
    current: u128,
    current_slot: u32,
    paused: bool,
    dirty: bool,
    drag: Option<Drag>,
    native_drag: Option<NativeDrag>,
    pending_resize: Option<Point>,
    resize_frame_at: Option<Instant>,
    resize_frames: HashMap<WindowId, FrameInsets>,
    last_cursor: Point,
    workspace_focus: Option<WorkspaceFocus>,
    launcher_until: Instant,
    pending_launcher: bool,
    last_reconcile: Instant,
    desktop_events: Option<std::sync::mpsc::Receiver<winvd::DesktopEvent>>,
    _desktop_listener: Option<winvd::DesktopEventThread>,
    startup_windows: HashMap<WindowId, AdoptionContext>,
}
fn desktop_id(d: winvd::Desktop) -> Result<u128, winvd::Error> {
    d.get_id().map(|id| id.to_u128())
}
fn vd_hwnd(id: WindowId) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(id as *mut _)
}

impl Controller {
    fn new(config: Config, path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let current = if config.native_workspaces {
            desktop_id(winvd::get_current_desktop().map_err(|e|format!("native workspace backend unavailable ({e:?}); Windows 11 24H2+ is required. Set native_workspaces=false for tiling only."))?).map_err(|e|format!("Desktop ID query failed: {e:?}"))?
        } else {
            0
        };
        let (desktop_events, desktop_listener) = if config.native_workspaces {
            let (tx, rx) = std::sync::mpsc::channel::<winvd::DesktopEvent>();
            let listener = winvd::listen_desktop_events(tx)
                .map_err(|e| format!("Native desktop event subscription failed: {e:?}"))?;
            (Some(rx), Some(listener))
        } else {
            (None, None)
        };
        let monitors = monitors();
        let current_slot = if config.native_workspaces {
            winvd::get_desktop(windows::core::GUID::from_u128(current))
                .get_index()
                .map_err(|e| format!("Desktop slot query failed: {e:?}"))?
                + 1
        } else {
            1
        };
        // Capture eligibility before any layout is applied, including cloaked/minimized
        // windows. Delayed discovery must not turn an existing window into a new launch.
        let startup_windows = all_windows()
            .into_iter()
            .filter(|id| eligible(*id as HWND, &config))
            .filter_map(|id| adoption_context(id as HWND, &monitors).map(|context| (id, context)))
            .collect();
        Ok(Self {
            config,
            path,
            records: HashMap::new(),
            layouts: HashMap::new(),
            monitors,
            current,
            current_slot,
            paused: false,
            dirty: true,
            drag: None,
            native_drag: None,
            pending_resize: None,
            resize_frame_at: None,
            resize_frames: HashMap::new(),
            last_cursor: cursor(),
            workspace_focus: None,
            launcher_until: Instant::now(),
            pending_launcher: false,
            last_reconcile: Instant::now(),
            desktop_events,
            _desktop_listener: desktop_listener,
            startup_windows,
        })
    }
    fn publish_creation_plan(&self) {
        let blocked_monitors =
            self.records
                .iter()
                .filter(|(id, r)| {
                    r.desktop == self.current
                        && (r.fullscreen
                            || self.monitors.iter().any(|m| {
                                m.id == r.monitor && native_fullscreen(**id as HWND, m.full)
                            }))
                })
                .map(|(_, r)| r.monitor.clone())
                .collect();
        CREATION_PLAN.with(|s| {
            *s.borrow_mut() = Some(CreationPlan {
                config: self.config.clone(),
                layouts: self.layouts.clone(),
                monitors: self.monitors.clone(),
                current: self.current,
                enabled: !self.paused && self.drag.is_none() && self.native_drag.is_none(),
                blocked_monitors,
                test_process: None,
                test_executable: None,
                test_point: None,
                known_windows: self
                    .records
                    .keys()
                    .chain(self.startup_windows.keys())
                    .copied()
                    .collect(),
            })
        });
    }
    fn valid(&self, id: WindowId) -> bool {
        self.records.get(&id).is_some_and(
            |r| unsafe { IsWindow(id as HWND) != 0 } && identity(id as HWND) == r.identity,
        )
    }
    fn remove(&mut self, id: WindowId) {
        self.restore_topmost(id);
        self.end_fullscreen(id);
        self.restore_style(id);
        self.records.remove(&id);
        for l in self.layouts.values_mut() {
            l.remove(id);
        }
    }
    fn restore_style(&mut self, id: WindowId) {
        if self.valid(id)
            && let Some(r) = self.records.get_mut(&id)
            && r.removed_style != 0
        {
            unsafe {
                let h = id as HWND;
                let now = GetWindowLongPtrW(h, GWL_STYLE) as u32;
                SetWindowLongPtrW(h, GWL_STYLE, (now | r.removed_style) as isize);
                SetWindowPos(
                    h,
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOZORDER
                        | SWP_NOACTIVATE
                        | SWP_FRAMECHANGED
                        | SWP_ASYNCWINDOWPOS,
                );
            }
            r.removed_style = 0;
        }
    }
    fn style(&mut self, id: WindowId) {
        self.style_with_mask(id, STYLE_MASK);
    }
    fn style_with_mask(&mut self, id: WindowId, mask: u32) {
        if self.paused || !self.config.remove_caption_buttons {
            return;
        }
        if self.valid(id)
            && let Some(r) = self.records.get_mut(&id)
        {
            if r.style_failed {
                return;
            }
            unsafe {
                let h = id as HWND;
                let old = GetWindowLongPtrW(h, GWL_STYLE) as u32;
                let removed = old & mask;
                let restored = r.removed_style & !mask;
                if removed != 0 || restored != 0 {
                    SetLastError(0);
                    let result =
                        SetWindowLongPtrW(h, GWL_STYLE, ((old | restored) & !mask) as isize);
                    if result != 0 || GetLastError() == 0 {
                        r.removed_style = (r.removed_style | removed) & !restored;
                        SetWindowPos(
                            h,
                            null_mut(),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE
                                | SWP_NOSIZE
                                | SWP_NOZORDER
                                | SWP_NOACTIVATE
                                | SWP_FRAMECHANGED
                                | SWP_ASYNCWINDOWPOS,
                        );
                    } else {
                        r.style_failed = true;
                        report!(
                            "Cannot change caption styles for {id:#x}: {}",
                            std::io::Error::last_os_error()
                        );
                    }
                }
            }
        }
    }
    fn insert(&mut self, id: WindowId, p: Point) {
        let Some(r) = self.records.get(&id) else {
            return;
        };
        if r.minimized {
            return;
        }
        let Some(m) = self.monitors.iter().find(|m| m.id == r.monitor) else {
            return;
        };
        let focused = unsafe { GetForegroundWindow() as usize };
        self.layouts
            .entry((r.desktop, r.monitor.clone()))
            .or_default()
            .insert(id, m.work, p, Some(focused));
    }
    fn reconcile(&mut self) {
        self.last_reconcile = Instant::now();
        self.dirty = false;
        if self.config.native_workspaces {
            match winvd::get_current_desktop()
                .and_then(|d| Ok((desktop_id(d)?, d.get_index()? + 1)))
            {
                Ok((d, slot)) => {
                    if d != self.current
                        && !self
                            .workspace_focus
                            .as_ref()
                            .is_some_and(|f| f.explicit && f.expires > Instant::now())
                    {
                        self.workspace_focus = (!self.paused && self.config.focus_follows_cursor)
                            .then(|| WorkspaceFocus::new(d, false));
                    }
                    self.current = d;
                    self.current_slot = slot;
                }
                Err(e) => {
                    report!("Desktop query failed: {e:?}");
                    return;
                }
            }
        }
        let new = monitors();
        if new.is_empty() {
            return;
        }
        self.monitors = new;
        let stale: Vec<_> = self
            .records
            .keys()
            .copied()
            .filter(|id| !self.valid(*id))
            .collect();
        for id in stale {
            self.remove(id);
        }
        let migrated: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| !self.monitors.iter().any(|m| m.id == r.monitor))
            .map(|(id, _)| *id)
            .collect();
        for id in migrated {
            for l in self.layouts.values_mut() {
                l.remove(id);
            }
            self.records.get_mut(&id).unwrap().monitor = self.monitors[0].id.clone();
            if !self.records[&id].floating {
                self.insert(id, self.monitors[0].work.center());
            }
        }
        for id in all_windows() {
            let h = id as HWND;
            if let Some(record) = self.records.get(&id) {
                // Cloaked/hidden windows retain their trees on inactive native desktops.
                if self.config.native_workspaces
                    && let Ok(d) = winvd::get_desktop_by_window(vd_hwnd(id)).and_then(desktop_id)
                    && d != record.desktop
                {
                    for l in self.layouts.values_mut() {
                        l.remove(id);
                    }
                    let r = self.records.get_mut(&id).unwrap();
                    r.desktop = d;
                    if !r.floating {
                        self.insert(id, cursor());
                    }
                }
                continue;
            }
            if !eligible(h, &self.config) || cloaked(h) {
                continue;
            }
            if self.config.native_workspaces
                && winvd::is_pinned_window(vd_hwnd(id)).unwrap_or(false)
            {
                continue;
            }
            let desktop = if self.config.native_workspaces {
                match winvd::get_desktop_by_window(vd_hwnd(id)).and_then(desktop_id) {
                    Ok(d) => d,
                    Err(_) => continue,
                }
            } else {
                0
            };
            self.adopt(id, desktop, cursor());
        }
        self.layouts
            .retain(|_, l| !l.tiles(Rect::default()).is_empty());
        self.sync_minimized();
    }
    fn adopt(&mut self, id: WindowId, desktop: u128, pointer: Point) {
        let h = id as HWND;
        let pointer = crate::creation::take_pointer(h)
            .map(|(x, y)| Point { x, y })
            .unwrap_or(pointer);
        let handle = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) } as usize;
        let existing = self
            .startup_windows
            .get(&id)
            .filter(|context| context.identity == identity(h));
        let Some(m) = adoption_monitor(&self.monitors, handle, pointer, existing) else {
            return;
        };
        let insertion = existing
            .map(|context| context.point)
            .filter(|point| m.full.contains(*point))
            .unwrap_or_else(|| {
                if existing.is_some() {
                    m.work.center()
                } else {
                    pointer
                }
            });
        let monitor = m.id.clone();
        self.records.insert(
            id,
            Record {
                identity: identity(h),
                desktop,
                monitor,
                floating: rule_mode(h, &self.config) == Some(RuleMode::Floating),
                minimized: unsafe { IsIconic(h) != 0 },
                visibility_pending: None,
                topmost_owned: false,
                topmost_failed: false,
                topmost_attempt: None,
                fullscreen: false,
                paused_fullscreen_rect: None,
                removed_style: 0,
                removed_fullscreen_style: 0,
                style_failed: false,
                refusals: 0,
                last_attempt: None,
                desired: None,
            },
        );
        self.startup_windows.remove(&id);
        if !self.records[&id].floating {
            self.insert(id, insertion);
        } else {
            self.ensure_topmost(id);
        }
    }
    fn placement(&self) -> Vec<(WindowId, Rect)> {
        let mut result = vec![];
        for m in &self.monitors {
            if let Some(layout) = self.layouts.get(&(self.current, m.id.clone())) {
                let tiles = layout.tiles(m.work);
                let full = tiles
                    .iter()
                    .find(|(id, _)| {
                        self.records.get(id).is_some_and(|r| r.fullscreen)
                            || native_fullscreen(*id as HWND, m.full)
                    })
                    .map(|(id, _)| *id);
                if let Some(id) = full {
                    result.push((id, m.full));
                } else {
                    result.extend(tiles);
                }
            }
        }
        result
    }
    fn apply(&mut self) {
        self.apply_layout();
    }
    fn apply_layout(&mut self) {
        if self.paused {
            return;
        }
        for id in self
            .records
            .iter()
            .filter(|(_, r)| r.floating && r.desktop == self.current)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
        {
            self.ensure_topmost(id);
        }
        // Active resizing uses coalesced placement, without ordinary refusal backoff.
        if self.drag.as_ref().is_some_and(|d| d.right) {
            return;
        }
        if self.native_drag.is_some() || self.drag.as_ref().is_some_and(|d| !d.right) {
            return;
        }
        if self.drag.is_none() && user_mouse_interaction() {
            return;
        }
        let lone_tiles: HashSet<_> = self
            .monitors
            .iter()
            .filter_map(|monitor| {
                let tiles = self
                    .layouts
                    .get(&(self.current, monitor.id.clone()))?
                    .tiles(monitor.work);
                (tiles.len() == 1).then(|| tiles[0].0)
            })
            .collect();
        for (id, target) in self.placement() {
            let h = id as HWND;
            if !self.valid(id) || cloaked(h) || unsafe { IsWindowVisible(h) == 0 } {
                continue;
            }
            if !self.records[&id].fullscreen
                && self.monitors.iter().any(|m| native_fullscreen(h, m.full))
            {
                continue;
            }
            if self.records[&id].minimized || unsafe { IsIconic(h) != 0 } {
                continue;
            }
            let supports_maximize = (unsafe { GetWindowLongPtrW(h, GWL_STYLE) } as u32
                | self.records[&id].removed_style)
                & WS_MAXIMIZEBOX
                != 0;
            let lone =
                lone_tiles.contains(&id) && !self.records[&id].fullscreen && supports_maximize;
            // Windows needs the maximize capability to calculate work-area bounds.
            // Preserve it while hiding the other managed caption controls.
            let style_mask = if lone {
                STYLE_MASK & !WS_MAXIMIZEBOX
            } else {
                STYLE_MASK
            };
            let on_monitor = self.monitors.iter().any(|m| {
                m.id == self.records[&id].monitor
                    && unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) } as usize
                        == m.handle
            });
            if unsafe { IsZoomed(h) != 0 } {
                if lone && on_monitor {
                    self.style_with_mask(id, style_mask);
                    let record = self.records.get_mut(&id).unwrap();
                    record.desired = Some(target);
                    record.refusals = 0;
                    record.last_attempt = None;
                } else {
                    unsafe {
                        ShowWindowAsync(h, SW_RESTORE);
                    }
                }
                continue;
            }
            self.style_with_mask(id, style_mask);
            let Some(actual) = visible_rect(h) else {
                continue;
            };
            let record = self.records.get_mut(&id).unwrap();
            if record.desired != Some(target) {
                record.desired = Some(target);
                record.refusals = 0;
                record.last_attempt = None;
            }
            if !lone && actual.near(target, 2) {
                // Brief acceptance must not reset the refusal budget before geometry settles.
                if record
                    .last_attempt
                    .is_none_or(|last| last.elapsed() > Duration::from_secs(1))
                {
                    record.refusals = 0;
                    record.last_attempt = None;
                }
                continue;
            }
            if let Some(last) = record.last_attempt {
                if last.elapsed() < Duration::from_millis(250) {
                    continue;
                }
                record.refusals += 1;
                if record.refusals >= 3 {
                    report!("Floating geometry refusal: class={} ({id:#x})", class(h));
                    self.float(id, true);
                    continue;
                }
            }
            record.last_attempt = Some(Instant::now());
            if lone && on_monitor {
                maximize(h);
            } else {
                // Get the window onto its assigned monitor before asking Windows
                // to maximize it, so native maximization uses the right work area.
                place(h, target);
            }
        }
    }
    fn float(&mut self, id: WindowId, floating: bool) {
        self.end_fullscreen(id);
        self.restore_style(id);
        if floating && self.valid(id) && unsafe { IsZoomed(id as HWND) != 0 } {
            unsafe {
                ShowWindowAsync(id as HWND, SW_RESTORE);
            }
        }
        if !floating {
            self.restore_topmost(id);
        }
        if let Some(r) = self.records.get_mut(&id) {
            r.floating = floating;
            r.fullscreen = false;
            r.refusals = 0;
            r.last_attempt = None;
            r.topmost_failed = false;
            r.topmost_attempt = None;
        }
        for l in self.layouts.values_mut() {
            l.remove(id);
        }
        if !floating {
            self.insert(id, cursor());
        } else {
            self.ensure_topmost(id);
        }
    }
    fn ensure_topmost(&mut self, id: WindowId) {
        if self.paused || !self.valid(id) {
            return;
        }
        let h = id as HWND;
        if unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 } {
            return;
        }
        let r = self.records.get_mut(&id).unwrap();
        if r.topmost_failed
            || r.topmost_attempt
                .is_some_and(|last| last.elapsed() < Duration::from_millis(250))
        {
            return;
        }
        r.topmost_attempt = Some(Instant::now());
        if set_topmost(h, true) {
            r.topmost_owned = true;
        } else {
            r.topmost_failed = true;
            report!(
                "Cannot keep floating window {id:#x} on top: {}",
                std::io::Error::last_os_error()
            );
        }
    }
    fn restore_topmost(&mut self, id: WindowId) {
        if self.valid(id)
            && let Some(r) = self.records.get_mut(&id)
            && r.topmost_owned
        {
            if set_topmost(id as HWND, false) {
                r.topmost_owned = false;
            } else {
                report!(
                    "Cannot restore floating window stacking for {id:#x}: {}",
                    std::io::Error::last_os_error()
                );
            }
            r.topmost_attempt = None;
        }
    }
    fn end_fullscreen(&mut self, id: WindowId) {
        if self.valid(id)
            && let Some(r) = self.records.get_mut(&id)
        {
            if r.removed_fullscreen_style != 0 {
                unsafe {
                    let h = id as HWND;
                    let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
                    SetWindowLongPtrW(h, GWL_STYLE, (style | r.removed_fullscreen_style) as isize);
                    SetWindowPos(
                        h,
                        null_mut(),
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED
                            | SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_NOZORDER
                            | SWP_NOACTIVATE
                            | SWP_ASYNCWINDOWPOS,
                    );
                }
                r.removed_fullscreen_style = 0;
                mark_fullscreen(id, false);
            }
            r.fullscreen = false;
            if let Some(rect) = r.paused_fullscreen_rect.take() {
                place(id as HWND, rect);
            }
        }
    }
    fn fullscreen(&mut self, id: WindowId) {
        if !self.valid(id) || self.records[&id].floating {
            return;
        }
        if self.records[&id].fullscreen {
            self.end_fullscreen(id);
            return;
        }
        let monitor = self.records[&id].monitor.clone();
        let restore_rect = self.paused.then(|| visible_rect(id as HWND)).flatten();
        for other in self
            .records
            .iter()
            .filter(|(other, r)| {
                **other != id && r.monitor == monitor && r.desktop == self.current && r.fullscreen
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
        {
            self.end_fullscreen(other);
        }
        unsafe {
            let h = id as HWND;
            let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
            let mask = WS_CAPTION | WS_THICKFRAME;
            SetLastError(0);
            let previous = SetWindowLongPtrW(h, GWL_STYLE, (style & !mask) as isize);
            if previous == 0 && GetLastError() != 0 {
                report!(
                    "Cannot enter fullscreen: {}",
                    std::io::Error::last_os_error()
                );
                return;
            }
            let r = self.records.get_mut(&id).unwrap();
            r.paused_fullscreen_rect = restore_rect;
            r.removed_fullscreen_style = style & mask;
            r.fullscreen = true;
            SetWindowPos(
                h,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | SWP_ASYNCWINDOWPOS,
            );
        }
        mark_fullscreen(id, true);
        if self.paused
            && let Some(monitor) = self.monitors.iter().find(|m| m.id == monitor)
        {
            place(id as HWND, monitor.full);
        }
    }
    fn tick(&mut self) {
        self.sync_minimized();
        if self.drag.as_ref().is_some_and(|d| d.right) {
            self.paint_resize(false);
            return;
        }
        if let Some(rx) = &self.desktop_events {
            for _ in rx.try_iter() {
                self.dirty = true;
            }
        }
        if self.pending_launcher {
            let held = INPUT_STATE.with(|s| s.borrow().as_ref().is_some_and(|i| i.super_down()));
            if !held {
                unsafe {
                    keybd_event(VK_CONTROL as u8, 0, 0, 0);
                    keybd_event(VK_ESCAPE as u8, 0, 0, 0);
                    keybd_event(VK_ESCAPE as u8, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
                }
                self.pending_launcher = false;
                self.launcher_until = Instant::now() + Duration::from_secs(2);
            }
        }
        if self.drag.is_none()
            && self.native_drag.is_none()
            && (self.dirty || self.last_reconcile.elapsed() > Duration::from_secs(2))
        {
            self.reconcile();
        }
        self.apply();
        let p = cursor();
        let moved = p != self.last_cursor;
        self.last_cursor = p;
        if self
            .workspace_focus
            .as_ref()
            .is_some_and(|f| f.expires <= Instant::now())
        {
            self.workspace_focus = None;
        }
        let workspace_focus = self
            .workspace_focus
            .as_ref()
            .is_some_and(|f| f.ready(self.current, self.paused, self.config.focus_follows_cursor));
        if ((moved && !self.paused && self.config.focus_follows_cursor) || workspace_focus)
            && self.drag.is_none()
            && self.native_drag.is_none()
            && (workspace_focus || Instant::now() > self.launcher_until)
            && (!workspace_focus || self.workspace_focus_geometry_ready(p))
            && self.focus_under_cursor(p)
            && workspace_focus
        {
            self.workspace_focus = None;
        }
    }
    fn workspace_focus_geometry_ready(&self, point: Point) -> bool {
        let Some(monitor) = self.monitors.iter().find(|m| m.full.contains(point)) else {
            return false;
        };
        self.placement()
            .iter()
            .filter(|(id, _)| {
                self.records
                    .get(id)
                    .is_some_and(|r| r.monitor == monitor.id)
            })
            .all(|(id, target)| {
                self.valid(*id)
                    && !cloaked(*id as HWND)
                    && visible_rect(*id as HWND).is_some_and(|r| r.near(*target, 2))
            })
    }
    fn focus_under_cursor(&self, point: Point) -> bool {
        unsafe {
            if GetAsyncKeyState(VK_LBUTTON as i32) < 0 || GetAsyncKeyState(VK_RBUTTON as i32) < 0 {
                return false;
            }
            let fg = GetForegroundWindow();
            if !fg.is_null() && (!GetWindow(fg, GW_OWNER).is_null() || IsWindowEnabled(fg) == 0) {
                return false;
            }
            let mut gui: GUITHREADINFO = zeroed();
            gui.cbSize = size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(0, &mut gui) != 0
                && (gui.flags
                    & (GUI_INMENUMODE | GUI_INMOVESIZE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE)
                    != 0
                    || !gui.hwndCapture.is_null())
            {
                return false;
            }
            if [
                "startmenuexperiencehost.exe",
                "searchhost.exe",
                "searchapp.exe",
                "shellexperiencehost.exe",
            ]
            .contains(&executable(fg).as_str())
            {
                return false;
            }
            let hovered = GetAncestor(
                WindowFromPoint(POINT {
                    x: point.x,
                    y: point.y,
                }),
                GA_ROOT,
            );
            let id = hovered as usize;
            if self
                .records
                .get(&id)
                .is_some_and(|r| r.desktop == self.current && !r.minimized)
                && self.valid(id)
                && !cloaked(hovered)
                && IsWindowVisible(hovered) != 0
                && IsWindowEnabled(hovered) != 0
            {
                return hovered == fg || SetForegroundWindow(hovered) != 0;
            }
            false
        }
    }
    fn workspace(&mut self, slot: u32, moving: bool, id: WindowId) -> Result<(), Box<dyn Error>> {
        if !self.config.native_workspaces {
            return Err("native workspaces are disabled".into());
        }
        let mut count = winvd::get_desktop_count().map_err(|e| format!("{e:?}"))?;
        while count < slot {
            winvd::create_desktop().map_err(|e| format!("{e:?}"))?;
            count += 1;
        }
        if moving {
            if !self.valid(id) {
                return Ok(());
            }
            self.end_fullscreen(id);
            winvd::move_window_to_desktop(slot - 1, &vd_hwnd(id)).map_err(|e| format!("{e:?}"))?;
            let desktop = desktop_id(winvd::get_desktop(slot - 1)).map_err(|e| format!("{e:?}"))?;
            for l in self.layouts.values_mut() {
                l.remove(id);
            }
            let r = self.records.get_mut(&id).unwrap();
            r.desktop = desktop;
            if !r.floating {
                self.insert(id, cursor());
            }
        }
        self.dirty = true;
        winvd::switch_desktop(slot - 1).map_err(|e| format!("{e:?}"))?;
        let destination = desktop_id(winvd::get_desktop(slot - 1)).map_err(|e| format!("{e:?}"))?;
        self.workspace_focus = Some(WorkspaceFocus::new(destination, true));
        Ok(())
    }
    fn action(&mut self, action: Action) -> bool {
        let id = unsafe { GetForegroundWindow() as usize };
        match action {
            Action::Exit => return false,
            Action::Minimize => {
                self.set_minimized(id, true);
            }
            Action::RestoreMinimized => {
                let p = cursor();
                let monitor = self
                    .monitors
                    .iter()
                    .find(|m| m.full.contains(p))
                    .map(|m| m.id.clone())
                    .or_else(|| self.records.get(&id).map(|r| r.monitor.clone()));
                if let Some(monitor) = monitor {
                    self.restore_monitor(&monitor);
                }
            }
            Action::Pause => {
                self.paused = !self.paused;
                self.drag = None;
                self.native_drag = None;
                self.pending_resize = None;
                self.resize_frames.clear();
                self.resize_frame_at = None;
                if self.paused {
                    for id in self.records.keys().copied().collect::<Vec<_>>() {
                        self.end_fullscreen(id);
                        self.restore_style(id);
                        self.restore_topmost(id);
                    }
                } else {
                    // Respect monitor changes made with ordinary Windows controls while paused.
                    for id in self.records.keys().copied().collect::<Vec<_>>() {
                        if !self.valid(id) {
                            continue;
                        }
                        let handle =
                            unsafe { MonitorFromWindow(id as HWND, MONITOR_DEFAULTTONEAREST) }
                                as usize;
                        if let Some(monitor) = self.monitors.iter().find(|m| m.handle == handle)
                            && monitor.id != self.records[&id].monitor
                        {
                            let point = visible_rect(id as HWND)
                                .map(|r| r.center())
                                .unwrap_or(monitor.work.center());
                            self.reinsert(id, point);
                        }
                    }
                    self.dirty = true;
                    for record in self.records.values_mut() {
                        record.refusals = 0;
                        record.last_attempt = None;
                        record.desired = None;
                    }
                }
                report!(
                    "Automatic tiling {} (bindings remain active)",
                    if self.paused { "paused" } else { "resumed" }
                );
            }
            Action::Reload => match config::load(&self.path) {
                Ok(c) => {
                    if c.native_workspaces != self.config.native_workspaces {
                        report!("Changing native_workspaces requires restarting Tauiler");
                        return true;
                    }
                    INPUT_STATE.with(|s| {
                        if let Some(i) = s.borrow_mut().as_mut() {
                            i.update_config(&c);
                        }
                    });
                    if !c.remove_caption_buttons {
                        for id in self.records.keys().copied().collect::<Vec<_>>() {
                            self.restore_style(id);
                        }
                    }
                    self.config = c;
                    let released: Vec<_> = self
                        .records
                        .keys()
                        .copied()
                        .filter(|id| !eligible(*id as HWND, &self.config))
                        .collect();
                    for id in released {
                        self.remove(id);
                    }
                    self.dirty = true;
                    report!("Configuration reloaded");
                }
                Err(e) => report!("Reload rejected; previous configuration retained: {e}"),
            },
            Action::Launcher => self.pending_launcher = true,
            Action::Launch { executable, args } => {
                if let Err(e) = std::process::Command::new(&executable).args(args).spawn() {
                    report!("Cannot launch {executable}: {e}");
                }
            }
            Action::Workspace { slot } => {
                if let Err(e) = self.workspace(slot, false, id) {
                    report!("Workspace failed: {e}");
                }
            }
            Action::MoveWorkspace { slot } => {
                if let Err(e) = self.workspace(slot, true, id) {
                    report!("Move workspace failed: {e}");
                }
            }
            Action::Close => {
                let owner = unsafe { GetWindow(id as HWND, GW_OWNER) } as usize;
                if self.valid(id)
                    || (self.valid(owner) && unsafe { IsWindowEnabled(id as HWND) != 0 })
                {
                    unsafe {
                        PostMessageW(id as HWND, WM_CLOSE, 0, 0);
                    }
                }
            }
            Action::ToggleFloating => {
                if let Some(r) = self.records.get(&id) {
                    self.float(id, !r.floating);
                }
            }
            Action::Fullscreen => {
                self.fullscreen(id);
            }
            Action::Focus { direction } | Action::Move { direction } => {
                if let Some(next) = self.neighbor(id, direction) {
                    if matches!(action, Action::Move { .. }) {
                        let p = visible_rect(next as HWND).unwrap_or_default().center();
                        if self.paused {
                            if let Some(rect) = visible_rect(id as HWND) {
                                place(
                                    id as HWND,
                                    Rect {
                                        x: p.x - rect.width / 2,
                                        y: p.y - rect.height / 2,
                                        ..rect
                                    },
                                );
                            }
                        } else {
                            self.reinsert(id, p);
                        }
                    } else {
                        self.workspace_focus = None;
                        unsafe {
                            SetForegroundWindow(next as HWND);
                        }
                    }
                }
            }
            Action::Resize { direction } => {
                let (dx, dy) = match direction {
                    Direction::Left => (-40, 0),
                    Direction::Right => (40, 0),
                    Direction::Up => (0, -40),
                    Direction::Down => (0, 40),
                };
                self.resize(id, dx, dy);
            }
        }
        self.apply_layout();
        true
    }
    fn neighbor(&self, id: WindowId, direction: Direction) -> Option<WindowId> {
        let origin = visible_rect(id as HWND)?.center();
        self.records
            .iter()
            .filter(|(w, r)| {
                **w != id
                    && r.desktop == self.current
                    && !r.minimized
                    && self.valid(**w)
                    && !cloaked(**w as HWND)
                    && unsafe {
                        IsWindowVisible(**w as HWND) != 0 && IsWindowEnabled(**w as HWND) != 0
                    }
            })
            .filter_map(|(w, _)| {
                let p = visible_rect(*w as HWND)?.center();
                let dx = i64::from(p.x - origin.x);
                let dy = i64::from(p.y - origin.y);
                let (forward, cross) = match direction {
                    Direction::Left => (-dx, dy),
                    Direction::Right => (dx, dy),
                    Direction::Up => (-dy, dx),
                    Direction::Down => (dy, dx),
                };
                (forward > 0).then_some((*w, forward * forward + cross * cross * 4))
            })
            .min_by_key(|(id, score)| (*score, *id))
            .map(|(id, _)| id)
    }
    fn reinsert(&mut self, id: WindowId, p: Point) {
        let Some(m) = self.monitors.iter().find(|m| m.full.contains(p)).cloned() else {
            return;
        };
        self.end_fullscreen(id);
        let Some(r) = self.records.get_mut(&id) else {
            return;
        };
        r.monitor = m.id;
        r.fullscreen = false;
        if !r.floating {
            for l in self.layouts.values_mut() {
                l.remove(id);
            }
            self.insert(id, p);
        }
    }
    fn resize(&mut self, id: WindowId, dx: i32, dy: i32) {
        if self.paused {
            if self.valid(id)
                && let Some(rect) = visible_rect(id as HWND)
            {
                place(
                    id as HWND,
                    Rect {
                        width: (rect.width + dx).max(64),
                        height: (rect.height + dy).max(64),
                        ..rect
                    },
                );
            }
            return;
        }
        let Some(r) = self.records.get(&id) else {
            return;
        };
        let Some(m) = self.monitors.iter().find(|m| m.id == r.monitor) else {
            return;
        };
        if let Some(l) = self.layouts.get_mut(&(r.desktop, r.monitor.clone())) {
            l.resize(id, dx, dy, m.work);
        }
    }
    fn set_minimized(&mut self, id: WindowId, minimized: bool) {
        if !self.valid(id) {
            return;
        }
        if minimized {
            self.end_fullscreen(id);
        }
        if unsafe { ShowWindowAsync(id as HWND, if minimized { SW_MINIMIZE } else { SW_RESTORE }) }
            == 0
        {
            report!(
                "Cannot {} window {id:#x}: {}",
                if minimized { "minimize" } else { "restore" },
                std::io::Error::last_os_error()
            );
            return;
        }
        let record = self.records.get_mut(&id).unwrap();
        record.minimized = minimized;
        record.visibility_pending = Some((minimized, Instant::now()));
        record.desired = None;
        record.refusals = 0;
        record.last_attempt = None;
        for layout in self.layouts.values_mut() {
            layout.remove(id);
        }
        if !minimized && !self.records[&id].floating {
            self.insert(id, cursor());
        }
    }
    fn restore_monitor(&mut self, monitor: &str) {
        let windows = self
            .records
            .iter()
            .filter(|(id, r)| {
                r.desktop == self.current
                    && r.monitor == monitor
                    && (r.minimized || unsafe { IsIconic(**id as HWND) != 0 })
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in windows {
            self.set_minimized(id, false);
        }
    }
    fn sync_minimized(&mut self) {
        for id in self.records.keys().copied().collect::<Vec<_>>() {
            if !self.valid(id) {
                continue;
            }
            let actual = unsafe { IsIconic(id as HWND) != 0 };
            let record = self.records.get_mut(&id).unwrap();
            if let Some((requested, since)) = record.visibility_pending {
                if requested != actual && since.elapsed() < Duration::from_secs(1) {
                    continue;
                }
                record.visibility_pending = None;
                if requested != actual {
                    report!(
                        "Window {id:#x} refused a {} request",
                        if requested { "minimize" } else { "restore" }
                    );
                }
            }
            if record.minimized == actual {
                continue;
            }
            record.minimized = actual;
            record.desired = None;
            record.refusals = 0;
            record.last_attempt = None;
            for layout in self.layouts.values_mut() {
                layout.remove(id);
            }
            if !actual && !self.records[&id].floating {
                self.insert(id, cursor());
            }
        }
    }
    fn paint_resize(&mut self, force: bool) {
        if !force
            && self
                .resize_frame_at
                .is_some_and(|last| last.elapsed() < Duration::from_millis(16))
        {
            return;
        }
        let Some(p) = self.pending_resize.take() else {
            return;
        };
        let Some(d) = self.drag.as_ref() else {
            return;
        };
        let id = d.id;
        if !d.right || !self.valid(id) {
            return;
        }
        let target = d.target(p);
        let dx = p.x - d.origin.x;
        let dy = p.y - d.origin.y;
        let layout = d.start_layout.clone();
        let r = &self.records[&id];
        let key = (r.desktop, r.monitor.clone());
        let floating = r.floating || self.paused;
        let targets = if floating {
            vec![(id, target)]
        } else {
            if let Some(layout) = layout {
                self.layouts.insert(key.clone(), layout);
            }
            self.resize(id, dx, dy);
            let Some(monitor) = self.monitors.iter().find(|m| m.id == key.1) else {
                return;
            };
            self.layouts
                .get(&key)
                .map(|l| l.tiles(monitor.work))
                .unwrap_or_default()
        };
        for (id, target) in targets {
            if !self.valid(id) {
                continue;
            }
            if !self.resize_frames.contains_key(&id) {
                if !floating {
                    self.style(id);
                }
                let Some(frame) = frame_insets(id as HWND) else {
                    continue;
                };
                self.resize_frames.insert(id, frame);
            }
            let record = self.records.get_mut(&id).unwrap();
            if record.desired == Some(target) {
                continue;
            }
            record.desired = Some(target);
            record.refusals = 0;
            record.last_attempt = None;
            place_cached(id as HWND, target, self.resize_frames[&id]);
        }
        self.resize_frame_at = Some(Instant::now());
    }
    fn event(&mut self, event: Event) -> bool {
        match event {
            Event::TrayMenu => {} // The runtime owns the menu resources; see dispatch_event.
            Event::Action(a) => return self.action(a),
            Event::Dirty => self.dirty = true,
            Event::Destroy(id) => {
                self.startup_windows.remove(&id);
                if self.drag.as_ref().is_some_and(|d| d.id == id) {
                    self.drag = None;
                    self.pending_resize = None;
                    self.resize_frames.clear();
                }
                if self.native_drag.as_ref().is_some_and(|d| d.id == id) {
                    self.native_drag = None;
                }
                // Never restore stale style bits to a handle that may already have been reused.
                self.records.remove(&id);
                for l in self.layouts.values_mut() {
                    l.remove(id);
                }
                self.dirty = true;
            }
            Event::DragStart(p, right) => {
                let h = unsafe { GetAncestor(WindowFromPoint(POINT { x: p.x, y: p.y }), GA_ROOT) };
                let id = h as usize;
                if self.valid(id)
                    && let Some(start_rect) = visible_rect(h)
                {
                    if unsafe { IsZoomed(h) != 0 } {
                        unsafe {
                            ShowWindowAsync(h, SW_RESTORE);
                        }
                    }
                    let r = &self.records[&id];
                    let start_layout = self.layouts.get(&(r.desktop, r.monitor.clone())).cloned();
                    self.drag = Some(Drag {
                        id,
                        right,
                        origin: p,
                        start_rect,
                        start_layout,
                    });
                    self.pending_resize = None;
                    self.resize_frame_at = None;
                    self.resize_frames.clear();
                    unsafe {
                        SetForegroundWindow(h);
                    }
                }
            }
            Event::DragMove(p) => {
                if let Some(d) = self.drag.as_ref() {
                    let id = d.id;
                    let right = d.right;
                    if !self.valid(id) {
                        self.drag = None;
                        return true;
                    }
                    let target = d.target(p);
                    if right {
                        self.pending_resize = Some(p);
                        self.paint_resize(false);
                    } else {
                        place(id as HWND, target);
                    }
                }
            }
            Event::DragEnd(p) => {
                // Apply the release coordinate too, even when movement events were coalesced.
                self.event(Event::DragMove(p));
                self.paint_resize(true);
                if let Some(d) = self.drag.take() {
                    self.pending_resize = None;
                    self.resize_frames.clear();
                    self.resize_frame_at = None;
                    if !d.right && !self.paused {
                        self.reinsert(d.id, p);
                    }
                    self.dirty = true;
                    self.apply_layout();
                }
            }
            Event::NativeDragStart(id) => {
                if self.drag.is_none()
                    && self.native_drag.is_none()
                    && self.valid(id)
                    && let Some(start_rect) = visible_rect(id as HWND)
                {
                    self.native_drag = Some(NativeDrag { id, start_rect });
                }
            }
            Event::NativeDragEnd(id) => {
                if self.native_drag.as_ref().is_some_and(|d| d.id == id) {
                    let d = self.native_drag.take().unwrap();
                    if !self.paused
                        && self.valid(id)
                        && let Some(actual) = visible_rect(id as HWND)
                        && !actual.near(d.start_rect, 2)
                    {
                        if self.records[&id].floating {
                            self.reinsert(id, actual.center());
                        } else if actual.width != d.start_rect.width
                            || actual.height != d.start_rect.height
                        {
                            let dx = if actual.x != d.start_rect.x {
                                actual.x - d.start_rect.x
                            } else {
                                actual.width - d.start_rect.width
                            };
                            let dy = if actual.y != d.start_rect.y {
                                actual.y - d.start_rect.y
                            } else {
                                actual.height - d.start_rect.height
                            };
                            self.resize(id, dx, dy);
                        } else {
                            self.reinsert(id, cursor());
                        }
                    }
                    self.last_cursor = cursor();
                    self.dirty = true;
                    self.apply();
                }
            }
        }
        true
    }
}
fn user_mouse_interaction() -> bool {
    // Integration controllers only own disposable windows. User input must not
    // make their asynchronous geometry assertions nondeterministic.
    if NATIVE_TEST.with(Cell::get) {
        return false;
    }
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON as i32) < 0 || GetAsyncKeyState(VK_RBUTTON as i32) < 0 {
            return true;
        }
        let mut gui: GUITHREADINFO = zeroed();
        gui.cbSize = size_of::<GUITHREADINFO>() as u32;
        GetGUIThreadInfo(0, &mut gui) != 0 && gui.flags & GUI_INMOVESIZE != 0
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        for id in self.records.keys().copied().collect::<Vec<_>>() {
            self.restore_topmost(id);
            self.end_fullscreen(id);
            self.restore_style(id);
        }
    }
}
fn set_topmost(hwnd: HWND, topmost: bool) -> bool {
    unsafe {
        SetWindowPos(
            hwnd,
            if topmost {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS,
        ) != 0
    }
}
fn native_fullscreen(hwnd: HWND, area: Rect) -> bool {
    (unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == 0 })
        && visible_rect(hwnd).is_some_and(|r| r.near(area, 2))
}
fn mark_fullscreen(id: WindowId, full: bool) {
    use windows::Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Shell::{ITaskbarList2, TaskbarList},
    };
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(taskbar) =
            CoCreateInstance::<_, ITaskbarList2>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
        {
            let _ = taskbar.HrInit();
            if let Err(e) = taskbar.MarkFullscreenWindow(vd_hwnd(id), full) {
                report!("Taskbar fullscreen notification failed: {e}");
            }
        }
        if init.is_ok() {
            CoUninitialize();
        }
    }
}
fn maximize(hwnd: HWND) {
    unsafe {
        let mut placement: WINDOWPLACEMENT = zeroed();
        placement.length = size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut placement) != 0 {
            placement.flags = WPF_ASYNCWINDOWPLACEMENT;
            placement.showCmd = SW_SHOWMAXIMIZED as u32;
            SetWindowPlacement(hwnd, &placement);
        }
    }
}
fn place(hwnd: HWND, target: Rect) {
    unsafe {
        if let (Some(outer), Some(visible)) = (window_rect(hwnd), visible_rect(hwnd)) {
            let x = target.x + outer.x - visible.x;
            let y = target.y + outer.y - visible.y;
            let width = (target.width + outer.width - visible.width).max(1);
            let height = (target.height + outer.height - visible.height).max(1);
            // Async placement avoids blocking on hung foreign applications. Verify on a later tick.
            SetWindowPos(
                hwnd,
                null_mut(),
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS | SWP_NOSENDCHANGING,
            );
        }
    }
}

pub fn run(config: Config, path: PathBuf, args: &[String]) -> Result<(), Box<dyn Error>> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    if args.iter().any(|a| a == "--self-test") {
        return self_test(&config);
    }
    if args.iter().any(|a| a == "--inspect") {
        println!("Monitors: {:#?}", monitors());
        for id in all_windows() {
            if eligible(id as HWND, &config) {
                println!(
                    "{id:#x}: {} [{}] {:?} cloaked={}",
                    title(id as HWND),
                    class(id as HWND),
                    visible_rect(id as HWND),
                    cloaked(id as HWND)
                );
            }
        }
        if config.native_workspaces {
            println!(
                "Native workspaces: {:?}; current: {:?}",
                winvd::get_desktop_count(),
                winvd::get_current_desktop().and_then(|d| d.get_index())
            );
        }
        return Ok(());
    }
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, include_str!("../config.example.toml"))?;
    }
    report!("Starting Tauiler {}", env!("CARGO_PKG_VERSION"));
    THREAD_ID.with(|id| *id.borrow_mut() = unsafe { GetCurrentThreadId() });
    INPUT_STATE.with(|s| *s.borrow_mut() = Some(Input::new(&config)));
    let mut resources = Resources::new()?;
    let mut timer_interval = 50;
    let taskbar_created = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    let mut controller = Controller::new(config, path)?;
    controller.reconcile();
    controller.apply();
    controller.publish_creation_plan();
    let creation_hook = match crate::creation::CreationHook::install(resources.window) {
        Ok(hook) => {
            report!("Window creation hook installed");
            Some(hook)
        }
        Err(error) => {
            report!("Creation hook unavailable; using normal adoption: {error}");
            None
        }
    };
    resources.update_icon(controller.current_slot, controller.paused)?;
    println!(
        "Tauiler running. SUPER+V toggles floating; click the workspace icon or SUPER+M to pause tiling. Bindings remain active. Right-click the icon to exit."
    );
    unsafe {
        let mut msg: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut msg, null_mut(), 0, 0);
            if result == 0 {
                break;
            }
            if result < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if msg.message == taskbar_created && taskbar_created != 0 {
                Shell_NotifyIconW(NIM_ADD, &resources.tray);
                TRAY_MODERN.with(|state| {
                    state.set(Shell_NotifyIconW(NIM_SETVERSION, &resources.tray) != 0)
                });
                controller.dirty = true;
            }
            if msg.message == WM_TIMER {
                controller.tick();
            }
            if msg.message == WM_DISPLAYCHANGE || msg.message == WM_SETTINGCHANGE {
                controller.dirty = true;
            }
            let mut running = true;
            let mut publish_creation = msg.message == WM_TIMER
                || msg.message == WM_DISPLAYCHANGE
                || msg.message == WM_SETTINGCHANGE;
            loop {
                let next = EVENTS.with(|q| q.borrow_mut().pop_front());
                let Some(e) = next else {
                    break;
                };
                if !matches!(e, Event::Dirty | Event::DragMove(_)) {
                    publish_creation = true;
                }
                if matches!(e, Event::Dirty) {
                    DIRTY_QUEUED.with(|d| d.set(false));
                }
                if !dispatch_event(&mut controller, &resources, e) {
                    running = false;
                    break;
                }
            }
            if !running {
                break;
            }
            if publish_creation {
                controller.publish_creation_plan();
            }
            MANAGED.with(|set| *set.borrow_mut() = controller.records.keys().copied().collect());
            resources.update_icon(controller.current_slot, controller.paused)?;
            let next_interval = if controller.drag.as_ref().is_some_and(|d| d.right) {
                16
            } else {
                50
            };
            if next_interval != timer_interval {
                SetTimer(resources.window, 1, next_interval, None);
                timer_interval = next_interval;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    CREATION_PLAN.with(|s| *s.borrow_mut() = None);
    drop(creation_hook);
    drop(controller);
    drop(resources);
    INPUT_STATE.with(|s| *s.borrow_mut() = None);
    report!("Tauiler exited; hooks released and owned styles restored");
    Ok(())
}
fn dispatch_event(controller: &mut Controller, resources: &Resources, event: Event) -> bool {
    if matches!(event, Event::TrayMenu) {
        return resources
            .menu(&controller.path, controller.paused)
            .is_none_or(|action| controller.action(action));
    }
    controller.event(event)
}

struct NativeTestScope;
impl Drop for NativeTestScope {
    fn drop(&mut self) {
        NATIVE_TEST.with(|state| state.set(false));
    }
}
fn self_test(settings: &Config) -> Result<(), Box<dyn Error>> {
    NATIVE_TEST.with(|state| state.set(true));
    let _input_scope = NativeTestScope;
    unsafe {
        let name = wide("Tauiler.SelfTest");
        let module = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: module,
            lpszClassName: name.as_ptr(),
            ..zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let a = CreateWindowExW(
            0,
            name.as_ptr(),
            wide("Tauiler isolated test A").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            100,
            100,
            500,
            400,
            null_mut(),
            null_mut(),
            module,
            std::ptr::null(),
        );
        let b = CreateWindowExW(
            0,
            name.as_ptr(),
            wide("Tauiler isolated test B").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            200,
            100,
            500,
            400,
            null_mut(),
            null_mut(),
            module,
            std::ptr::null(),
        );
        if a.is_null() || b.is_null() {
            if !a.is_null() {
                DestroyWindow(a);
            }
            if !b.is_null() {
                DestroyWindow(b);
            }
            return Err("cannot create self-test windows".into());
        }
        ShowWindow(a, SW_SHOWNOACTIVATE);
        ShowWindow(b, SW_SHOWNOACTIVATE);
        let result = (|| -> Result<(), Box<dyn Error>> {
            // Exercise real resource installation/cleanup and native desktop notifications,
            // but never adopt any existing application windows in this test.
            THREAD_ID.with(|id| *id.borrow_mut() = GetCurrentThreadId());
            let mut resources = Resources::new()?;
            for slot in 1..=10 {
                for paused in [false, true] {
                    resources.update_icon(slot, paused)?;
                    let mut info: ICONINFO = zeroed();
                    if GetIconInfo(resources.tray.hIcon, &mut info) == 0 {
                        return Err("numbered tray icon handle is invalid".into());
                    }
                    DeleteObject(info.hbmColor);
                    DeleteObject(info.hbmMask);
                    if resources.tray_state != Some((slot, paused)) {
                        return Err("tray workspace/pause state did not update".into());
                    }
                }
            }
            resources.update_icon(1, false)?;
            let desktop_probe = Controller::new(settings.clone(), PathBuf::from("unused"))?;
            drop(desktop_probe);
            test_creation_override(resources.window)?;
            test_monitor_adoption(a, b)?;
            test_workspace_cursor_focus(a, b)?;
            let area = monitors().first().ok_or("no native test monitor")?.work;
            let mut l = Layout::default();
            l.insert(a as usize, area, area.center(), None);
            l.insert(b as usize, area, Point { x: 900, y: 200 }, None);
            for (id, target) in l.tiles(area) {
                place(id as HWND, target);
            }
            let mut msg: MSG = zeroed();
            for _ in 0..10 {
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            for (id, target) in l.tiles(area) {
                let actual = visible_rect(id as HWND).ok_or("test window bounds unavailable")?;
                if !actual.near(target, 2) {
                    return Err(
                        format!("native placement mismatch: {actual:?} != {target:?}").into(),
                    );
                }
            }
            let mut c = Controller::new(
                Config {
                    native_workspaces: false,
                    ..Config::default()
                },
                PathBuf::from("unused"),
            )?;
            c.records.insert(
                a as usize,
                Record {
                    identity: identity(a),
                    desktop: 0,
                    monitor: String::new(),
                    floating: false,
                    minimized: false,
                    visibility_pending: None,
                    topmost_owned: false,
                    topmost_failed: false,
                    topmost_attempt: None,
                    fullscreen: false,
                    paused_fullscreen_rect: None,
                    removed_style: 0,
                    removed_fullscreen_style: 0,
                    style_failed: false,
                    refusals: 0,
                    last_attempt: None,
                    desired: None,
                },
            );
            let monitor = c.monitors.first().ok_or("no monitors available")?.clone();
            c.monitors = vec![Monitor {
                work: area,
                ..monitor.clone()
            }];
            c.records.get_mut(&(a as usize)).unwrap().monitor = monitor.id.clone();
            c.layouts.insert((0, monitor.id.clone()), l);
            c.records.insert(
                b as usize,
                Record {
                    identity: identity(b),
                    desktop: 0,
                    monitor: monitor.id.clone(),
                    floating: false,
                    minimized: false,
                    visibility_pending: None,
                    topmost_owned: false,
                    topmost_failed: false,
                    topmost_attempt: None,
                    fullscreen: false,
                    paused_fullscreen_rect: None,
                    removed_style: 0,
                    removed_fullscreen_style: 0,
                    style_failed: false,
                    refusals: 0,
                    last_attempt: None,
                    desired: None,
                },
            );
            let before = GetWindowLongPtrW(a, GWL_STYLE) as u32;
            c.style(a as usize);
            if GetWindowLongPtrW(a, GWL_STYLE) as u32 & STYLE_MASK != 0 {
                return Err("caption removal failed".into());
            }
            c.restore_style(a as usize);
            if GetWindowLongPtrW(a, GWL_STYLE) as u32 & STYLE_MASK != before & STYLE_MASK {
                return Err("caption restoration failed".into());
            }
            test_tray_interaction(&mut c, &resources)?;
            c.set_minimized(b as usize, true);
            pump_test_messages();
            c.sync_minimized();
            settle_test_layout(&mut c);
            if IsIconic(b) == 0 || !c.records[&(b as usize)].minimized || c.placement().len() != 1 {
                return Err(
                    "minimizing did not keep the window minimized and reclaim its tile".into(),
                );
            }
            if IsZoomed(a) == 0 {
                return Err("single remaining tiled window was not natively maximized".into());
            }
            if !visible_rect(a).is_some_and(|r| r.near(area, 2)) {
                return Err(format!("minimized window left an empty tile: actual={:?}, expected={area:?}, record={:?}, interaction={}", visible_rect(a), c.records[&(a as usize)], user_mouse_interaction()).into());
            }
            c.set_minimized(a as usize, true);
            pump_test_messages();
            c.sync_minimized();
            c.records.get_mut(&(a as usize)).unwrap().monitor = "other-test-monitor".into();
            c.restore_monitor(&monitor.id);
            pump_test_messages();
            c.sync_minimized();
            c.apply();
            pump_test_messages();
            if IsIconic(b) != 0 || IsIconic(a) == 0 {
                return Err("monitor restore restored the wrong minimized windows".into());
            }
            c.records.get_mut(&(a as usize)).unwrap().monitor = monitor.id.clone();
            c.restore_monitor(&monitor.id);
            pump_test_messages();
            c.sync_minimized();
            c.apply();
            pump_test_messages();
            settle_test_layout(&mut c);
            if IsIconic(a) != 0 || c.placement().len() != 2 || IsZoomed(a) != 0 || IsZoomed(b) != 0
            {
                return Err(
                    "restored windows did not return from maximization to split tiling".into(),
                );
            }
            // A window already minimized when first adopted must be restorable too.
            c.set_minimized(b as usize, true);
            pump_test_messages();
            c.sync_minimized();
            c.remove(b as usize);
            c.adopt(b as usize, 0, area.center());
            if !c.records[&(b as usize)].minimized || c.placement().len() != 1 {
                return Err("adoption did not preserve an already minimized window".into());
            }
            c.restore_monitor(&monitor.id);
            pump_test_messages();
            c.sync_minimized();
            c.apply();
            pump_test_messages();
            if IsIconic(b) != 0 || c.placement().len() != 2 {
                return Err("monitor restore missed an initially minimized window".into());
            }
            c.float(b as usize, true);
            c.apply();
            pump_test_messages();
            if GetWindowLongPtrW(b, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST == 0 {
                return Err("floating window was not made topmost".into());
            }
            SetWindowPos(
                a,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            pump_test_messages();
            let mut above = GetTopWindow(null_mut());
            let mut floating_above = false;
            for _ in 0..4096 {
                if above == b {
                    floating_above = true;
                    break;
                }
                if above == a || above.is_null() {
                    break;
                }
                above = GetWindow(above, GW_HWNDNEXT);
            }
            if !floating_above {
                return Err("raising a tiled window covered the floating window".into());
            }
            if !visible_rect(a).is_some_and(|r| r.near(area, 2)) {
                return Err("floating toggle did not reclaim sibling space".into());
            }
            c.float(b as usize, false);
            c.apply();
            pump_test_messages();
            if GetWindowLongPtrW(b, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 {
                return Err("retiling did not restore normal stacking".into());
            }
            // An app's pre-existing topmost state belongs to the app, not Tauiler.
            set_topmost(b, true);
            pump_test_messages();
            c.float(b as usize, true);
            c.float(b as usize, false);
            pump_test_messages();
            if GetWindowLongPtrW(b, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST == 0 {
                return Err("retiling removed app-owned topmost state".into());
            }
            set_topmost(b, false);
            pump_test_messages();
            // Pause restores Windows controls and releases all automatic management.
            c.fullscreen(a as usize);
            c.float(b as usize, true);
            c.action(Action::Pause);
            if !c.paused {
                return Err("pause action did not suspend tiling".into());
            }
            pump_test_messages();
            for h in [a, b] {
                let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
                if style & (STYLE_MASK | WS_CAPTION | WS_THICKFRAME)
                    != (STYLE_MASK | WS_CAPTION | WS_THICKFRAME)
                {
                    return Err("pause did not restore window decorations".into());
                }
            }
            c.ensure_topmost(b as usize);
            if GetWindowLongPtrW(b, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 {
                return Err("pause retained automatic floating topmost".into());
            }
            c.float(b as usize, false);
            let current = visible_rect(a).ok_or("pause test bounds unavailable")?;
            let moved = Rect {
                x: current.x + 37,
                y: current.y + 19,
                ..current
            };
            place(a, moved);
            pump_test_messages();
            for _ in 0..4 {
                c.apply();
            }
            pump_test_messages();
            if !visible_rect(a).is_some_and(|r| r.near(moved, 2)) {
                return Err("automatic correction ran while paused".into());
            }
            c.action(Action::Launcher);
            if !c.paused || !c.pending_launcher {
                return Err("launcher binding stopped working while paused".into());
            }
            c.pending_launcher = false;
            let sibling = visible_rect(b).ok_or("pause sibling bounds unavailable")?;
            c.resize(a as usize, -40, -40);
            c.apply_layout();
            c.style(a as usize);
            pump_test_messages();
            let manual = Rect {
                width: moved.width - 40,
                height: moved.height - 40,
                ..moved
            };
            if !visible_rect(a).is_some_and(|r| r.near(manual, 2)) {
                return Err("explicit resize did not work while paused".into());
            }
            if !visible_rect(b).is_some_and(|r| r.near(sibling, 2)) {
                return Err("paused resize rearranged another window".into());
            }
            if GetWindowLongPtrW(a, GWL_STYLE) as u32 & STYLE_MASK != STYLE_MASK {
                return Err("paused command removed window decorations".into());
            }
            c.fullscreen(a as usize);
            pump_test_messages();
            if !visible_rect(a).is_some_and(|r| r.near(monitor.full, 2)) {
                return Err("paused fullscreen shortcut failed".into());
            }
            c.fullscreen(a as usize);
            pump_test_messages();
            if !visible_rect(a).is_some_and(|r| r.near(manual, 2)) {
                return Err("paused fullscreen did not restore free window bounds".into());
            }
            c.event(Event::NativeDragStart(a as usize));
            place(a, moved);
            pump_test_messages();
            c.event(Event::NativeDragEnd(a as usize));
            pump_test_messages();
            if !visible_rect(a).is_some_and(|r| r.near(moved, 2)) {
                return Err("paused native drag triggered tiling".into());
            }
            c.set_minimized(b as usize, true);
            pump_test_messages();
            c.apply();
            pump_test_messages();
            if !visible_rect(a).is_some_and(|r| r.near(moved, 2)) {
                return Err("paused minimize filled the vacant space".into());
            }
            c.set_minimized(b as usize, false);
            pump_test_messages();
            c.apply();
            if !visible_rect(a).is_some_and(|r| r.near(moved, 2)) {
                return Err("paused restore rearranged another window".into());
            }
            c.action(Action::Pause);
            pump_test_messages();
            if c.paused {
                return Err("pause action did not resume tiling".into());
            }
            if c.placement().len() != 2 {
                return Err("floating reinsertion failed".into());
            }
            c.fullscreen(a as usize);
            settle_test_layout(&mut c);
            if !visible_rect(a).is_some_and(|r| r.near(monitor.full, 2)) {
                return Err("fullscreen placement failed".into());
            }
            c.end_fullscreen(a as usize);
            settle_test_layout(&mut c);
            let target = c
                .placement()
                .into_iter()
                .find(|(id, _)| *id == a as usize)
                .ok_or("fullscreen lost its tile")?
                .1;
            if !visible_rect(a).is_some_and(|r| r.near(target, 2)) {
                return Err(format!("fullscreen tile restoration failed: actual={:?}, expected={target:?}, record={:?}", visible_rect(a), c.records[&(a as usize)]).into());
            }
            // Corrections must leave a user's native drag untouched.
            c.event(Event::NativeDragStart(a as usize));
            let dragged = Rect {
                x: target.x + 57,
                y: target.y + 31,
                ..target
            };
            place(a, dragged);
            pump_test_messages();
            for _ in 0..4 {
                c.apply();
            }
            if !visible_rect(a).is_some_and(|r| r.near(dragged, 2))
                || c.records[&(a as usize)].floating
            {
                return Err("geometry correction fought an active native drag".into());
            }
            c.event(Event::NativeDragEnd(a as usize));
            pump_test_messages();
            c.apply();
            pump_test_messages();
            if c.placement().len() != 2 {
                return Err("native drag release lost tiled membership".into());
            }
            // Rapid SUPER movement must track total pointer displacement.
            let start_rect = visible_rect(a).ok_or("drag test bounds unavailable")?;
            let origin = start_rect.center();
            c.drag = Some(Drag {
                id: a as usize,
                right: false,
                origin,
                start_rect,
                start_layout: None,
            });
            let end = Point {
                x: origin.x + 113,
                y: origin.y + 67,
            };
            for step in 1..=8 {
                c.event(Event::DragMove(Point {
                    x: origin.x + step * 14,
                    y: origin.y + step * 8,
                }));
            }
            c.event(Event::DragMove(end));
            pump_test_messages();
            let expected = Rect {
                x: start_rect.x + 113,
                y: start_rect.y + 67,
                ..start_rect
            };
            if !visible_rect(a).is_some_and(|r| r.near(expected, 2)) {
                return Err("SUPER drag lost pointer displacement".into());
            }
            c.event(Event::DragEnd(end));
            pump_test_messages();
            c.apply();
            pump_test_messages();
            // A burst of raw resize moves must retain only the latest point until
            // the next frame; release must flush that point without waiting.
            let key = (0, monitor.id.clone());
            let start_layout = c
                .layouts
                .get(&key)
                .ok_or("resize test layout missing")?
                .clone();
            let start_rect = visible_rect(a).ok_or("resize test bounds missing")?;
            let origin = start_rect.center();
            let end = Point {
                x: origin.x + 80,
                y: origin.y + 40,
            };
            let mut expected_layout = start_layout.clone();
            expected_layout.resize(a as usize, 80, 40, area);
            c.drag = Some(Drag {
                id: a as usize,
                right: true,
                origin,
                start_rect,
                start_layout: Some(start_layout),
            });
            c.resize_frame_at = Some(Instant::now() + Duration::from_secs(1));
            let before_resize = c.records[&(a as usize)].desired;
            for step in 1..=80 {
                c.event(Event::DragMove(Point {
                    x: origin.x + step,
                    y: origin.y + step / 2,
                }));
            }
            if c.pending_resize != Some(end) || c.records[&(a as usize)].desired != before_resize {
                return Err("resize input was not coalesced between frames".into());
            }
            c.event(Event::DragEnd(end));
            pump_test_messages();
            for (id, target) in expected_layout.tiles(area) {
                if !visible_rect(id as HWND).is_some_and(|r| r.near(target, 2)) {
                    return Err(
                        "resize release did not immediately apply the final pointer position"
                            .into(),
                    );
                }
            }
            c.float(b as usize, true);
            pump_test_messages();
            // Dropping the controller restores owned caption and topmost changes on exit.
            drop(c);
            pump_test_messages();
            if GetWindowLongPtrW(a, GWL_STYLE) as u32 & STYLE_MASK != before & STYLE_MASK {
                return Err("exit did not restore caption controls".into());
            }
            if GetWindowLongPtrW(b, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 {
                return Err("exit did not release Tauiler-owned topmost state".into());
            }
            // Dropping both owners must clean up before the disposable windows are destroyed.
            drop(resources);
            println!(
                "Native self-test passed: sent/posted tray clicks, visible context menu, pause/resume/quit routing, rounded icons, minimize/monitor restore, tiling, floating, fullscreen, dragging, resizing, and exit cleanup. No existing app windows modified."
            );
            Ok(())
        })();
        DestroyWindow(a);
        DestroyWindow(b);
        UnregisterClassW(name.as_ptr(), module);
        result
    }
}

fn settle_test_layout(controller: &mut Controller) {
    for _ in 0..10 {
        controller.apply();
        pump_test_messages();
        if controller.placement().iter().all(|(id, target)| {
            visible_rect(*id as HWND).is_some_and(|actual| actual.near(*target, 2))
        }) {
            break;
        }
    }
}

fn pump_test_messages() {
    unsafe {
        let mut msg: MSG = zeroed();
        for _ in 0..10 {
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn take_tray_event() -> Option<Event> {
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        let index = events.iter().position(|event| {
            matches!(
                event,
                Event::TrayMenu | Event::Action(Action::Pause) | Event::Action(Action::Exit)
            )
        })?;
        events.remove(index)
    })
}
fn test_tray_interaction(
    controller: &mut Controller,
    resources: &Resources,
) -> Result<(), Box<dyn Error>> {
    unsafe {
        while take_tray_event().is_some() {}
        let select = if TRAY_MODERN.with(Cell::get) {
            NIN_SELECT
        } else {
            WM_LBUTTONUP
        };
        SendMessageW(resources.window, TRAY, 0, ((1 << 16) | select) as isize);
        let event =
            take_tray_event().ok_or("sent tray click never reached the window procedure")?;
        dispatch_event(controller, resources, event);
        if !controller.paused {
            return Err("sent tray click did not pause tiling".into());
        }
        PostMessageW(resources.window, TRAY, 0, ((1 << 16) | select) as isize);
        pump_test_messages();
        let event = take_tray_event().ok_or("posted tray click was not dispatched")?;
        dispatch_event(controller, resources, event);
        if controller.paused {
            return Err("posted tray click did not resume tiling".into());
        }
        SendMessageW(
            resources.window,
            TRAY,
            0,
            ((1 << 16) | WM_CONTEXTMENU) as isize,
        );
        let event = take_tray_event().ok_or("sent tray context notification was discarded")?;
        if !matches!(event, Event::TrayMenu) {
            return Err("tray context notification did not request a menu".into());
        }
        let owner = resources.window as usize;
        let thread = GetWindowThreadProcessId(resources.window, null_mut());
        let observed = std::sync::Arc::new(AtomicBool::new(false));
        let result = observed.clone();
        let cancel = std::thread::spawn(move || {
            for _ in 0..100 {
                let mut gui: GUITHREADINFO = zeroed();
                gui.cbSize = size_of::<GUITHREADINFO>() as u32;
                if GetGUIThreadInfo(thread, &mut gui) != 0
                    && gui.flags & (GUI_INMENUMODE | GUI_POPUPMENUMODE) != 0
                {
                    result.store(true, Ordering::Relaxed);
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            PostMessageW(owner as HWND, WM_CANCELMODE, 0, 0);
        });
        dispatch_event(controller, resources, event);
        cancel.join().map_err(|_| "tray menu check thread failed")?;
        if !observed.load(Ordering::Relaxed) {
            return Err("right-click callback did not open a native context menu".into());
        }
        SendMessageW(resources.window, WM_CLOSE, 0, 0);
        let event = take_tray_event().ok_or("tray controller close did not enqueue quit")?;
        if dispatch_event(controller, resources, event) {
            return Err("quit action did not request complete shutdown".into());
        }
        Ok(())
    }
}

fn test_creation_override(controller: HWND) -> Result<(), Box<dyn Error>> {
    use std::os::windows::process::CommandExt;
    let directory = std::env::current_exe()?
        .parent()
        .ok_or("no test executable directory")?
        .to_path_buf();
    if !directory.join("tauiler_creation_hook.dll").exists() {
        return Err(
            "creation hook DLL missing; build the complete workspace before native checks".into(),
        );
    }
    let mut c = Controller::new(
        Config {
            native_workspaces: false,
            ..Config::default()
        },
        PathBuf::from("unused"),
    )?;
    let area = c
        .monitors
        .iter()
        .find(|m| m.full.contains(cursor()))
        .ok_or("no cursor monitor")?
        .work;
    c.publish_creation_plan();
    CREATION_PLAN.with(|s| {
        let mut s = s.borrow_mut();
        let plan = s.as_mut().unwrap();
        plan.test_process = Some(0);
        plan.test_point = Some(area.center());
    });
    let hook = crate::creation::CreationHook::from_directory(controller, &directory)?;
    // Wait for the x86 host to install its global hook, pumping controller RPCs.
    pump_test_messages();
    pump_test_messages();
    if std::env::var_os("TAUILER_EXPLORER_PROBE").is_some() {
        let shell_pid = unsafe { identity(GetShellWindow()).0 };
        CREATION_PLAN.with(|s| {
            let mut s = s.borrow_mut();
            let plan = s.as_mut().unwrap();
            plan.test_process = None;
            plan.test_executable = Some("explorer.exe".into());
        });
        let before: HashSet<_> = all_windows().into_iter().collect();
        let folder = directory.join("Tauiler Explorer hook probe");
        std::fs::create_dir_all(&folder)?;
        let wine = std::env::var("TAUILER_EXPLORER_PROBE").is_ok_and(|v| v == "wine");
        if wine {
            unsafe {
                keybd_event(VK_LWIN as u8, 0, 0, 0);
                keybd_event(b'E', 0, 0, 0);
                keybd_event(b'E', 0, KEYEVENTF_KEYUP, 0);
                keybd_event(VK_LWIN as u8, 0, KEYEVENTF_KEYUP, 0);
            }
        } else {
            let _ = std::process::Command::new("explorer.exe")
                .arg(&folder)
                .spawn()?;
        }
        let start = Instant::now();
        let mut found = None;
        let mut samples = vec![];
        while start.elapsed() < Duration::from_secs(3) {
            unsafe {
                let mut msg: MSG = zeroed();
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if found.is_none() {
                found = all_windows().into_iter().find(|id| {
                    !before.contains(id)
                        && class(*id as HWND) == "CabinetWClass"
                        && (wine || title(*id as HWND).contains("Tauiler Explorer hook probe"))
                });
            }
            if let Some(id) = found {
                let h = id as HWND;
                if unsafe { IsWindowVisible(h) != 0 }
                    && let Some(rect) = window_rect(h)
                    && samples.last() != Some(&rect)
                {
                    samples.push(rect);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(id) = found {
            let h = id as HWND;
            unsafe {
                println!(
                    "Explorer live probe: pid={}, shell_pid={shell_pid}, style={:#x}, awareness={}, bounds={samples:?}, owner={:?}",
                    identity(h).0,
                    GetWindowLongPtrW(h, GWL_STYLE),
                    GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(h)),
                    GetPropW(h, crate::creation::protocol::OWNER)
                );
                PostMessageW(h, WM_CLOSE, 0, 0);
            }
            pump_test_messages();
            if samples.is_empty()
                || samples.iter().any(|r| {
                    (r.x - area.x).abs() > 24
                        || (r.y - area.y).abs() > 2
                        || r.width < area.width
                        || r.height < area.height
                })
            {
                return Err(format!(
                    "Explorer first-show placement escaped its tile: {samples:?}, area={area:?}"
                )
                .into());
            }
        } else {
            return Err("Explorer live probe did not find its test window".into());
        }
        CREATION_PLAN.with(|s| s.borrow_mut().as_mut().unwrap().test_process = Some(0));
    }
    let result = (|| -> Result<(), Box<dyn Error>> {
        for bits in [64, 32] {
            let probe = directory.join(if bits == 64 {
                "tauiler-hook-probe.exe"
            } else {
                "tauiler-hook-probe32.exe"
            });
            if !probe.exists() {
                return Err(format!("missing {bits}-bit native hook probe").into());
            }
            for scenario in [
                "late-moves",
                "late-style",
                "paused-show",
                "floating-show",
                "ignored-show",
                "tile",
                "split",
                "paused",
                "tool",
                "legacy",
                "ignored",
                "floating",
                "restored",
            ] {
                c.paused = scenario.starts_with("paused");
                c.layouts.clear();
                if scenario == "split" {
                    let monitor = c.monitors.iter().find(|m| m.work == area).unwrap();
                    let mut layout = Layout::default();
                    layout.insert(usize::MAX, monitor.work, monitor.work.center(), None);
                    c.layouts.insert((c.current, monitor.id.clone()), layout);
                }
                c.config.rules.clear();
                if matches!(
                    scenario,
                    "ignored" | "floating" | "ignored-show" | "floating-show"
                ) {
                    c.config.rules.push(config::Rule {
                        executable: Some(
                            if bits == 64 {
                                "tauiler-hook-probe.exe"
                            } else {
                                "tauiler-hook-probe32.exe"
                            }
                            .into(),
                        ),
                        class: None,
                        title_contains: None,
                        mode: if scenario.starts_with("ignored") {
                            RuleMode::Ignore
                        } else {
                            RuleMode::Floating
                        },
                    });
                }
                c.publish_creation_plan();
                CREATION_PLAN.with(|s| {
                    let mut s = s.borrow_mut();
                    let plan = s.as_mut().unwrap();
                    plan.test_process = Some(0);
                    plan.test_point = Some(area.center());
                });
                let path = directory.join(format!("creation-probe-{bits}-{scenario}.txt"));
                let mut child = std::process::Command::new(&probe)
                    .arg(&path)
                    .arg(if scenario.ends_with("-show") {
                        "late-moves"
                    } else if matches!(scenario, "tool" | "legacy" | "late-moves" | "late-style") {
                        scenario
                    } else {
                        "normal"
                    })
                    .creation_flags(0x08000000)
                    .spawn()?;
                CREATION_PLAN
                    .with(|s| s.borrow_mut().as_mut().unwrap().test_process = Some(child.id()));
                let start = Instant::now();
                let status = loop {
                    unsafe {
                        let mut msg: MSG = zeroed();
                        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                    if let Some(status) = child.try_wait()? {
                        break status;
                    }
                    if start.elapsed() > Duration::from_secs(5) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("{bits}-bit creation probe timed out").into());
                    }
                    std::thread::sleep(Duration::from_millis(1));
                };
                if !status.success() {
                    return Err(format!("{bits}-bit creation probe failed: {status}").into());
                }
                let value = std::fs::read_to_string(path)?;
                let actual: Vec<i32> = value
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                if matches!(scenario, "tile" | "restored" | "late-moves" | "late-style") {
                    // The initial outer rectangle includes the invisible resize border.
                    if actual.len() != 4
                        || (actual[0] - area.x).abs() > 24
                        || (actual[1] - area.y).abs() > 2
                        || actual[2] < area.width
                        || actual[3] < area.height
                    {
                        return Err(format!(
                            "{bits}-bit pre-show tile failed: WM_NCCREATE={actual:?}, area={area:?}"
                        )
                        .into());
                    }
                } else if scenario == "split" {
                    let (width, height) = if area.width >= area.height {
                        ((area.width + 1) / 2, area.height)
                    } else {
                        (area.width, (area.height + 1) / 2)
                    };
                    if actual.len() != 4
                        || (actual[2] - width).abs() > 48
                        || (actual[3] - height).abs() > 24
                    {
                        return Err(format!("{bits}-bit pre-show split failed: WM_NCCREATE={actual:?}, area={area:?}").into());
                    }
                } else if scenario.ends_with("-show") {
                    if actual != [501, 171, 800, 600] {
                        return Err(format!("{bits}-bit {scenario} first show was incorrectly overridden: {actual:?}").into());
                    }
                } else if actual != [73, 91, 420, 310] {
                    return Err(format!(
                        "{bits}-bit {scenario} creation was incorrectly overridden: {actual:?}"
                    )
                    .into());
                }
            }
        }
        Ok(())
    })();
    CREATION_PLAN.with(|s| *s.borrow_mut() = None);
    drop(hook);
    result?;
    println!(
        "Creation hook passed: x64/x86 WM_NCCREATE receives tile coordinates before show; pause, tool, ignore and floating bypasses passed"
    );
    Ok(())
}

fn test_workspace_cursor_focus(a: HWND, b: HWND) -> Result<(), Box<dyn Error>> {
    if unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 || GetAsyncKeyState(VK_RBUTTON as i32) < 0 }
    {
        println!("Stationary workspace focus check skipped: user mouse interaction is active");
        return Ok(());
    }

    let mut c = Controller::new(
        Config {
            native_workspaces: false,
            ..Config::default()
        },
        PathBuf::from("unused"),
    )?;
    let point = cursor();
    let Some(monitor) = c.monitors.iter().find(|m| m.work.contains(point)).cloned() else {
        println!("Stationary workspace focus check skipped: pointer outside monitor work areas");
        return Ok(());
    };
    unsafe {
        ShowWindow(a, SW_RESTORE);
        ShowWindow(b, SW_RESTORE);
    }
    place(
        a,
        Rect {
            x: point.x - 80,
            y: point.y - 50,
            width: 300,
            height: 200,
        },
    );
    let away = c
        .monitors
        .iter()
        .find(|m| !m.full.contains(point))
        .map(|m| m.work.center())
        .unwrap_or(Point {
            x: if point.x < monitor.work.center().x {
                monitor.work.x + monitor.work.width - 160
            } else {
                monitor.work.x + 160
            },
            y: if point.y < monitor.work.center().y {
                monitor.work.y + monitor.work.height - 100
            } else {
                monitor.work.y + 100
            },
        });
    place(
        b,
        Rect {
            x: away.x - 100,
            y: away.y - 70,
            width: 200,
            height: 140,
        },
    );
    pump_test_messages();
    for window in [a, b] {
        c.adopt(window as usize, 0, point);
        c.float(window as usize, true);
    }
    pump_test_messages();
    if unsafe { SetForegroundWindow(b) == 0 } {
        println!("Stationary workspace focus check skipped: Windows rejected foreground setup");
        return Ok(());
    }
    for (paused, explicit, expected) in [(false, true, a), (true, true, a), (true, false, b)] {
        if cursor() != point {
            println!(
                "Stationary workspace focus check skipped: user moved the pointer during setup"
            );
            return Ok(());
        }
        unsafe {
            SetForegroundWindow(b);
        }
        c.paused = paused;
        c.workspace_focus = Some(WorkspaceFocus::new(c.current, explicit));
        c.last_cursor = cursor();
        // Observe only the two disposable windows, never reconcile real app windows.
        c.dirty = false;
        c.last_reconcile = Instant::now();
        c.tick();
        pump_test_messages();
        if unsafe { GetForegroundWindow() } != expected {
            return Err(format!(
                "stationary workspace focus failed: paused={paused}, explicit={explicit}"
            )
            .into());
        }
        if expected == a && c.workspace_focus.is_some() {
            return Err(
                "workspace focus request did not finish after selecting the hovered window".into(),
            );
        }
    }
    println!(
        "Stationary workspace focus passed: switch request focuses without cursor movement, including explicit paused bindings"
    );
    Ok(())
}

fn test_monitor_adoption(a: HWND, b: HWND) -> Result<(), Box<dyn Error>> {
    let mut c = Controller::new(
        Config {
            native_workspaces: false,
            ..Config::default()
        },
        PathBuf::from("unused"),
    )?;
    if c.monitors.len() < 2 {
        println!("Two-monitor native adoption check skipped: only one monitor attached");
        return Ok(());
    }
    let first = c.monitors[0].clone();
    let second = c.monitors[1].clone();
    for (window, monitor) in [(a, &first), (b, &second)] {
        let p = monitor.work.center();
        place(
            window,
            Rect {
                x: p.x - 180,
                y: p.y - 120,
                width: 360,
                height: 240,
            },
        );
    }
    pump_test_messages();
    for window in [a, b] {
        let context = adoption_context(window, &c.monitors)
            .ok_or("adoption test window monitor unavailable")?;
        c.startup_windows.insert(window as usize, context);
    }
    // Only one window is initially discovered. The second is adopted in a later pass,
    // with the insertion pointer deliberately remaining on the first monitor.
    let pointer = first.work.center();
    c.adopt(a as usize, 0, pointer);
    settle_test_layout(&mut c);
    c.adopt(b as usize, 0, pointer);
    settle_test_layout(&mut c);
    if c.records[&(a as usize)].monitor != first.id || c.records[&(b as usize)].monitor != second.id
    {
        return Err("delayed startup adoption moved a window to the pointer's monitor".into());
    }
    for (window, monitor) in [(a, &first), (b, &second)] {
        if unsafe { IsZoomed(window) == 0 }
            || !visible_rect(window).is_some_and(|r| r.near(monitor.work, 2))
        {
            return Err(format!("adopted windows did not fill their separate monitor work areas: actual={:?}, expected={:?}, record={:?}, interaction={}", visible_rect(window), monitor.work, c.records[&(window as usize)], user_mouse_interaction()).into());
        }
    }
    println!(
        "Two-monitor adoption passed: existing windows retained separate monitors across delayed discovery"
    );
    Ok(())
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    fn creation_plan() -> CreationPlan {
        CreationPlan {
            config: Config::default(),
            layouts: HashMap::new(),
            monitors: vec![monitor("left", 1, -1000)],
            current: 0,
            enabled: true,
            blocked_monitors: HashSet::new(),
            test_process: None,
            test_executable: None,
            test_point: None,
            known_windows: HashSet::new(),
        }
    }
    fn creation_probe(x: i32, y: i32) -> crate::creation::protocol::Request {
        let mut request = crate::creation::protocol::Request {
            window: 99,
            process: 1,
            thread: 1,
            phase: 0,
            reserved: 0,
            style: WS_OVERLAPPEDWINDOW,
            ex_style: 0,
            point_x: x,
            point_y: y,
            class: [0; 256],
            title: [0; 512],
        };
        for (slot, value) in request
            .class
            .iter_mut()
            .zip("Normal.Test.Window".encode_utf16())
        {
            *slot = value;
        }
        request
    }
    #[test]
    fn creation_predicts_the_correct_split_from_taskbar_without_mutating_layout() {
        let mut plan = creation_plan();
        let area = plan.monitors[0].work;
        let mut layout = Layout::default();
        layout.insert(11, area, area.center(), None);
        layout.insert(12, area, Point { x: -100, y: 100 }, None);
        let before = layout.tiles(area);
        plan.layouts.insert((0, "left".into()), layout);
        let target = plan.target(&creation_probe(-100, 790), "test.exe").unwrap();
        assert_eq!(
            target,
            Rect {
                x: -500,
                y: 375,
                width: 500,
                height: 375
            }
        );
        assert_eq!(plan.layouts[&(0, "left".into())].tiles(area), before);
    }
    #[test]
    fn creation_bypasses_pause_fullscreen_and_ambiguous_title_rules() {
        let mut plan = creation_plan();
        let request = creation_probe(-100, 150);
        assert!(plan.target(&request, "test.exe").is_some());
        plan.enabled = false;
        assert!(plan.target(&request, "test.exe").is_none());
        plan.enabled = true;
        plan.blocked_monitors.insert("left".into());
        assert!(plan.target(&request, "test.exe").is_none());
        plan.blocked_monitors.clear();
        plan.config.rules = vec![config::Rule {
            executable: Some("TEST.exe".into()),
            class: None,
            title_contains: Some("Splash".into()),
            mode: RuleMode::Floating,
        }];
        assert!(plan.target(&request, "test.exe").is_none());
        assert!(plan.target(&request, "other.exe").is_some());
    }
    fn monitor(id: &str, handle: usize, x: i32) -> Monitor {
        Monitor {
            id: id.into(),
            handle,
            work: Rect {
                x,
                y: 0,
                width: 1000,
                height: 750,
            },
            full: Rect {
                x,
                y: 0,
                width: 1000,
                height: 800,
            },
        }
    }
    #[test]
    fn delayed_existing_windows_keep_separate_monitors_with_pointer_on_first() {
        let monitors = [monitor("left", 1, -1000), monitor("right", 2, 0)];
        let pointer = Point { x: -500, y: 200 };
        for monitor in &monitors {
            let existing = AdoptionContext {
                identity: (1, 1),
                monitor: monitor.id.clone(),
                point: monitor.work.center(),
            };
            assert_eq!(
                adoption_monitor(&monitors, monitor.handle, pointer, Some(&existing))
                    .unwrap()
                    .id,
                monitor.id
            );
        }
        assert_eq!(
            adoption_monitor(&monitors, 2, pointer, None).unwrap().id,
            "left"
        );
    }
    #[test]
    fn workspace_focus_waits_for_the_destination_and_expires() {
        let mut focus = WorkspaceFocus::new(42, true);
        assert!(!focus.ready(41, false, true));
        assert!(focus.ready(42, false, true));
        assert!(focus.ready(42, true, false));
        focus.explicit = false;
        assert!(!focus.ready(42, true, true));
        assert!(!focus.ready(42, false, false));
        focus.expires = Instant::now() - Duration::from_millis(1);
        assert!(!focus.ready(42, false, true));
    }
    #[test]
    fn first_show_only_positions_new_unmanaged_windows() {
        let mut plan = creation_plan();
        let mut request = creation_probe(-100, 150);
        request.phase = 1;
        assert!(plan.target(&request, "explorer.exe").is_some());
        plan.known_windows.insert(request.window as usize);
        assert!(plan.target(&request, "explorer.exe").is_none());
        plan.known_windows.clear();
        plan.enabled = false;
        assert!(plan.target(&request, "explorer.exe").is_none());
        plan.enabled = true;
        plan.config.rules = vec![config::Rule {
            executable: Some("explorer.exe".into()),
            class: None,
            title_contains: None,
            mode: RuleMode::Floating,
        }];
        assert!(plan.target(&request, "explorer.exe").is_none());
    }
    #[test]
    fn disconnected_startup_monitor_falls_back_to_window_source() {
        let monitors = [monitor("remaining", 2, 0)];
        let existing = AdoptionContext {
            identity: (1, 1),
            monitor: "disconnected".into(),
            point: Point { x: -500, y: 200 },
        };
        assert_eq!(
            adoption_monitor(&monitors, 2, existing.point, Some(&existing))
                .unwrap()
                .id,
            "remaining"
        );
    }
    #[test]
    fn move_targets_use_total_displacement_even_when_window_updates_lag() {
        let d = Drag {
            id: 1,
            right: false,
            origin: Point { x: 50, y: 70 },
            start_rect: Rect {
                x: -100,
                y: 20,
                width: 500,
                height: 300,
            },
            start_layout: None,
        };
        assert_eq!(
            d.target(Point { x: 70, y: 100 }),
            Rect {
                x: -80,
                y: 50,
                width: 500,
                height: 300
            }
        );
        assert_eq!(
            d.target(Point { x: 90, y: 110 }),
            Rect {
                x: -60,
                y: 60,
                width: 500,
                height: 300
            }
        );
        assert_eq!(d.target(d.origin), d.start_rect);
    }
    #[test]
    fn floating_resize_can_reverse_after_hitting_minimum_without_drift() {
        let d = Drag {
            id: 1,
            right: true,
            origin: Point { x: 50, y: 70 },
            start_rect: Rect {
                x: 100,
                y: 20,
                width: 500,
                height: 300,
            },
            start_layout: None,
        };
        assert_eq!(d.target(Point { x: -1000, y: -1000 }).width, 64);
        assert_eq!(d.target(d.origin), d.start_rect);
        assert_eq!(
            d.target(Point { x: 100, y: 90 }),
            Rect {
                x: 100,
                y: 20,
                width: 550,
                height: 320
            }
        );
    }
}
