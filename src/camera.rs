// src/camera.rs

use glam::{Mat4, Vec2, Vec3};
use std::f32::consts::TAU;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

const SAFE_LIMIT: f32 = 0.0001;

#[derive(Default)]
pub struct CameraController {
    rotate_horizontal: f32,
    rotate_vertical: f32,
    scroll: f32,
    pan: Vec2, // 用于平移
    pub center: Vec3,
    pub init_center: Vec3,
    is_left_mouse_pressed: bool,
    is_right_mouse_pressed: bool,
    is_middle_mouse_pressed: bool,
    last_mouse_pos: Option<PhysicalPosition<f64>>,
    is_first_load_center: bool,
}

impl CameraController {
    pub fn new() -> Self {
        Self {
            scroll: 0.05,
            center: Vec3::ZERO,
            init_center: Vec3::ZERO,
            is_first_load_center: true,
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        self.rotate_horizontal = 0.0;
        self.rotate_vertical = 0.0;
        self.scroll = 0.05;
        self.pan = Vec2::ZERO;
        self.center = self.init_center;
    }

    pub fn process_scroll(&mut self, delta: &MouseScrollDelta) {
        let scroll_amount = -match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 0.05,
            MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => *y as f32 * 0.01,
        };
        self.scroll += scroll_amount;
        self.scroll = self.scroll.clamp(0.01, 10.0);
    }

    // 处理光标移动
    pub fn process_mouse_move(&mut self, new_position: PhysicalPosition<f64>) {
        if let Some(last_pos) = self.last_mouse_pos {

            let dx = (new_position.x - last_pos.x) as f32;
            let dy = (new_position.y - last_pos.y) as f32;

            if self.is_left_mouse_pressed {
                self.rotate_horizontal -= dx * 0.005;
                self.rotate_vertical += dy * 0.005;
                self.rotate_vertical = self.rotate_vertical.clamp(-TAU / 4.0 + SAFE_LIMIT, TAU / 4.0 - SAFE_LIMIT);
            }
            if self.is_right_mouse_pressed {
                self.pan.x += dx * 0.05 * self.scroll;
                self.pan.y += dy * 0.05 * self.scroll;
            }
        }

        // 只有在按键按下时才更新 last_mouse_pos，以计算拖动
        if self.is_left_mouse_pressed || self.is_right_mouse_pressed {
            self.last_mouse_pos = Some(new_position);
        } else {
            self.last_mouse_pos = None;
        }
    }


    pub fn process_window_events(&mut self, event: &winit::event::WindowEvent) {
        if let winit::event::WindowEvent::MouseInput { state, button, .. } = event {
            match button {
                MouseButton::Left => self.is_left_mouse_pressed = *state == ElementState::Pressed,
                MouseButton::Right => self.is_right_mouse_pressed = *state == ElementState::Pressed,
                MouseButton::Middle => self.is_middle_mouse_pressed = * state == ElementState::Pressed,
                _ => {}
            }

            if !self.is_left_mouse_pressed && !self.is_right_mouse_pressed {
                self.last_mouse_pos = None;
            }
        }
    }

    pub fn build_view_projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        let radius = 50.0 * self.scroll;

        // 计算相机在以`center`为中心的球体上的位置
        let eye_offset = Vec3::new(
            self.rotate_horizontal.sin() * self.rotate_vertical.cos() * radius,
            self.rotate_vertical.sin() * radius,
            self.rotate_horizontal.cos() * self.rotate_vertical.cos() * radius,
        );
        let eye = self.center + eye_offset;

        // 计算相机的观察方向和上方向
        let up = Vec3::Y;
        let forward = (self.center - eye).normalize();
        let right = up.cross(forward).normalize();
        let view_up = forward.cross(right);

        // 计算平移(pan)效果
        // 平移是在相机平面上进行的，现在它会移动`center`和`eye`
        let pan_offset = self.pan.x * right + self.pan.y * view_up;
        let final_target = self.center + pan_offset;
        let final_eye = eye + pan_offset;

        // 创建视图矩阵
        let view = Mat4::look_at_rh(final_eye, final_target, view_up);
        let proj = Mat4::perspective_rh(45.0f32.to_radians(), aspect_ratio, 0.1, 1000.0);

        #[rustfmt::skip]
        const OPENGL_TO_WGPU_MATRIX: Mat4 = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0,
        ]);

        OPENGL_TO_WGPU_MATRIX * proj * view
    }

    pub fn frame_bounding_box(&mut self, min_corner: Vec3, max_corner: Vec3) {
        let size = min_corner.distance(max_corner);
        self.center = (min_corner + max_corner) / 2.0;

        if self.is_first_load_center{
            self.init_center = self.center;
            self.is_first_load_center = false;
        }

        // 如果点云只有一个点或没有尺寸，给一个默认尺寸
        let effective_size = if size < 1e-6 { 1.0 } else { size };

        // 这是一个常用的启发式算法，让物体刚好充满视图
        let distance = effective_size * 1.5; // 乘1.5留出一些边距

        // 根据距离反推出需要的 scroll 值
        self.scroll = distance / 50.0;

        // 重置平移，确保视图是正对中心的
        self.pan = Vec2::ZERO;

        log::info!("Framing view on point cloud center: {:?}, adjusting scroll to: {}", self.center, self.scroll);
    }
}