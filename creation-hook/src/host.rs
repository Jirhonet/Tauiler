#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{mem::zeroed, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*};

fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let controller = args.get(1).ok_or("controller required")?.parse::<usize>()? as HWND;
    let expected_pid = args
        .get(2)
        .ok_or("controller PID required")?
        .parse::<u32>()?;
    let path = std::env::current_exe()?.with_file_name("tauiler_creation_hook32.dll");
    let path: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let module = LoadLibraryW(path.as_ptr());
        if module.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let proc = GetProcAddress(module, c"tauiler_creation_hook".as_ptr().cast());
        let Some(proc) = proc else {
            FreeLibrary(module);
            return Err("missing hook export".into());
        };
        let callback = std::mem::transmute::<unsafe extern "system" fn() -> isize, HOOKPROC>(proc);
        let hook = SetWindowsHookExW(WH_CBT, callback, module, 0);
        if hook.is_null() {
            FreeLibrary(module);
            return Err(std::io::Error::last_os_error().into());
        }
        let Some(show_proc) = GetProcAddress(module, c"tauiler_show_hook".as_ptr().cast()) else {
            UnhookWindowsHookEx(hook);
            FreeLibrary(module);
            return Err("first-show hook export missing".into());
        };
        let show_callback =
            std::mem::transmute::<unsafe extern "system" fn() -> isize, HOOKPROC>(show_proc);
        let show_hook = SetWindowsHookExW(WH_CALLWNDPROC, show_callback, module, 0);
        if show_hook.is_null() {
            UnhookWindowsHookEx(hook);
            FreeLibrary(module);
            return Err(std::io::Error::last_os_error().into());
        }
        SetTimer(null_mut(), 1, 250, None);
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let mut pid = 0;
            GetWindowThreadProcessId(controller, &mut pid);
            if pid != expected_pid || IsWindow(controller) == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
        UnhookWindowsHookEx(show_hook);
        FreeLibrary(module);
    }
    Ok(())
}
