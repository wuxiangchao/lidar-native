use std::path::Path;
use winit::window::Icon;

#[allow(dead_code)]
pub fn load_icon(path: &Path) -> Result<Icon, Box<dyn std::error::Error>> {
    let image_bytes = std::fs::read(path)?;
    let image = image::load_from_memory(&image_bytes)?;
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    let icon = Icon::from_rgba(rgba.into_raw(), width, height)?;
    Ok(icon)
}