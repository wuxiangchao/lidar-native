use std::path::Path;
use winit::window::Icon;
use crate::common::Point;
use glam::Vec3;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum InteractionMode {
    Camera,
    Measuring,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ColoringMode {
    White,
    ByHeight,
}

#[allow(dead_code)]
pub fn load_icon(path: &Path) -> Result<Icon, Box<dyn std::error::Error>> {
    let image_bytes = std::fs::read(path)?;
    let image = image::load_from_memory(&image_bytes)?;
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    let icon = Icon::from_rgba(rgba.into_raw(), width, height)?;
    Ok(icon)
}

pub fn calculate_aabb(points: &[Point]) -> Option<(Vec3, Vec3)> {
    if points.is_empty() {
        return None;
    }
    let mut min = points[0].position;
    let mut max = points[0].position;
    for p in points.iter().skip(1) {
        min = min.min(p.position);
        max = max.max(p.position);
    }
    Some((min, max))
}

pub fn gradient_map(value: f32) -> [f32; 4] {
    let value = value.clamp(0.0, 1.0);
    let r = (value * 2.0 - 1.0).clamp(0.0, 1.0);
    let g = (1.0 - (value * 2.0 - 1.0).abs()).clamp(0.0, 1.0);
    let b = (1.0 - value * 2.0).clamp(0.0, 1.0);
    [r, g, b, 1.0]
}