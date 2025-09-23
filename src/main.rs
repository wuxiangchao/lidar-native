use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use winit::event::{DeviceEvent, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowBuilder};

mod camera;
mod common;
mod processing;
mod renderer;
mod logger;

mod network;

use camera::CameraController;
use common::Point;
use processing::AngleFinder;
use renderer::Renderer;

struct AppState {
    renderer: Renderer,
    camera_controller: CameraController,
    angle_finder: Arc<AngleFinder>,
    points: Vec<Point>,
    point_size: f32,
    log_buffer: Arc<Mutex<Vec<String>>>,
    ip_addr: String,
    port: String,
    is_listening: bool,
    shutdown_tx: Option<watch::Sender<bool>>,
    data_rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl AppState {
    async fn new(window: Arc<Window>) -> Self {
        let renderer = Renderer::new(window).await;
        let camera_controller = CameraController::new();
        let angle_finder = Arc::new(AngleFinder::new("data/MEMS_Voltage_9.18-2.xlsx", "Sheet1").unwrap());
        let log_buffer = Arc::new(Mutex::new(Vec::new()));

        logger::init(log_buffer.clone()).unwrap();
        log::info!("Logger initialized. Application starting...");

        Self {
            renderer,
            camera_controller,
            angle_finder,
            points: Vec::new(),
            point_size: 0.1,
            log_buffer,
            ip_addr: "192.168.1.102".to_string(),
            port: "1234".to_string(),
            is_listening: false,
            shutdown_tx: None,
            data_rx: None,
        }
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_path = "C:/Windows/Fonts/msyh.ttc";
    if let Ok(font_bytes) = std::fs::read(font_path) {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(font_bytes),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "my_font".to_owned());
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(0, "my_font".to_owned());
        }
        ctx.set_fonts(fonts);
        log::info!("Chinese font setup complete.");
    } else {
        log::error!("Failed to load font: {}. Chinese characters may not display correctly.", font_path);
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("BitCi-MEMS-LiDAR上位机 v0.1.0.9")
            .with_inner_size(winit::dpi::LogicalSize::new(1600, 900))
            .build(&event_loop)
            .unwrap()
    );

    let mut app_state = pollster::block_on(AppState::new(window.clone()));
    let mut egui_state = egui_winit::State::new(egui::Context::default(), egui::ViewportId::default(), &window, None, None);

    setup_fonts(egui_state.egui_ctx());

    let mut egui_renderer = egui_wgpu::Renderer::new(&app_state.renderer.device, app_state.renderer.config.format, None, 1);

    let rt = tokio::runtime::Runtime::new().unwrap();

    event_loop.run(move |event, window_target| {
        // 使用 Poll 模式，让应用尽可能快地重绘，适合实时渲染
        window_target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => {

                let response = egui_state.on_window_event(&window, event);

                // 如果 egui 没有完全捕获事件，再让相机处理
                // if !response.consumed {
                app_state.camera_controller.process_window_events(event);
                // }

                if !egui_state.egui_ctx().is_using_pointer() {
                    match event {
                        WindowEvent::CursorMoved { position, .. } => {
                            app_state.camera_controller.process_mouse_move(*position);
                        }
                        WindowEvent::MouseWheel { delta, .. } => {
                            app_state.camera_controller.process_scroll(delta);
                        }

                        _ => {}
                    }
                }

                match event {
                    WindowEvent::CloseRequested => window_target.exit(),
                    WindowEvent::Resized(physical_size) => app_state.renderer.resize(*physical_size),
                    WindowEvent::RedrawRequested => {
                        //
                        if let Some(rx) = app_state.data_rx.as_mut() {
                            while let Ok(byte_data) = rx.try_recv() {
                                let new_points = processing::bytes_to_points(&byte_data, &app_state.angle_finder);
                                if !new_points.is_empty() {
                                    app_state.points = new_points;
                                    app_state.renderer.update_point_cloud(&app_state.points);
                                }
                            }
                        }

                        // --- UI & 渲染 ---
                        let raw_input = egui_state.take_egui_input(&window);
                        let full_output = egui_state.egui_ctx().run(raw_input, |ctx| {
                            // ctx.style_mut(|style| style.interaction.show_context_menu_on_long_press = false);
                            egui::SidePanel::left("control_panel").min_width(250.0).show(ctx, |ui| {
                                ui.heading("激光雷达控制面板");
                                ui.separator();

                                ui.group(|ui| {
                                    ui.label("UDP网络设置");
                                    ui.add_enabled(!app_state.is_listening, egui::TextEdit::singleline(&mut app_state.ip_addr));
                                    ui.add_enabled(!app_state.is_listening, egui::TextEdit::singleline(&mut app_state.port));

                                    if app_state.is_listening {
                                        if ui.button("断开").clicked() {
                                            log::info!("Disconnect button clicked.");
                                            if let Some(tx) = app_state.shutdown_tx.take() { tx.send(true).unwrap(); }
                                            app_state.is_listening = false;
                                            app_state.data_rx = None;
                                        }
                                    } else {
                                        if ui.button("连接").clicked() {
                                            log::info!("Connect button clicked.");
                                            let (shutdown_tx, shutdown_rx) = watch::channel(false);
                                            let (data_tx, data_rx) = mpsc::channel(100);
                                            app_state.shutdown_tx = Some(shutdown_tx);
                                            app_state.data_rx = Some(data_rx);

                                            let ip = app_state.ip_addr.clone();
                                            let port = app_state.port.clone();
                                            rt.spawn(async move {
                                                if let Err(e) = network::run_udp_listener(ip, port, data_tx, shutdown_rx).await {
                                                    log::error!("UDP listener task failed: {}", e);
                                                }
                                            });
                                            app_state.is_listening = true;
                                        }
                                    }
                                });
                                ui.separator();
                                if ui.button("重置视角").clicked() { app_state.camera_controller.reset(); }
                                if ui.button("生成测试点云").clicked() {
                                    let points = generate_test_points(&app_state.angle_finder);
                                    log::info!("Generated {} test points.", points.len());
                                    app_state.renderer.update_point_cloud(&points);
                                    app_state.points = points;
                                }
                                ui.separator();
                                ui.add(egui::Slider::new(&mut app_state.point_size, 0.05..=1.0).text("点云大小"));
                                ui.separator();
                                ui.heading("状态");
                                ui.label(format!("点云数量: {}", app_state.points.len()));
                                ui.label(format!("连接状态: {}", if app_state.is_listening {"已连接"} else {"未连接"}));
                            });

                            egui::TopBottomPanel::bottom("log_panel").resizable(true).min_height(200.0).show(ctx, |ui| {
                                ui.horizontal(|ui| {
                                    ui.heading("日志");
                                    if ui.button("清空日志").clicked() { app_state.log_buffer.lock().unwrap().clear(); }
                                });
                                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                                    let log_buffer = app_state.log_buffer.lock().unwrap();
                                    for msg in log_buffer.iter() { ui.label(msg); }
                                });
                            });

                            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_ui| {});
                        });

                        egui_state.handle_platform_output(&window, full_output.platform_output);
                        let paint_jobs = egui_state.egui_ctx().tessellate(full_output.shapes, window.scale_factor() as f32);
                        for (id, image_delta) in &full_output.textures_delta.set {
                            egui_renderer.update_texture(&app_state.renderer.device, &app_state.renderer.queue, *id, image_delta);
                        }
                        for id in &full_output.textures_delta.free { egui_renderer.free_texture(id); }

                        let screen_descriptor = egui_wgpu::ScreenDescriptor {
                            size_in_pixels: [app_state.renderer.size.width, app_state.renderer.size.height],
                            pixels_per_point: window.scale_factor() as f32,
                        };

                        let aspect_ratio = app_state.renderer.size.width as f32 / app_state.renderer.size.height as f32;
                        let view_proj = app_state.camera_controller.build_view_projection_matrix(aspect_ratio);
                        app_state.renderer.update_uniforms(view_proj, app_state.point_size);

                        match app_state.renderer.render(&mut egui_renderer, &paint_jobs, &screen_descriptor) {
                            Ok(_) => {},
                            Err(wgpu::SurfaceError::Lost) => app_state.renderer.resize(app_state.renderer.size),
                            Err(wgpu::SurfaceError::OutOfMemory) => window_target.exit(),
                            Err(e) => log::error!("Render error: {:?}", e),
                        }
                    }
                    _ => {}
                }
            }
            Event::DeviceEvent { event: device_event, .. } => {
                // log::info!("DeviceEvent received: {:?}", device_event);
                // if !egui_state.egui_ctx().wants_pointer_input() {
                //     log::info!("mouse motion condition met");
                //     app_state.camera_controller.process_device_events(&device_event);
                // }
            }
            Event::AboutToWait => { window.request_redraw(); }
            _ => {}
        }
    }).unwrap();
}

fn generate_test_points(finder: &Arc<AngleFinder>) -> Vec<Point> {
    // ...
    let mut sample_bytes = Vec::new();
    const V3_MIN: f64 = 2.11; const V3_MAX: f64 = 8.20;
    const V4_MIN: f64 = 1.70; const V4_MAX: f64 = 9.03;
    let v3_range = V3_MAX - V3_MIN;
    let v4_range = V4_MAX - V4_MIN;
    for i in 0..5000 {
        let progress = i as f64 / 4999.0;
        let v_u3 = V3_MIN + v3_range * progress;
        let v_u4 = V4_MIN + v4_range * ((progress * std::f64::consts::PI * 4.0).sin() * 0.5 + 0.5);
        let tof = 20 + (i % 80);
        let v3_16 = (v_u3 / 10.0 * 65535.0) as u16;
        let v4_16 = (v_u4 / 10.0 * 65535.0) as u16;
        sample_bytes.extend_from_slice(&v3_16.to_be_bytes());
        sample_bytes.extend_from_slice(&v4_16.to_be_bytes());
        sample_bytes.push(tof as u8);
    }
    processing::bytes_to_points(&sample_bytes, finder)
}