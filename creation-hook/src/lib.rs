#![cfg(windows)]
#[allow(dead_code)]
mod protocol;
use protocol::*;
use std::{mem::size_of, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::*,
    System::{DataExchange::COPYDATASTRUCT, Threading::*},
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

// No loader-lock work, background threads, heap-owned global state, or blocking
// controller RPC without a deadline. A failed request leaves creation unchanged.
/// # Safety
/// Called by Windows as a WH_CBT hook. For HCBT_CREATEWND, `l` must point to
/// the live CBT_CREATEWND structure supplied by the operating system.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn tauiler_creation_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code == HCBT_CREATEWND as i32 && l != 0 {
            override_creation(w as HWND, &mut *(l as *mut CBT_CREATEWNDW));
        }
        CallNextHookEx(null_mut(), code, w, l)
    }
}

unsafe fn override_creation(hwnd: HWND, data: &mut CBT_CREATEWNDW) {
    unsafe {
        if data.lpcs.is_null() {
            return;
        }
        let cs = &mut *data.lpcs;
        if !candidate(cs.style as u32, cs.dwExStyle, !cs.hwndParent.is_null()) {
            return;
        }
        if let Some(target) = request_placement(hwnd, cs.style as u32, cs.dwExStyle, 0) {
            cs.x = target.left;
            cs.y = target.top;
            cs.cx = target.right - target.left;
            cs.cy = target.bottom - target.top;
        }
    }
}

thread_local! { static SHOWING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// # Safety
/// Windows supplies a live CWPSTRUCT for nonnegative WH_CALLWNDPROC codes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn tauiler_show_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 && l != 0 {
            let message = &*(l as *const CWPSTRUCT);
            if message.message == WM_SHOWWINDOW && message.wParam != 0 && message.lParam == 0 {
                SHOWING.with(|active| {
                    if active.replace(true) {
                        return;
                    }
                    let h = message.hwnd;
                    let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
                    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
                    if candidate(style, ex, !GetWindow(h, GW_OWNER).is_null())
                        && let Some(target) = request_placement(h, style, ex, 1)
                    {
                        // Synchronous placement on the creating/showing app's own thread.
                        // The original WM_SHOWWINDOW message is passed on unchanged.
                        SetWindowPos(
                            h,
                            null_mut(),
                            target.left,
                            target.top,
                            target.right - target.left,
                            target.bottom - target.top,
                            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
                        );
                    }
                    active.set(false);
                });
            }
        }
        CallNextHookEx(null_mut(), code, w, l)
    }
}

unsafe fn request_placement(hwnd: HWND, style: u32, ex_style: u32, phase: u32) -> Option<RECT> {
    unsafe {
        // Physical tile coordinates are valid directly for per-monitor-aware apps.
        // Older DPI models keep their original path rather than spawning at scaled coordinates.
        if GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext())
            != DPI_AWARENESS_PER_MONITOR_AWARE
        {
            return None;
        }
        let controller = FindWindowW(CONTROLLER, std::ptr::null());
        let mut controller_pid = 0;
        GetWindowThreadProcessId(controller, &mut controller_pid);
        if controller.is_null() || controller_pid == GetCurrentProcessId() {
            return None;
        }
        let mut point = POINT::default();
        if GetPhysicalCursorPos(&mut point) == 0 {
            return None;
        }
        if phase == 1 && GetPropW(hwnd, OWNER) as usize as u32 == controller_pid {
            point.x = GetPropW(hwnd, POINT_X) as usize as i32;
            point.y = GetPropW(hwnd, POINT_Y) as usize as i32;
        }
        let mut request = Request {
            window: hwnd as usize as u64,
            process: GetCurrentProcessId(),
            thread: GetCurrentThreadId(),
            phase,
            reserved: 0,
            style,
            ex_style,
            point_x: point.x,
            point_y: point.y,
            class: [0; 256],
            title: [0; 512],
        };
        GetClassNameW(hwnd, request.class.as_mut_ptr(), request.class.len() as i32);
        // The class exists at this point; the title need not exist yet. Do not read
        // CREATESTRUCT's ANSI/Unicode string pointer inside another app.
        GetWindowTextW(hwnd, request.title.as_mut_ptr(), request.title.len() as i32);
        let copy = COPYDATASTRUCT {
            dwData: MAGIC,
            cbData: size_of::<Request>() as u32,
            lpData: &request as *const _ as *mut _,
        };
        let mut accepted = 0;
        if SendMessageTimeoutW(
            controller,
            WM_COPYDATA,
            hwnd as usize,
            &copy as *const _ as isize,
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            15,
            &mut accepted,
        ) == 0
            || accepted != 1
        {
            return None;
        }
        if GetPropW(hwnd, OWNER) as usize as u32 != controller_pid {
            return None;
        }
        let width = GetPropW(hwnd, WIDTH) as usize as i32;
        let height = GetPropW(hwnd, HEIGHT) as usize as i32;
        let target = (width > 0 && height > 0).then(|| RECT {
            left: GetPropW(hwnd, X) as usize as i32,
            top: GetPropW(hwnd, Y) as usize as i32,
            right: GetPropW(hwnd, X) as usize as i32 + width,
            bottom: GetPropW(hwnd, Y) as usize as i32 + height,
        });
        for key in [X, Y, WIDTH, HEIGHT] {
            RemovePropW(hwnd, key);
        }
        target
    }
}
