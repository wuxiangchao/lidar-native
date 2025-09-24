use std::fs::File;
use std::io::{BufRead, BufReader,BufWriter, Write};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowBuilder};

use egui_plot::{Bar, BarChart, Legend, Line, Plot, PlotPoints};

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

#[derive(PartialEq, Eq)]
enum AppTab {
    Controls,
    Charts,
}

struct AppState {
    renderer: Renderer,
    camera_controller: CameraController,
    angle_finder: Arc<AngleFinder>,
    points: Arc<Mutex<Vec<Point>>>,
    point_size: f32,
    log_buffer: Arc<Mutex<Vec<String>>>,
    ip_addr: String,
    port: String,
    is_listening: bool,
    shutdown_tx: Option<watch::Sender<bool>>,
    data_rx: Option<mpsc::Receiver<Vec<u8>>>,
    target_ip_addr: String,
    target_port: String,
    active_tab: AppTab,
    waveform_data: VecDeque<[f64; 2]>,
    waveform_counter: u64,
    histogram_bins: Vec<Bar>,
    point_loader_rx: mpsc::Receiver<Vec<Point>>,
    point_loader_tx: mpsc::Sender<Vec<Point>>,
}

impl AppState {
    async fn new(window: Arc<Window>) -> Self {
        let renderer = Renderer::new(window).await;
        let camera_controller = CameraController::new();
        let angle_finder = Arc::new(AngleFinder::new("data/MEMS_Voltage_9.18-2.xlsx", "Sheet1").unwrap());
        let log_buffer = Arc::new(Mutex::new(Vec::new()));

        logger::init(log_buffer.clone()).unwrap();
        log::info!("Logger initialized. Application starting...");

        let histogram_bins = (0..20)
            .map(|i| Bar::new((i * 5 + 2) as f64, 0.0)
                .width(4.5))
            .collect();

        let (point_loader_tx, point_loader_rx) = mpsc::channel(1);

        Self {
            renderer,
            camera_controller,
            angle_finder,
            points: Arc::new(Mutex::new(Vec::new())),
            point_size: 0.05,
            log_buffer,
            ip_addr: "192.168.1.102".to_string(),
            port: "1234".to_string(),
            is_listening: false,
            shutdown_tx: None,
            data_rx: None,
            target_ip_addr: "192.168.1.10".to_string(),
            target_port: "1234".to_string(),
            active_tab: AppTab::Controls, // Default to the controls tab
            waveform_data: VecDeque::with_capacity(512), // 波形图最多显示最近的512个点
            waveform_counter: 0,
            histogram_bins,
            point_loader_rx,
            point_loader_tx,
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
        window_target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => {
                let _ = egui_state.on_window_event(&window, event);
                app_state.camera_controller.process_window_events(event);

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
                        if let Ok(loaded_points) = app_state.point_loader_rx.try_recv() {
                            let mut points = app_state.points.lock().unwrap();
                            *points = loaded_points;
                            app_state.renderer.update_point_cloud(&points);
                            log::info!("Successfully loaded {} points from file.", points.len());
                        }

                        if let Some(rx) = app_state.data_rx.as_mut() {
                            while let Ok(byte_data) = rx.try_recv() {
                                // 更新3D点云
                                let new_points = processing::bytes_to_points(&byte_data, &app_state.angle_finder);
                                if !new_points.is_empty() {
                                    let mut points = app_state.points.lock().unwrap();
                                    points.extend(&new_points);
                                    app_state.renderer.update_point_cloud(&points);
                                }

                                // 更新图表数据
                                for chunk in byte_data.chunks_exact(5) {
                                    let tof = chunk[4];
                                    let dist = f64::from(tof) * 0.375;

                                    app_state.waveform_data.push_back([app_state.waveform_counter as f64, dist]);
                                    app_state.waveform_counter += 1;
                                    if app_state.waveform_data.len() > 512 {
                                        app_state.waveform_data.pop_front();
                                    }

                                    let bin_index = (dist / 5.0).floor() as usize;
                                    if bin_index < app_state.histogram_bins.len() {
                                        app_state.histogram_bins[bin_index].value += 1.0;
                                    }
                                }
                            }
                        }

                        let raw_input = egui_state.take_egui_input(&window);
                        let full_output = egui_state.egui_ctx().run(raw_input, |ctx| {
                            egui::SidePanel::left("control_panel").min_width(350.0).show(ctx, |ui| {
                                ui.heading("控制面板");
                                ui.separator();

                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut app_state.active_tab, AppTab::Controls, "⚙️ 控制");
                                    ui.selectable_value(&mut app_state.active_tab, AppTab::Charts, "📊 图表");
                                });
                                ui.separator();

                                match app_state.active_tab {
                                    AppTab::Controls => {
                                        ui.group(|ui| {
                                            ui.label("本地监听设置");
                                            ui.add_enabled(!app_state.is_listening, egui::TextEdit::singleline(&mut app_state.ip_addr));
                                            ui.add_enabled(!app_state.is_listening, egui::TextEdit::singleline(&mut app_state.port));
                                        });

                                        ui.group(|ui| {
                                            ui.label("目标设备设置");
                                            ui.text_edit_singleline(&mut app_state.target_ip_addr);
                                            ui.text_edit_singleline(&mut app_state.target_port);
                                        });

                                        ui.group(|ui| {
                                            ui.horizontal(|ui| {
                                                if app_state.is_listening {
                                                    if ui.button("✖️ 断开连接").clicked() {
                                                        if let Some(tx) = app_state.shutdown_tx.take() { tx.send(true).unwrap(); }
                                                        app_state.is_listening = false;
                                                        app_state.data_rx = None;
                                                    }
                                                } else {
                                                    if ui.button("🔌 建立连接").clicked() {
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

                                                ui.add_enabled_ui(app_state.is_listening, |ui| {
                                                    if ui.button("▶️ 开始测量").clicked() {
                                                        let ip = app_state.target_ip_addr.clone();
                                                        let port = app_state.target_port.clone();
                                                        rt.spawn(async move {
                                                            if let Err(e) = network::send_command(&ip, &port, "5A29").await {
                                                                log::error!("Failed to send command: {}", e);
                                                            }
                                                        });
                                                    }
                                                });

                                                ui.add_enabled_ui(app_state.is_listening, |ui| {
                                                    if ui.button("🚫 结束测量").clicked() {
                                                        let ip = app_state.target_ip_addr.clone();
                                                        let port = app_state.target_port.clone();
                                                        rt.spawn(async move {
                                                            if let Err(e) = network::send_command(&ip, &port, "5A30").await {
                                                                log::error!("Failed to send command: {}", e);
                                                            }
                                                        });
                                                    }
                                                });
                                            });
                                        });

                                        ui.collapsing("🛠️ 高级功能", |ui|{
                                            ui.horizontal(|ui| {
                                                if ui.button("🔄 重置视角").clicked() { app_state.camera_controller.reset(); }
                                                if ui.button("🗑️ 清空点云").clicked() {
                                                    app_state.points.lock().unwrap().clear();
                                                    app_state.renderer.update_point_cloud(&[]);
                                                    log::info!("Point cloud cleared.");
                                                }
                                            });

                                            ui.horizontal(|ui| {
                                                if ui.button("💾 保存点云").clicked() {
                                                    let points_clone = Arc::clone(&app_state.points);
                                                    std::thread::spawn(move || {
                                                        if let Some(path) = rfd::FileDialog::new()
                                                            .add_filter("XYZ Point Cloud", &["xyz"])
                                                            .save_file()
                                                        {
                                                            log::info!("Saving point cloud to: {:?}", path);
                                                            let points_guard = points_clone.lock().unwrap();
                                                            if let Ok(file) = File::create(path) {
                                                                let mut writer = BufWriter::new(file);
                                                                for p in points_guard.iter() {
                                                                    writeln!(writer, "{} {} {}", p.position.x, p.position.y, p.position.z).unwrap();
                                                                }
                                                                log::info!("Successfully saved {} points.", points_guard.len());
                                                            } else {
                                                                log::error!("Failed to create file for saving.");
                                                            }
                                                        }
                                                    });
                                                }

                                                if ui.button("📂 加载点云").clicked() {
                                                    let tx_clone = app_state.point_loader_tx.clone();
                                                    std::thread::spawn(move || {
                                                        if let Some(path) = rfd::FileDialog::new()
                                                            .add_filter("XYZ Point Cloud", &["xyz"])
                                                            .pick_file()
                                                        {
                                                            log::info!("Loading point cloud from: {:?}", path);
                                                            if let Ok(file) = File::open(path) {
                                                                let reader = BufReader::new(file);
                                                                let loaded_points: Vec<Point> = reader.lines()
                                                                    .filter_map(Result::ok)
                                                                    .filter_map(|line| {
                                                                        let parts: Vec<f32> = line.split_whitespace()
                                                                            .filter_map(|s| s.parse::<f32>().ok())
                                                                            .collect();
                                                                        if parts.len() == 3 {
                                                                            Some(Point::new(parts[0], parts[1], parts[2]))
                                                                        } else { None }
                                                                    })
                                                                    .collect();

                                                                // 将加载的点发送回主线程
                                                                tx_clone.blocking_send(loaded_points).unwrap();
                                                            } else {
                                                                log::error!("Failed to open file for loading.");
                                                            }
                                                        }
                                                    });
                                                }
                                            });

                                            ui.add(egui::Slider::new(&mut app_state.point_size, 0.01..=1.0).text("点云大小"));
                                        });
                                    }
                                    AppTab::Charts => {
                                        ui.label("实时距离波形");
                                        let line = Line::new(PlotPoints::from_iter(app_state.waveform_data.iter().copied()));
                                        Plot::new("waveform_plot")
                                            .height(200.0)
                                            .view_aspect(2.0)
                                            .auto_bounds_x()
                                            .auto_bounds_y()
                                            .show(ui, |plot_ui| plot_ui.line(line));

                                        ui.separator();
                                        ui.label("实时距离直方图");
                                        let chart = BarChart::new(app_state.histogram_bins.clone())
                                            .color(egui::Color32::LIGHT_GREEN)
                                            .name("点数");
                                        Plot::new("histogram_plot")
                                            .height(200.0)
                                            .legend(Legend::default())
                                            .show(ui, |plot_ui| plot_ui.bar_chart(chart));
                                    }
                                }

                                ui.separator();
                                ui.heading("📊 状态");
                                ui.label(format!("点云数量: {}", app_state.points.lock().unwrap().len()));
                                ui.label(format!("连接状态: {}", if app_state.is_listening {"已连接"} else {"未连接"}));
                            });

                            egui::TopBottomPanel::bottom("log_panel").resizable(true).min_height(50.0).show(ctx, |ui| {
                                ui.horizontal(|ui| {
                                    ui.heading("📜 日志");
                                    if ui.button("🗑️ 清空").clicked() { app_state.log_buffer.lock().unwrap().clear(); }
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
            Event::AboutToWait => { window.request_redraw(); }
            _ => {}
        }
    }).unwrap();
}

#[warn(dead_code)]
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