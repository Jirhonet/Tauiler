use windows_sys::{Win32::UI::WindowsAndMessaging::*, core::PCWSTR};

pub const CONTROLLER: PCWSTR = windows_sys::core::w!("Tauiler.Controller");
pub const MAGIC: usize = 0x5441_5532;
pub const OWNER: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.Owner");
pub const X: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.X");
pub const Y: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.Y");
pub const WIDTH: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.Width");
pub const HEIGHT: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.Height");
pub const POINT_X: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.PointX");
pub const POINT_Y: PCWSTR = windows_sys::core::w!("Tauiler.Creation.v2.PointY");

// Fixed-width, pointer-free payload: identical on x86 and x64. WM_COPYDATA
// supplies the OS-marshalled buffer only for the duration of the call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Request {
    pub window: u64,
    pub process: u32,
    pub thread: u32,
    pub phase: u32,
    pub reserved: u32,
    pub style: u32,
    pub ex_style: u32,
    pub point_x: i32,
    pub point_y: i32,
    pub class: [u16; 256],
    pub title: [u16; 512],
}
const _: () = assert!(std::mem::size_of::<Request>() == 1576);

pub fn candidate(style: u32, ex_style: u32, owned: bool) -> bool {
    !owned
        && style & (WS_CHILD | WS_POPUP | WS_MINIMIZE | WS_MAXIMIZE) == 0
        && style & WS_CAPTION == WS_CAPTION
        && style & WS_THICKFRAME != 0
        && ex_style & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) == 0
}

pub fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_normal_resizable_top_level_windows_are_candidates() {
        assert!(candidate(WS_OVERLAPPEDWINDOW, 0, false));
        for excluded in [WS_CHILD, WS_POPUP, WS_MINIMIZE, WS_MAXIMIZE] {
            assert!(!candidate(WS_OVERLAPPEDWINDOW | excluded, 0, false));
        }
        assert!(!candidate(WS_OVERLAPPEDWINDOW, WS_EX_TOOLWINDOW, false));
        assert!(!candidate(WS_OVERLAPPEDWINDOW, 0, true));
        assert!(!candidate(WS_CAPTION, 0, false));
    }
}
