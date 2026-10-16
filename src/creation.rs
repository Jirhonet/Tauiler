//! Loader and lifecycle of the architecture-matched CBT hook modules.
#[allow(dead_code)]
#[path = "../creation-hook/src/protocol.rs"]
pub mod protocol;
use std::{
    error::Error,
    path::Path,
    process::{Child, Command},
    ptr::null_mut,
};
use windows_sys::Win32::{Foundation::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*};

pub struct CreationHook {
    module: HMODULE,
    hook: HHOOK,
    show_hook: HHOOK,
    helper: Option<Child>,
}
impl CreationHook {
    pub fn install(controller: HWND) -> Result<Self, Box<dyn Error>> {
        Self::from_directory(
            controller,
            std::env::current_exe()?
                .parent()
                .ok_or("executable directory unavailable")?,
        )
    }
    pub fn from_directory(controller: HWND, directory: &Path) -> Result<Self, Box<dyn Error>> {
        use std::os::windows::process::CommandExt;
        let path = directory.join("tauiler_creation_hook.dll");
        let wide: Vec<_> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            let module = LoadLibraryExW(
                wide.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if module.is_null() {
                return Err(format!(
                    "cannot load {}: {}",
                    path.display(),
                    std::io::Error::last_os_error()
                )
                .into());
            }
            let Some(proc) = GetProcAddress(module, c"tauiler_creation_hook".as_ptr().cast())
            else {
                FreeLibrary(module);
                return Err("creation hook export missing".into());
            };
            let callback =
                std::mem::transmute::<unsafe extern "system" fn() -> isize, HOOKPROC>(proc);
            let hook = SetWindowsHookExW(WH_CBT, callback, module, 0);
            if hook.is_null() {
                FreeLibrary(module);
                return Err(std::io::Error::last_os_error().into());
            }
            let mut result = Self {
                module,
                hook,
                show_hook: null_mut(),
                helper: None,
            };
            let Some(show_proc) = GetProcAddress(module, c"tauiler_show_hook".as_ptr().cast())
            else {
                return Err("first-show hook export missing".into());
            };
            let show_callback =
                std::mem::transmute::<unsafe extern "system" fn() -> isize, HOOKPROC>(show_proc);
            result.show_hook = SetWindowsHookExW(WH_CALLWNDPROC, show_callback, module, 0);
            if result.show_hook.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let helper_path = directory.join("tauiler-hook-host32.exe");
            if helper_path.exists() {
                let mut pid = 0;
                GetWindowThreadProcessId(controller, &mut pid);
                result.helper = Some(
                    Command::new(helper_path)
                        .arg((controller as usize).to_string())
                        .arg(pid.to_string())
                        .creation_flags(0x08000000)
                        .spawn()?,
                );
            } else {
                crate::report!("32-bit creation helper missing; 32-bit apps use normal adoption");
            }
            Ok(result)
        }
    }
}
impl Drop for CreationHook {
    fn drop(&mut self) {
        unsafe {
            UnhookWindowsHookEx(self.hook);
            if !self.show_hook.is_null() {
                UnhookWindowsHookEx(self.show_hook);
            }
            FreeLibrary(self.module);
        }
        if let Some(child) = self.helper.as_mut() {
            // Only the helper process spawned by this owner is terminated. Windows
            // releases its global hook; no application process is stopped.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn take_pointer(hwnd: HWND) -> Option<(i32, i32)> {
    use protocol::*;
    unsafe {
        if GetPropW(hwnd, OWNER) as usize as u32
            != windows_sys::Win32::System::Threading::GetCurrentProcessId()
        {
            return None;
        }
        let point = (
            GetPropW(hwnd, POINT_X) as usize as i32,
            GetPropW(hwnd, POINT_Y) as usize as i32,
        );
        for key in [OWNER, POINT_X, POINT_Y, X, Y, WIDTH, HEIGHT] {
            RemovePropW(hwnd, key);
        }
        Some(point)
    }
}
