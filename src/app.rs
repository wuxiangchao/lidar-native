// src/app.rs

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use winit::event::{WindowEvent, MouseButton, ElementState};
use winit::window::Window;
use glam::{Vec4, Vec4Swizzles};
use winit::dpi::PhysicalPosition;

use crate::camera::CameraController;
use crate::common::Point;
use crate::processing::{self, AngleFinder};
use crate::renderer::Renderer;
use crate::ui;
use crate::utils::{InteractionMode, ColoringMode, calculate_aabb, gradient_map};

#[derive(PartialEq, Eq)]
pub enum AppTab {
    Controls,
    Charts,
}


pub struct AppState {
    // 3D Rendering & Camera
    pub renderer: Renderer,
    pub camera_controller: CameraController,
    pub points: Arc<Mutex<Vec<Point>>>,
    pub point_size: f32,

    // Data Processing
    pub angle_finder: Option<Arc<AngleFinder>>,
    pub is_loading_lookup_table: bool,
    lookup_table_rx: mpsc::Receiver<Result<AngleFinder, anyhow::Error>>,

    // UI State & Data
    pub log_buffer: Arc<Mutex<Vec<String>>>,
    pub active_tab: AppTab,
    pub waveform_data: VecDeque<[f64; 2]>,
    pub waveform_counter: u64,
    pub histogram_bins: Vec<egui_plot::Bar>,

    // Networking
    pub ip_addr: String,
    pub port: String,
    pub is_listening: bool,
    pub target_ip_addr: String,
    pub target_port: String,

    // Asynchronous Communication
    pub shutdown_tx: Option<watch::Sender<bool>>,
    pub data_rx: Option<mpsc::Receiver<Vec<u8>>>,
    pub point_loader_tx: mpsc::Sender<Vec<Point>>,
    pub point_loader_rx: mpsc::Receiver<Vec<Point>>,

    // 增加性能监控字段
    pub last_perf_update: std::time::Instant,
    frame_count_since_last_update: u32,
    points_processed_since_last_update: usize,
    bytes_received_since_last_update: usize,
    pub fps: f32,
    pub pps: u32,
    pub data_rate_kbs: f32,

    // measure tools
    pub interaction_mode: InteractionMode,
    pub measurement_points: Vec<Point>,
    pub measured_distance: Option<f32>,
    pub latest_cursor_position: PhysicalPosition<f64>,

    // camera center
    pub has_centered_on_initial_cloud: bool,

    pub coloring_mode: ColoringMode, //
    pub coloring_dirty: bool,
}

pub struct App {
    pub state: AppState,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

// 用于从后台任务发送结果的通道
type LookupTableSender = mpsc::Sender<Result<AngleFinder, anyhow::Error>>;

impl App {
    pub async fn new(window: Arc<Window>,
                     instance: wgpu::Instance,
                     adapter: wgpu::Adapter,
                     device: wgpu::Device,
                     queue: wgpu::Queue,
    ) -> Self {
        let renderer = Renderer::new(window.clone(),&instance, &adapter, device, queue);
        let camera_controller = CameraController::new();

        // 异步加载查找表
        let (tx, rx): (LookupTableSender, _) = mpsc::channel(1);

        // 启动一个后台任务来加载查找表文件
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                AngleFinder::new(
                    "data/MEMS_Voltage_9.18-2.xlsx",
                    "Sheet1"
                )
            }).await.unwrap(); //
            // 将加载结果
            if tx.send(result).await.is_err() {
                log::error!("Failed to send loaded lookup table back to main thread.");
            }
        });


        let log_buffer = Arc::new(
            Mutex::new(Vec::new())
        );

        crate::logger::init(log_buffer.clone()).unwrap();
        log::info!("Logger initialized. Application starting...");

        let histogram_bins = (0..20)
            .map(|i|
                egui_plot::Bar::new(
                    (i * 5 + 2) as f64,
                    0.0).width(4.5)
            )
            .collect();

        let (point_loader_tx, point_loader_rx) = mpsc::channel(1);

        let egui_state = egui_winit::State::new(
            egui::Context::default(),
            egui::ViewportId::default(),
            &window,
            None,
            None
        );

        setup_fonts(egui_state.egui_ctx());

        let egui_renderer = egui_wgpu::Renderer::new(
            &renderer.device,
            renderer.config.format,
            None,
            1);

        let state = AppState {
            renderer,
            camera_controller,
            angle_finder: None,
            is_loading_lookup_table: true,
            lookup_table_rx: rx,
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
            active_tab: AppTab::Controls,
            waveform_data: VecDeque::with_capacity(512),
            waveform_counter: 0,
            histogram_bins,
            point_loader_rx,
            point_loader_tx,

            // 性能监控字段
            last_perf_update: std::time::Instant::now(),
            frame_count_since_last_update: 0,
            points_processed_since_last_update: 0,
            bytes_received_since_last_update: 0,
            fps: 0.0,
            pps: 0,
            data_rate_kbs: 0.0,

            // 初始化测量工具状态
            interaction_mode: InteractionMode::Camera,
            measurement_points: Vec::new(),
            measured_distance: None,
            latest_cursor_position: PhysicalPosition::default(),

            //
            has_centered_on_initial_cloud: false,

            coloring_mode: ColoringMode::White,
            coloring_dirty: true,
        };

        Self {
            state,
            egui_state,
            egui_renderer,
        }
    }

    fn pick_point(&self, position: PhysicalPosition<f64>) -> Option<Point> {
        let points_guard = self.state.points.lock().unwrap();
        if points_guard.is_empty() {
            return None;
        }

        // 获取必要的矩阵和尺寸
        let size = &self.state.renderer.size;
        let aspect_ratio = size.width as f32 / size.height as f32;
        let view_proj = self.state.camera_controller.build_view_projection_matrix(aspect_ratio);
        let view_proj_inverse = view_proj.inverse();

        // 将屏幕坐标转换为标准化设备坐标 (NDC) [-1..1]
        let ndc_x = (position.x as f32 / size.width as f32) * 2.0 - 1.0;
        let ndc_y = (1.0 - (position.y as f32 / size.height as f32)) * 2.0 - 1.0; // Y轴在屏幕和NDC中通常是相反的

        // 将NDC坐标 un-project 回世界坐标，创建一条射线
        // 我们取近平面(z=0)和远平面(z=1)上的两个点来定义射线
        let near_point = view_proj_inverse * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let far_point = view_proj_inverse * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);

        // 齐次坐标转换回三维坐标
        let near_point = near_point.xyz() / near_point.w;
        let far_point = far_point.xyz() / far_point.w;

        let ray_origin = near_point;
        let ray_direction = (far_point - near_point).normalize();

        // log::info!("Ray created -> Origin: {:?}, Direction: {:?}", ray_origin, ray_direction);

        // 遍历所有点，找到离射线最近的点
        let mut closest_point: Option<Point> = None;
        let mut min_distance_sq = f32::MAX;

        // 设置一个拾取半径，避免选中太远的点
        let pick_radius = self.state.point_size * 25.0;

        for point in points_guard.iter() {
            let p = point.position;
            let oc = p - ray_origin;
            let t = oc.dot(ray_direction);

            // 计算点到射线的垂直距离的平方
            let projected = ray_origin + t * ray_direction;
            let dist_sq = p.distance_squared(projected);

            if dist_sq < pick_radius * pick_radius && dist_sq < min_distance_sq {
                min_distance_sq = dist_sq;
                closest_point = Some(*point);
            }
        }
        closest_point
    }

    fn handle_measurement_click(&mut self) {
        if let Some(picked_point) = self.pick_point(self.state.latest_cursor_position) {
            log::info!("Picked a point at: {:?}", picked_point.position);

            if self.state.measurement_points.len() >= 2 {
                self.state.measurement_points.clear();
                self.state.measured_distance = None;
            }

            self.state.measurement_points.push(picked_point);

            if self.state.measurement_points.len() == 2 {
                let p1 = self.state.measurement_points[0].position;
                let p2 = self.state.measurement_points[1].position;
                let distance = p1.distance(p2);
                self.state.measured_distance = Some(distance);
                log::info!("Measured distance: {}", distance);
            }
        } else {
            log::warn!("No point found under cursor.");
        }
    }

    pub fn handle_event(&mut self, window: &Window, event: &WindowEvent) {
        let _ = self.egui_state.on_window_event(window, event);

        self.state.camera_controller.process_window_events(event);
        if let WindowEvent::CursorMoved { position, .. } = event {
            self.state.latest_cursor_position = *position;
        }

        if !self.egui_state.egui_ctx().is_using_pointer() {

            match self.state.interaction_mode {
                InteractionMode::Camera => {
                    // 在相机模式下，将事件传递给相机控制器
                    if let WindowEvent::CursorMoved { position, .. } = event {
                        if position.x > 530.0 && position.y > 30.0{
                            self.state.camera_controller.process_mouse_move(*position)
                        }
                    }
                    if let WindowEvent::MouseWheel { delta, .. } = event {
                        if self.state.latest_cursor_position.x > 530.0 && self.state.latest_cursor_position.y > 30.0{
                            self.state.camera_controller.process_scroll(delta);
                        }
                    }
                }
                InteractionMode::Measuring => {
                    // 在测量模式下，监听鼠标左键单击
                    if let WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } = event {
                        self.handle_measurement_click();
                    }
                }
            }
        }
    }

    fn check_for_lookup_table(&mut self) {
        // 非阻塞地检查通道中是否有消息
        match self.state.lookup_table_rx.try_recv() {
            Ok(Ok(angle_finder)) => {
                // 加载成功
                self.state.angle_finder = Some(Arc::new(angle_finder));
                self.state.is_loading_lookup_table = false;
                log::info!("查找表异步加载完成...");
            }
            Ok(Err(e)) => {
                // 加载失败
                self.state.is_loading_lookup_table = false;
                log::error!("异步加载查找表失败: {}", e);
            }
            // 通道为空或已关闭，什么都不做
            Err(_) => {}
        }
    }

    fn update_performance_metrics(&mut self) {
        self.state.frame_count_since_last_update += 1;
        let now = std::time::Instant::now();
        let elapsed = (now - self.state.last_perf_update).as_secs_f32();

        // 每秒更新一次
        if elapsed >= 1.0 {
            // 计算 FPS
            self.state.fps = self.state.frame_count_since_last_update as f32 / elapsed;

            // 计算 PPS
            self.state.pps = (self.state.points_processed_since_last_update as f32 / elapsed) as u32;

            // 计算数据速率
            self.state.data_rate_kbs = (self.state.bytes_received_since_last_update as f32 / 1024.0) / elapsed;

            // 重置计数器和计时器
            self.state.frame_count_since_last_update = 0;
            self.state.points_processed_since_last_update = 0;
            self.state.bytes_received_since_last_update = 0;
            self.state.last_perf_update = now;
        }
    }

    fn apply_coloring(&mut self) {
        let mut points_guard = self.state.points.lock().unwrap();
        if points_guard.is_empty() {
            return;
        }

        match self.state.coloring_mode {
            ColoringMode::White => {
                for p in points_guard.iter_mut() {
                    p.color = [1.0, 1.0, 1.0, 1.0];
                }
            }
            ColoringMode::ByHeight => {
                // 首先，找到整个点云的最小和最大Z值
                let (min_z, max_z) = {
                    let mut min = f32::MAX;
                    let mut max = f32::MIN;
                    for p in points_guard.iter() {
                        min = min.min(p.position.z);
                        max = max.max(p.position.z);
                    }
                    (min, max)
                };

                let height_range = max_z - min_z;
                if height_range < 1e-6 { // 避免除以零
                    for p in points_guard.iter_mut() {
                        p.color = gradient_map(0.5);
                    }
                } else {
                    for p in points_guard.iter_mut() {
                        let normalized_height = (p.position.z - min_z) / height_range;
                        p.color = gradient_map(normalized_height);
                    }
                }
            }
        }

        // 更新GPU缓冲区
        self.state.renderer.update_point_cloud(&points_guard);
        log::info!("Recolored point cloud with mode: {:?}", self.state.coloring_mode);
    }

    pub fn update_and_draw(&mut self, window: &Window, window_target: &winit::event_loop::EventLoopWindowTarget<()>) {
        // 检测性能
        self.update_performance_metrics();
        if self.state.coloring_dirty {
            self.apply_coloring();
            self.state.coloring_dirty = false;
        }
        // 阻塞式的检查查找表是否加载完成
        self.check_for_lookup_table();
        // Data Loading and Processing
        self.update_points_from_loader();
        self.update_points_from_network();

        // 绘制UI
        let raw_input = self.egui_state.take_egui_input(window);
        let full_output = self.egui_state.egui_ctx().run(raw_input, |ctx| {
            ui::draw_ui(ctx, &mut self.state);
        });

        self.egui_state.handle_platform_output(window, full_output.platform_output);
        let paint_jobs = self.egui_state.egui_ctx().tessellate(full_output.shapes, window.scale_factor() as f32);

        // Texture Updates for Egui
        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.state.renderer.device, &self.state.renderer.queue, *id, image_delta);
        }
        for id in &full_output.textures_delta.free { self.egui_renderer.free_texture(id); }

        // Prepare for Rendering
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.state.renderer.size.width, self.state.renderer.size.height],
            pixels_per_point: window.scale_factor() as f32,
        };

        let aspect_ratio = self.state.renderer.size.width as f32 / self.state.renderer.size.height as f32;
        let view_proj = self.state.camera_controller.build_view_projection_matrix(aspect_ratio);
        self.state.renderer.update_uniforms(view_proj, self.state.point_size);

        // Final Render Call
        match self.state.renderer.render(&mut self.egui_renderer, &paint_jobs, &screen_descriptor) {
            Ok(_) => {},
            Err(wgpu::SurfaceError::Lost) => self.state.renderer.resize(self.state.renderer.size),
            Err(wgpu::SurfaceError::OutOfMemory) => window_target.exit(),
            Err(e) => log::error!("Render error: {:?}", e),
        }
    }

    fn update_points_from_loader(&mut self) {
        if let Ok(loaded_points) = self.state.point_loader_rx.try_recv() {
            if let Some((min, max)) = calculate_aabb(&loaded_points) {
                self.state.camera_controller.frame_bounding_box(min, max);
            }
            let mut points = self.state.points.lock().unwrap();
            *points = loaded_points;
            self.state.renderer.update_point_cloud(&points);
            self.state.has_centered_on_initial_cloud = true;
            self.state.coloring_dirty = true;
            log::info!("Successfully loaded {} points from file.", points.len());
        }
    }

    fn update_points_from_network(&mut self) {
        // 有新数据的同时，确保查找表在异步线程中加载完成
        if let (Some(rx), Some(finder)) = (self.state.data_rx.as_mut(), self.state.angle_finder.as_ref()) {
            let mut new_points_batch = Vec::new();
            while let Ok(byte_data) = rx.try_recv() {
                self.state.bytes_received_since_last_update += byte_data.len();
                let new_points = processing::bytes_to_points(&byte_data, finder);

                if !new_points.is_empty() {
                    if !self.state.has_centered_on_initial_cloud {
                        if let Some((min, max)) = calculate_aabb(&new_points) {
                            self.state.camera_controller.frame_bounding_box(min, max);
                        }
                        self.state.has_centered_on_initial_cloud = true;
                    }
                    self.state.points_processed_since_last_update += new_points.len();
                    new_points_batch.extend(new_points);
                }

                for chunk in byte_data.chunks_exact(5) {
                    let tof = chunk[4];
                    let dist = f64::from(tof) * 0.375;
                    self.state.waveform_data.push_back([self.state.waveform_counter as f64, dist]);
                    self.state.waveform_counter += 1;
                    if self.state.waveform_data.len() > 512 {
                        self.state.waveform_data.pop_front();
                    }
                    let bin_index = (dist / 5.0).floor() as usize;
                    if bin_index < self.state.histogram_bins.len() {
                        self.state.histogram_bins[bin_index].value += 1.0;
                    }
                }
            }

            if !new_points_batch.is_empty() {
                let mut points_guard = self.state.points.lock().unwrap();
                points_guard.extend(&new_points_batch);
                if self.state.coloring_mode == ColoringMode::ByHeight {
                    self.state.coloring_dirty = true;
                } else {
                    self.state.renderer.update_point_cloud(&points_guard);
                }
            }
        }
    }
}

// Font setup function remains here as it's part of the app's initialization
fn setup_fonts(ctx: &egui::Context) {
    // 创建一个字体定义对象
    let mut fonts = egui::FontDefinitions::default();

    // 加载主字体 (微软雅黑)
    fonts.font_data.insert(
        "my_font".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/msyh.ttc")),
    );

    // 加载Emoji字体作为回退
    fonts.font_data.insert(
        "emoji_font".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/NotoColorEmoji-Regular.ttf")).tweak(
            // 微调 Emoji 字体的大小，让它和中文字体看起来更协调
            egui::FontTweak {
                scale: 1.1, // 让emoji稍微大一点
                ..Default::default()
            },
        ),
    );

    // 设置字体优先级
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "my_font".to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push("emoji_font".to_owned()); // push表示添加到末尾，作为备用

    // 对等宽字体也做同样处理
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "my_font".to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("emoji_font".to_owned());

    // 将配置好的字体加载到egui context中
    ctx.set_fonts(fonts);
}