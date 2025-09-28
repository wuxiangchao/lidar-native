// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use winit::event::{Event, StartCause, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// custom mod
mod app; // for application
mod camera; // for point cloud view
mod common; // for point define
mod logger; // for log print
mod network; // for udp communication
mod processing; // for data processing
mod renderer; // for renderer
mod ui; // for ui
mod utils; // tool functions

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
            .with_title("北京理工大学重庆微电子研究院MEMS-LiDAR v0.1.1")
            .with_maximized(true)
            .build(&event_loop)
            .unwrap(),
    );

    // app：主要负责后台点云收发，耗时程序，任务调度
    let mut app = App::new(window.clone(),instance, adapter, device, queue).await;


    event_loop.run(move |event, window_target| {
        window_target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::NewEvents(StartCause::Init) => {
                // 这是事件循环开始的信号，立即让窗口可见。
                println!("Event loop initialized. Making window visible.");
                window.set_visible(true);
            }

            // 窗口事件
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => {
                app.handle_event(&window, event);
                match event {
                    // 窗口关闭时间
                    WindowEvent::CloseRequested => window_target.exit(),
                    // 调整窗口尺寸事件
                    WindowEvent::Resized(physical_size) => app.state.renderer.resize(*physical_size),
                    // 窗口重绘事件
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