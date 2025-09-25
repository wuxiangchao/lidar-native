// src/app.rs

use std::collections::VecDeque;
// use std::fs::File;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use winit::event::WindowEvent;
use winit::window::Window;

use crate::camera::CameraController;
use crate::common::Point;
use crate::processing::{self, AngleFinder};
use crate::renderer::Renderer;
use crate::ui;

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
    pub angle_finder: Arc<AngleFinder>,

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
}

pub struct App {
    pub state: AppState,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

impl App {
    pub async fn new(window: Arc<Window>) -> Self {
        let renderer = Renderer::new(window.clone()).await;
        let camera_controller = CameraController::new();
        let angle_finder =
            Arc::new(AngleFinder::new("data/MEMS_Voltage_9.18-2.xlsx", "Sheet1").unwrap());
        let log_buffer = Arc::new(Mutex::new(Vec::new()));

        crate::logger::init(log_buffer.clone()).unwrap();
        log::info!("Logger initialized. Application starting...");

        let histogram_bins = (0..20)
            .map(|i| egui_plot::Bar::new((i * 5 + 2) as f64, 0.0).width(4.5))
            .collect();

        let (point_loader_tx, point_loader_rx) = mpsc::channel(1);

        let egui_state = egui_winit::State::new(egui::Context::default(), egui::ViewportId::default(), &window, None, None);
        setup_fonts(egui_state.egui_ctx());

        let egui_renderer = egui_wgpu::Renderer::new(&renderer.device, renderer.config.format, None, 1);

        let state = AppState {
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
            active_tab: AppTab::Controls,
            waveform_data: VecDeque::with_capacity(512),
            waveform_counter: 0,
            histogram_bins,
            point_loader_rx,
            point_loader_tx,
        };

        Self { state, egui_state, egui_renderer }
    }

    pub fn handle_event(&mut self, window: &Window, event: &WindowEvent) {
        let _ = self.egui_state.on_window_event(window, event);
        self.state.camera_controller.process_window_events(event);
        if !self.egui_state.egui_ctx().is_using_pointer() {
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    self.state.camera_controller.process_mouse_move(*position);
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    self.state.camera_controller.process_scroll(delta);
                }
                _ => {}
            }
        }
    }

    pub fn update_and_draw(&mut self, window: &Window, window_target: &winit::event_loop::EventLoopWindowTarget<()>) {
        // Data Loading and Processing
        self.update_points_from_loader();
        self.update_points_from_network();

        // UI Drawing
        let raw_input = self.egui_state.take_egui_input(&window);
        let full_output = self.egui_state.egui_ctx().run(raw_input, |ctx| {
            ui::draw_ui(ctx, &mut self.state);
        });

        self.egui_state.handle_platform_output(&window, full_output.platform_output);
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
            let mut points = self.state.points.lock().unwrap();
            *points = loaded_points;
            self.state.renderer.update_point_cloud(&points);
            log::info!("Successfully loaded {} points from file.", points.len());
        }
    }

    fn update_points_from_network(&mut self) {
        if let Some(rx) = self.state.data_rx.as_mut() {
            while let Ok(byte_data) = rx.try_recv() {
                // Update 3D point cloud
                let new_points = processing::bytes_to_points(&byte_data, &self.state.angle_finder);
                if !new_points.is_empty() {
                    let mut points = self.state.points.lock().unwrap();
                    points.extend(&new_points);
                    self.state.renderer.update_point_cloud(&points);
                }

                // Update chart data
                for chunk in byte_data.chunks_exact(5) {
                    let tof = chunk[4];
                    let dist = f64::from(tof) * 0.375;
                    self.state.waveform_data.push_back([self.state.waveform_counter as f64, dist]);
                    self.state.waveform_counter += 1;
                    if self.state.waveform_data.len() > 512 { self.state.waveform_data.pop_front(); }

                    let bin_index = (dist / 5.0).floor() as usize;
                    if bin_index < self.state.histogram_bins.len() {
                        self.state.histogram_bins[bin_index].value += 1.0;
                    }
                }
            }
        }
    }
}

// Font setup function remains here as it's part of the app's initialization
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_path = "C:/Windows/Fonts/msyh.ttc";
    if let Ok(font_bytes) = std::fs::read(font_path) {
        fonts.font_data.insert("my_font".to_owned(), egui::FontData::from_owned(font_bytes));
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "my_font".to_owned());
        }
        ctx.set_fonts(fonts);
        log::info!("Chinese font setup complete.");
    } else {
        log::error!("Failed to load font: {}. Chinese characters may not display correctly.", font_path);
    }
}