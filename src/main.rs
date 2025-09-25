// src/main.rs
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

mod app;
mod camera;
mod common;
mod logger;
mod network;
mod processing;
mod renderer;
mod ui;

mod utils;

use app::App;

#[tokio::main]
async fn main() {
    let event_loop = EventLoop::new().unwrap();

    // 再窗口实例化之前，实例化渲染需要的资源，避免闪屏
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("Failed to find an appropriate adapter");

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: None,
            },
            None,
        )
        .await
        .expect("Failed to create device");

    // 实例化屏幕
    let window = Arc::new(
        WindowBuilder::new()
            .with_visible(false)
            .with_title("BitCi-MEMS-LiDAR上位机 v0.1.0.9")
            .with_maximized(true)
            .build(&event_loop)
            .unwrap(),
    );

    // 解决加载黑屏
    let mut app = App::new(window.clone(),instance, adapter, device, queue).await;

    window.set_visible(true);

    event_loop.run(move |event, window_target| {
        window_target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => {
                app.handle_event(&window, event);
                match event {
                    WindowEvent::CloseRequested => window_target.exit(),
                    WindowEvent::Resized(physical_size) => app.state.renderer.resize(*physical_size),
                    WindowEvent::RedrawRequested => {
                        app.update_and_draw(&window, window_target);
                    }
                    _ => {}
                }
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    }).unwrap();
}