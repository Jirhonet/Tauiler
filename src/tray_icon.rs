//! Padded, antialiased number textures using the Windows UI typeface.
use std::{
    error::Error,
    mem::{size_of, zeroed},
    ptr::null_mut,
};
use windows_sys::Win32::Graphics::Gdi::*;
pub const SIZE: usize = 32;
const SCALE: usize = 4;
const RENDER_SIZE: usize = SIZE * SCALE;
pub fn label(slot: u32) -> String {
    if slot == 10 {
        "0".into()
    } else {
        slot.max(1).to_string()
    }
}

struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    font: HFONT,
    old_bitmap: HGDIOBJ,
    old_font: HGDIOBJ,
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            if !self.old_font.is_null() {
                SelectObject(self.dc, self.old_font);
            }
            if !self.old_bitmap.is_null() {
                SelectObject(self.dc, self.old_bitmap);
            }
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
        }
    }
}
/// Four-times rendering and downsampling gives opaque, smooth BGRA number textures.
pub fn pixels(slot: u32, paused: bool) -> Result<Vec<u8>, Box<dyn Error>> {
    unsafe {
        let mut canvas = Canvas {
            dc: CreateCompatibleDC(null_mut()),
            bitmap: null_mut(),
            font: null_mut(),
            old_bitmap: null_mut(),
            old_font: null_mut(),
        };
        if canvas.dc.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = RENDER_SIZE as i32;
        info.bmiHeader.biHeight = -(RENDER_SIZE as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        canvas.bitmap =
            CreateDIBSection(canvas.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if canvas.bitmap.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        canvas.old_bitmap = SelectObject(canvas.dc, canvas.bitmap);
        let background = if paused {
            [40, 40, 220, 255]
        } else {
            [0, 0, 0, 255]
        };
        {
            let buffer =
                std::slice::from_raw_parts_mut(bits as *mut u8, RENDER_SIZE * RENDER_SIZE * 4);
            for pixel in buffer.as_chunks_mut::<4>().0 {
                *pixel = background;
            }
        }
        let text = label(slot);
        let height = if text.len() == 1 { 92 } else { 76 };
        canvas.font = CreateFontW(
            -height,
            0,
            0,
            0,
            800,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32,
            DEFAULT_PITCH as u32,
            windows_sys::core::w!("Segoe UI"),
        );
        if canvas.font.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        canvas.old_font = SelectObject(canvas.dc, canvas.font);
        SetBkMode(canvas.dc, TRANSPARENT as i32);
        SetTextColor(canvas.dc, 0x00ff_ffff);
        let text: Vec<u16> = text.encode_utf16().collect();
        let padding = 5 * SCALE as i32;
        let mut rect = windows_sys::Win32::Foundation::RECT {
            left: padding,
            top: padding,
            right: RENDER_SIZE as i32 - padding,
            bottom: RENDER_SIZE as i32 - padding,
        };
        if DrawTextW(
            canvas.dc,
            text.as_ptr(),
            text.len() as i32,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        ) == 0
        {
            return Err("cannot render workspace number".into());
        }
        GdiFlush();
        let buffer = std::slice::from_raw_parts(bits as *const u8, RENDER_SIZE * RENDER_SIZE * 4);
        let mut output = vec![0u8; SIZE * SIZE * 4];
        for y in 0..SIZE {
            for x in 0..SIZE {
                let target = (y * SIZE + x) * 4;
                let mut covered = 0u32;
                for sy in 0..SCALE {
                    for sx in 0..SCALE {
                        let px = (x * SCALE + sx) as f64 + 0.5;
                        let py = (y * SCALE + sy) as f64 + 0.5;
                        let radius = 7.0 * SCALE as f64;
                        let cx = px.clamp(radius, RENDER_SIZE as f64 - radius);
                        let cy = py.clamp(radius, RENDER_SIZE as f64 - radius);
                        if (px - cx).powi(2) + (py - cy).powi(2) <= radius * radius {
                            covered += 1;
                        }
                    }
                }
                for channel in 0..3 {
                    let mut total = 0u32;
                    for sy in 0..SCALE {
                        for sx in 0..SCALE {
                            total += buffer
                                [((y * SCALE + sy) * RENDER_SIZE + x * SCALE + sx) * 4 + channel]
                                as u32;
                        }
                    }
                    // Windows 32-bit icons use premultiplied alpha at rounded edges.
                    output[target + channel] =
                        (total * covered / ((SCALE * SCALE * SCALE * SCALE) as u32)) as u8;
                }
                output[target + 3] = (covered * 255 / (SCALE * SCALE) as u32) as u8;
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspaces_have_smooth_padded_numbers_and_pause_background() {
        let mut seen = std::collections::HashSet::new();
        for slot in 1..=10 {
            let normal = pixels(slot, false).unwrap();
            let paused = pixels(slot, true).unwrap();
            assert_eq!(normal.len(), SIZE * SIZE * 4);
            assert!(seen.insert(normal.clone()));
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let at = (y * SIZE + x) * 4;
                    assert_eq!(normal[at + 3], paused[at + 3]);
                    assert!(normal[at..at + 3].iter().all(|v| *v <= normal[at + 3]));
                    assert!(paused[at..at + 3].iter().all(|v| *v <= paused[at + 3]));
                    if !(4..SIZE - 4).contains(&x) || !(4..SIZE - 4).contains(&y) {
                        assert_eq!(&normal[at..at + 3], &[0, 0, 0]);
                        if paused[at + 3] == 255 {
                            assert_eq!(&paused[at..at + 3], &[40, 40, 220]);
                        }
                    }
                }
            }
            assert!(
                normal
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 0 && p[0] < 255)
            );
            assert!(normal.as_chunks::<4>().0.iter().any(|p| p[0] > 220));
            assert_eq!(&normal[..4], &[0, 0, 0, 0]);
            assert!(
                normal
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 0 && p[3] < 255)
            );
        }
    }
    #[test]
    fn workspace_ten_uses_zero() {
        assert_eq!(label(10), "0");
        assert_eq!(label(1), "1");
        assert_eq!(label(11), "11");
    }
}
