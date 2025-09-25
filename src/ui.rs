// src/ui.rs

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::Arc;
use egui::{Context, Ui};
use egui_plot::{BarChart, Legend, Line, Plot, PlotPoints};
use tokio::sync::{mpsc, watch};
use crate::app::{AppState, AppTab};
use crate::common::Point;
use crate::network;

pub fn draw_ui(ctx: &Context, state: &mut AppState) {
    draw_control_panel(ctx, state);
    draw_log_panel(ctx, state);
    // This empty CentralPanel is needed to make the other panels work correctly
    egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_ui| {});
}

fn draw_control_panel(ctx: &Context, state: &mut AppState) {
    egui::SidePanel::left("control_panel").min_width(350.0).show(ctx, |ui| {
        ui.heading("控制面板");
        ui.separator();

        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.active_tab, AppTab::Controls, "⚙️ 控制");
            ui.selectable_value(&mut state.active_tab, AppTab::Charts, "📊 图表");
        });
        ui.separator();

        match state.active_tab {
            AppTab::Controls => draw_controls_tab(ui, state),
            AppTab::Charts => draw_charts_tab(ui, state),
        }

        ui.separator();
        ui.heading("📊 状态");
        ui.label(format!("点云数量: {}", state.points.lock().unwrap().len()));
        ui.label(format!("连接状态: {}", if state.is_listening {"已连接"} else {"未连接"}));
    });
}

fn draw_controls_tab(ui: &mut Ui, state: &mut AppState) {
    // 本地监听设置区
    ui.group(|ui| {
        ui.label("本地监听设置");
        ui.add_enabled(!state.is_listening, egui::TextEdit::singleline(&mut state.ip_addr));
        ui.add_enabled(!state.is_listening, egui::TextEdit::singleline(&mut state.port));
    });

    // 目标设备设置区
    ui.group(|ui| {
        ui.label("目标设备设置");
        ui.text_edit_singleline(&mut state.target_ip_addr);
        ui.text_edit_singleline(&mut state.target_port);
    });

    ui.group(|ui| {
        ui.horizontal(|ui| {
            draw_connection_buttons(ui, state);
            draw_measurement_buttons(ui, state);
        });
    });

    ui.collapsing("🛠️ 高级功能", |ui| {
        draw_advanced_functions(ui, state);
    });
}

fn draw_connection_buttons(ui: &mut Ui, state: &mut AppState) {
    if state.is_listening {
        if ui.button("✖️ 断开连接").clicked() {
            if let Some(tx) = state.shutdown_tx.take() {
                let _ = tx.send(true);
            }
            state.is_listening = false;
            state.data_rx = None;
            log::info!("UDP listener has been disconnected.");
        }
    } else {
        if ui.button("🔌 建立连接").clicked() {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let (data_tx, data_rx) = mpsc::channel(100);
            state.shutdown_tx = Some(shutdown_tx);
            state.data_rx = Some(data_rx);
            let ip = state.ip_addr.clone();
            let port = state.port.clone();
           tokio::spawn(async move {
                if let Err(e) = network::run_udp_listener(ip, port, data_tx, shutdown_rx).await {
                    log::error!("UDP listener task failed: {}", e);
                } else {
                    log::info!("UDP listener task finished gracefully.");
                }
            });
            state.is_listening = true;
        }
    }
}

fn draw_measurement_buttons(ui: &mut Ui, state: &mut AppState) {
    ui.add_enabled_ui(state.is_listening, |ui| {
        if ui.button("▶️ 开始测量").clicked() {
            let ip = state.target_ip_addr.clone();
            let port = state.target_port.clone();
            tokio::spawn(async move {
                if let Err(e) = network::send_command(&ip, &port, "5A29").await {
                    log::error!("Failed to send command: {}", e);
                }
            });
        }
    });

    ui.add_enabled_ui(state.is_listening, |ui| {
        if ui.button("🚫 结束测量").clicked() {
            let ip = state.target_ip_addr.clone();
            let port = state.target_port.clone();
            tokio::spawn(async move {
                if let Err(e) = network::send_command(&ip, &port, "5A30").await {
                    log::error!("Failed to send command: {}", e);
                }
            });
        }
    });
}

fn draw_advanced_functions(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        if ui.button("🔄 重置视角").clicked() { state.camera_controller.reset(); }
        if ui.button("🗑️ 清空点云").clicked() {
            state.points.lock().unwrap().clear();
            state.renderer.update_point_cloud(&[]);
            log::info!("Point cloud cleared.");
        }
    });

    ui.horizontal(|ui| {
        if ui.button("💾 保存点云").clicked() {
            let points_clone = Arc::clone(&state.points);
            std::thread::spawn(move || {
                if let Some(path) = rfd::FileDialog::new().add_filter("XYZ Point Cloud", &["xyz"]).save_file() {
                    log::info!("Saving point cloud to: {:?}", path);
                    let points_guard = points_clone.lock().unwrap();
                    if let Ok(file) = File::create(path) {
                        let mut writer = BufWriter::new(file);
                        for p in points_guard.iter() {
                            let _ = writeln!(writer, "{} {} {}", p.position.x, p.position.y, p.position.z);
                        }
                        log::info!("Successfully saved {} points.", points_guard.len());
                    }
                }
            });
        }

        if ui.button("📂 加载点云").clicked() {
            let tx_clone = state.point_loader_tx.clone();
            std::thread::spawn(move || {
                if let Some(path) = rfd::FileDialog::new().add_filter("XYZ Point Cloud", &["xyz"]).pick_file() {
                    log::info!("Loading point cloud from: {:?}", path);
                    if let Ok(file) = File::open(path) {
                        let reader = BufReader::new(file);
                        let loaded_points: Vec<Point> = reader.lines()
                            .filter_map(Result::ok)
                            .filter_map(|line| {
                                let parts: Vec<f32> = line.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                                if parts.len() == 3 { Some(Point::new(parts[0], parts[1], parts[2])) } else { None }
                            })
                            .collect();
                        let _ = tx_clone.blocking_send(loaded_points);
                    }
                }
            });
        }
    });
    ui.add(egui::Slider::new(&mut state.point_size, 0.01..=1.0).text("点云大小"));
}

fn draw_charts_tab(ui: &mut Ui, state: &mut AppState) {
    ui.label("实时距离波形");
    let line = Line::new(PlotPoints::from_iter(state.waveform_data.iter().copied()));
    Plot::new("waveform_plot").height(200.0).view_aspect(2.0).show(ui, |plot_ui| plot_ui.line(line));

    ui.separator();
    ui.label("实时距离直方图");
    let chart = BarChart::new(state.histogram_bins.clone()).color(egui::Color32::LIGHT_GREEN).name("点数");
    Plot::new("histogram_plot").height(200.0).legend(Legend::default()).show(ui, |plot_ui| plot_ui.bar_chart(chart));
}

fn draw_log_panel(ctx: &Context, state: &mut AppState) {
    egui::TopBottomPanel::bottom("log_panel").resizable(true).min_height(100.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("📜 日志");
            if ui.button("🗑️ 清空").clicked() { state.log_buffer.lock().unwrap().clear(); }
        });
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            let log_buffer = state.log_buffer.lock().unwrap();
            for msg in log_buffer.iter() { ui.label(msg); }
        });
    });
}