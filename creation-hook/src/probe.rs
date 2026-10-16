use std::{mem::zeroed, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::*,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(l as *const CREATESTRUCTW);
            let report = &mut *(cs.lpCreateParams as *mut [i32; 4]);
            *report = [cs.x, cs.y, cs.cx, cs.cy];
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let report_path = args.get(1).ok_or("report path required")?;
    let tool = args.get(2).is_some_and(|v| v == "tool");
    unsafe {
        SetProcessDpiAwarenessContext(if args.get(2).is_some_and(|v| v == "legacy") {
            DPI_AWARENESS_CONTEXT_UNAWARE
        } else {
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        });
        let module = GetModuleHandleW(std::ptr::null());
        let name = windows_sys::core::w!("Tauiler.Creation.Probe");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: module,
            lpszClassName: name,
            ..zeroed()
        };
        RegisterClassW(&wc);
        let mut report = [0i32; 4];
        let hwnd = CreateWindowExW(
            if tool { WS_EX_TOOLWINDOW } else { 0 },
            name,
            windows_sys::core::w!("Tauiler hook probe"),
            if args.get(2).is_some_and(|v| v == "late-style") {
                WS_OVERLAPPEDWINDOW & !WS_THICKFRAME
            } else {
                WS_OVERLAPPEDWINDOW
            },
            73,
            91,
            420,
            310,
            null_mut(),
            null_mut(),
            module,
            &mut report as *mut _ as _,
        );
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        if args
            .get(2)
            .is_some_and(|v| matches!(v.as_str(), "late-moves" | "late-style"))
        {
            SetWindowLongW(hwnd, GWL_STYLE, WS_OVERLAPPEDWINDOW as i32);
            SetWindowPos(
                hwnd,
                null_mut(),
                501,
                171,
                800,
                600,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let mut actual = RECT::default();
            GetWindowRect(hwnd, &mut actual);
            std::fs::write(
                report_path,
                format!(
                    "{} {} {} {}",
                    actual.left,
                    actual.top,
                    actual.right - actual.left,
                    actual.bottom - actual.top
                ),
            )?;
            DestroyWindow(hwnd);
            UnregisterClassW(name, module);
            return Ok(());
        }
        // WM_NCCREATE was observed before WM_CREATE and before any ShowWindow.
        std::fs::write(
            report_path,
            format!("{} {} {} {}", report[0], report[1], report[2], report[3]),
        )?;
        DestroyWindow(hwnd);
        UnregisterClassW(name, module);
    }
    Ok(())
}
