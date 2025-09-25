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
    is_left_mouse_pressed: bool,
    is_right_mouse_pressed: bool,
    is_middle_mouse_preesed: bool,
    last_mouse_pos: Option<PhysicalPosition<f64>>,
}

impl CameraController {
    pub fn new() -> Self {
        Self {
            scroll: 1.0,
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        self.rotate_horizontal = 0.0;
        self.rotate_vertical = 0.0;
        self.scroll = 1.0;
        self.pan = Vec2::ZERO;
    }

    pub fn process_scroll(&mut self, delta: &MouseScrollDelta) {
        let scroll_amount = -match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 0.05,
            MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => *y as f32 * 0.01,
        };
        self.scroll += scroll_amount;
        self.scroll = self.scroll.clamp(0.01, 10.0);
    }

    // 新增一个函数来处理光标移动
    pub fn process_mouse_move(&mut self, new_position: PhysicalPosition<f64>) {
        if let Some(last_pos) = self.last_mouse_pos {
            let dx = (new_position.x - last_pos.x) as f32;
            let dy = (new_position.y - last_pos.y) as f32;

            if self.is_left_mouse_pressed {
                self.rotate_horizontal -= dx * 0.005;
                self.rotate_vertical -= dy * 0.005;
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
                MouseButton::Middle => self.is_middle_mouse_preesed = * state == ElementState::Pressed,
                _ => {}
            }

            if !self.is_left_mouse_pressed && !self.is_right_mouse_pressed {
                self.last_mouse_pos = None;
            }
        }
    }

    pub fn build_view_projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        let radius = 50.0 * self.scroll;
        let eye = Vec3::new(
            self.rotate_horizontal.sin() * self.rotate_vertical.cos() * radius,
            self.rotate_vertical.sin() * radius,
            self.rotate_horizontal.cos() * self.rotate_vertical.cos() * radius,
        );

        let up = Vec3::Y;
        let forward = (Vec3::ZERO - eye).normalize();
        let right = up.cross(forward).normalize();
        let view_up = forward.cross(right);

        // 平移是在相机平面上进行的
        let target = self.pan.x * right + self.pan.y * view_up;

        let view = Mat4::look_at_rh(eye + target, target, view_up);
        let proj = Mat4::perspective_rh(45.0f32.to_radians(), aspect_ratio, 0.1, 1000.0);

        #[rustfmt::skip]
        const OPENGL_TO_WGPU_MATRIX: Mat4 = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0,
        ]);

        OPENGL_TO_WGPU_MATRIX * proj * view
    }
}