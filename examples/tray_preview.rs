#[path = "../src/tray_icon.rs"]
mod tray_icon;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 480;
    let height = 96;
    let mut image = [45u8, 45, 45, 255].repeat(width * height);
    for paused in [false, true] {
        for slot in 1..=10 {
            let icon = tray_icon::pixels(slot, paused)?;
            let x = (slot as usize - 1) * 48 + 8;
            let y = if paused { 56 } else { 8 };
            for row in 0..tray_icon::SIZE {
                let start = ((y + row) * width + x) * 4;
                let source = row * tray_icon::SIZE * 4;
                for column in 0..tray_icon::SIZE {
                    let target = start + column * 4;
                    let source = source + column * 4;
                    let alpha = icon[source + 3] as u32;
                    for channel in 0..3 {
                        image[target + channel] = (icon[source + channel] as u32
                            + 45 * (255 - alpha) / 255)
                            .min(255) as u8;
                    }
                    image[target + 3] = 255;
                }
            }
        }
    }
    std::fs::create_dir_all("target")?;
    std::fs::write("target/tray-preview.bgra", image)?;
    Ok(())
}
