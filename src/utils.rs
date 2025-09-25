use std::path::Path;
use winit::window::Icon;

#[allow(dead_code)]
pub fn load_icon(path: &Path) -> Result<Icon, Box<dyn std::error::Error>> {
    // std::fs::read 在运行时读取文件，返回 Result
    let image_bytes = std::fs::read(path)?;

    // image::load_from_memory_with_format 也返回 Result
    let image = image::load_from_memory(&image_bytes)?;

    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();

    // Icon::from_rgba 也可能失败，所以我们用 '?' 来传播错误
    let icon = Icon::from_rgba(rgba.into_raw(), width, height)?;

    Ok(icon)
}