// src/processing.rs (Final Fix - Parsing by Column Name)

use anyhow::{anyhow, Context, Result};
use calamine::{open_workbook, Reader, Xlsx};
use rayon::prelude::*;
use std::collections::HashMap;

use crate::common::Point;

pub struct AngleFinder {
    v_u4_col: Vec<f64>,
    h_angle_col: Vec<f64>,
    v_angle_col: Vec<f64>,
    known_v_u3s: Vec<f64>,
    lookup_indices: HashMap<i64, (usize, usize)>,
}

impl AngleFinder {
    pub fn new(file_path: &str, sheet_name: &str) -> Result<Self> {
        log::info!("正在后台异步加载查找表...");
        let mut workbook: Xlsx<_> = open_workbook(file_path)
            .with_context(|| format!("无法打开文件: {}", file_path))?;

        let range = match workbook.worksheet_range(sheet_name) {
            Ok(r) => r,
            Err(e) => return Err(e.into()),
        };

        let mut rows = range.rows();

        let header = rows.next().ok_or_else(|| anyhow!("Excel文件为空或没有表头"))?;

        let find_col = |name: &str| -> Result<usize> {
            header
                .iter()
                .position(|cell| cell.get_string().is_some_and(|s| s.trim() == name))
                .ok_or_else(|| anyhow!("在Excel文件中找不到名为 '{}' 的列", name))
        };

        let v3_idx = find_col("V_U3")?;
        let v4_idx = find_col("V_U4")?;
        let h_angle_idx = find_col("水平偏转角")?;
        let v_angle_idx = find_col("垂直偏转角")?;

        let mut raw_data: Vec<(f64, f64, f64, f64)> = rows
            .filter_map(|row| {
                // Use the found indices to read data, ensuring correctness
                if let (Some(v_u3), Some(v_u4), Some(h_angle), Some(v_angle)) = (
                    row.get(v3_idx).and_then(|c| c.get_float()),
                    row.get(v4_idx).and_then(|c| c.get_float()),
                    row.get(h_angle_idx).and_then(|c| c.get_float()),
                    row.get(v_angle_idx).and_then(|c| c.get_float()),
                ) {
                    Some((v_u3, v_u4, h_angle, v_angle))
                } else {
                    None
                }
            })
            .collect();

        raw_data.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap()
                .then_with(|| a.1.partial_cmp(&b.1).unwrap())
        });

        let mut lookup_indices = HashMap::new();
        let mut known_v_u3s = Vec::new();
        if !raw_data.is_empty() {
            let mut last_key: Option<i64> = None;
            let mut group_start_index = 0;
            for (i, &(v3, _, _, _)) in raw_data.iter().enumerate() {
                let current_key = (v3 * 1000.0).round() as i64;
                if last_key.is_none() {
                    last_key = Some(current_key);
                    known_v_u3s.push(v3);
                }
                if Some(current_key) != last_key {
                    if let Some(key) = last_key {
                        lookup_indices.insert(key, (group_start_index, i));
                    }
                    group_start_index = i;
                    last_key = Some(current_key);
                    known_v_u3s.push(v3);
                }
            }
            if let Some(key) = last_key {
                lookup_indices.insert(key, (group_start_index, raw_data.len()));
            }
        }

        let v_u4_col: Vec<f64> = raw_data.iter().map(|(_, v4, _, _)| *v4).collect();
        let h_angle_col: Vec<f64> = raw_data.iter().map(|(_, _, ha, _)| *ha).collect();
        let v_angle_col: Vec<f64> = raw_data.iter().map(|(_, _, _, va)| *va).collect();

        log::info!("查找表数据结构准备完成...");

        Ok(Self {
            v_u4_col,
            h_angle_col,
            v_angle_col,
            known_v_u3s,
            lookup_indices,
        })
    }

    // 
    fn find_in_slice_with_interp(&self, v_u4_target: f64, start: usize, end: usize) -> Option<(f64, f64)> {
        let v_u4_slice = &self.v_u4_col[start..end];
        let h_angle_slice = &self.h_angle_col[start..end];
        let v_angle_slice = &self.v_angle_col[start..end];
        match v_u4_slice.binary_search_by(|v| v.partial_cmp(&v_u4_target).unwrap()) {
            Ok(idx) => Some((h_angle_slice[idx], v_angle_slice[idx])),
            Err(idx) => {
                if idx == 0 || idx >= v_u4_slice.len() {
                    return None;
                }
                let (x1, x2) = (v_u4_slice[idx - 1], v_u4_slice[idx]);
                let (yh1, yh2) = (h_angle_slice[idx - 1], h_angle_slice[idx]);
                let (yv1, yv2) = (v_angle_slice[idx - 1], v_angle_slice[idx]);
                let ratio = (v_u4_target - x1) / (x2 - x1);
                let h_angle = yh1 + ratio * (yh2 - yh1);
                let v_angle = yv1 + ratio * (yv2 - yv1);
                Some((h_angle, v_angle))
            }
        }
    }

    fn find_single_angle(&self, v_u3_target: f64, v_u4_target: f64) -> Option<(f64, f64)> {
        match self.known_v_u3s.binary_search_by(|v| v.partial_cmp(&v_u3_target).unwrap()) {
            Ok(idx) => {
                let v_u3_val = self.known_v_u3s[idx];
                let key = (v_u3_val * 1000.0).round() as i64;
                if let Some(&(start, end)) = self.lookup_indices.get(&key) {
                    self.find_in_slice_with_interp(v_u4_target, start, end)
                } else {
                    None
                }
            }
            Err(idx) => {
                if idx == 0 || idx >= self.known_v_u3s.len() {
                    return None;
                }
                let v_u3_lower = self.known_v_u3s[idx - 1];
                let v_u3_upper = self.known_v_u3s[idx];
                let key_lower = (v_u3_lower * 1000.0).round() as i64;
                let &(start_l, end_l) = self.lookup_indices.get(&key_lower)?;
                let (h1, v1) = self.find_in_slice_with_interp(v_u4_target, start_l, end_l)?;
                let key_upper = (v_u3_upper * 1000.0).round() as i64;
                let &(start_u, end_u) = self.lookup_indices.get(&key_upper)?;
                let (h2, v2) = self.find_in_slice_with_interp(v_u4_target, start_u, end_u)?;
                let ratio = (v_u3_target - v_u3_lower) / (v_u3_upper - v_u3_lower);
                let h_angle = h1 + ratio * (h2 - h1);
                let v_angle = v1 + ratio * (v2 - v1);
                Some((h_angle, v_angle))
            }
        }
    }
}

pub fn bytes_to_points(bytes: &[u8], finder: &AngleFinder) -> Vec<Point> {
    const PACKET_SIZE: usize = 8;
    const HEADER_BYTE: u8 = 0x7E; // 包头
    let points: Vec<Point> = bytes
        .par_windows(PACKET_SIZE) // 创建一个滑动窗口，大小为8字节
        .filter(|window| window[0] == HEADER_BYTE) // 只保留以包头开始的窗口
        .filter_map(|chunk| { // 解析有效的数据块
            let up_16bit = u16::from_be_bytes([chunk[1], chunk[2]]);
            let left_16bit = u16::from_be_bytes([chunk[3], chunk[4]]);
            let tof = chunk[5] as f32;
            let tof_s = chunk[6] as f32;
            let tof_e = chunk[7] as f32;

            let fine_tune = tof_s - tof_e;
            let dist = (tof * 3.571_f32 * 1000.0_f32 + fine_tune * 98.0_f32) * 3.0_f32 / 20000.0_f32;
            // let dist = tof * 3.571_f32 * 1000.0_f32 * 3.0_f32 / 20000.0_f32;
            let v_u3 = (f64::from(left_16bit) / 65536.0) * 10.0;
            // v_u3 = (v_u3 * 10.0).round() / 10.0;

            let v_u4 = (f64::from(up_16bit) / 65536.0) * 10.0;
            // v_u4 = (v_u4 * 1000.0).trunc() / 1000.0;

            // 调用查找表并创建点
            finder
                .find_single_angle(v_u3, v_u4)
                .map(|(h_angle, v_angle)| {
                    // log::info!("v_u3: {:.3}, v_u4: {:.3} -> h_angle: {:.3}, v_angle: {:.3}", v_u3, v_u4, h_angle, v_angle);
                    let h_rad = (h_angle as f32).to_radians();
                    let v_rad = (v_angle as f32).to_radians();
                    let (sin_v, cos_v) = v_rad.sin_cos();
                    let (sin_h, cos_h) = h_rad.sin_cos();
                    let dy = dist * sin_v;
                    let edge = dist * cos_v;
                    let dz = edge * cos_h;
                    let dx = edge * sin_h;
                    Point::new(dx, -dy, dz)
                })
        })
        .collect();

    points
}