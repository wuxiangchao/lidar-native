// 您可以将这个函数添加到 app.rs 或者一个新的 processing_extra.rs 文件中

use glam::{Vec3}; // 确保引入Vec3
use crate::common::Point; // 确保引入您的Point结构体
use rand::seq::SliceRandom;

/// 使用RANSAC算法将点云融合到一个主平面上
///
/// # Arguments
/// * `points` - 原始点云切片
/// * `max_iterations` - RANSAC迭代次数
/// * `distance_threshold` - 判断点是否在平面内的距离阈值（单位：米）
///
/// # Returns
/// * `Vec<Point>` - 所有点被投影到最佳拟合平面后的新点云
pub fn fuse_points_to_plane(
    points: &[Point],
    max_iterations: usize,
    distance_threshold: f32,
) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec(); // 点数不足以定义平面，直接返回
    }

    let mut best_plane_normal = Vec3::Y;
    let mut best_plane_point = Vec3::ZERO;
    let mut max_inliers = 0;

    let mut rng = rand::thread_rng();

    // RANSAC寻找最佳平面
    for _ in 0..max_iterations {
        // 随机选择3个不重复的点
        let sample_points: Vec<_> = points.choose_multiple(&mut rng, 3).collect();
        let p1 = sample_points[0].position;
        let p2 = sample_points[1].position;
        let p3 = sample_points[2].position;

        // 从3个点计算出候选平面
        // 法向量 = (p2-p1) x (p3-p1)
        let normal = (p2 - p1).cross(p3 - p1).normalize_or_zero();
        if normal == Vec3::ZERO {
            continue; // 三点共线，跳过当前点
        }
        let point_on_plane = p1;

        // 遍历所有点，统计"局内点"数量
        let mut current_inliers = 0;
        for point in points {
            // 计算点到平面的距离: |(point_pos - point_on_plane) · normal|
            let distance = (point.position - point_on_plane).dot(normal).abs();
            if distance < distance_threshold {
                current_inliers += 1;
            }
        }

        // 如果当前平面更好，则保存它
        if current_inliers > max_inliers {
            max_inliers = current_inliers;
            best_plane_normal = normal;
            best_plane_point = point_on_plane;
        }
    }

    log::info!("RANSAC完成: 找到一个拥有 {} 个局内点的主平面。", max_inliers);

    // 将所有点投影到最佳平面
    let mut fused_points = Vec::with_capacity(points.len());
    for point in points {
        // 计算点到平面的投影点
        // projected_point = point - ((point - plane_point) · plane_normal) * plane_normal
        let v = point.position - best_plane_point;
        let dist = v.dot(best_plane_normal);
        let projected_position = point.position - dist * best_plane_normal;

        fused_points.push(Point {
            position: projected_position,
            color: point.color, // 保留原始颜色
        });
    }
    fused_points
}