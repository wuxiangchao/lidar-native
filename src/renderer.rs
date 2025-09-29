use crate::common::Point;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
// Ensure Vec3 is imported
use std::mem::size_of_val;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    position: [f32; 3],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: Mat4, // Use glam::Mat4
    point_size: f32,
    _padding: [f32; 3], // Use glam::Vec3 for padding
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    point_buffer: wgpu::Buffer,
    num_points: u32,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,

    // 坐标轴相关字段
    axis_render_pipeline: wgpu::RenderPipeline,
    axis_vertex_buffer: wgpu::Buffer,
    axis_model_matrix: Mat4,
    axis_model_buffer: wgpu::Buffer,
    axis_bind_group: wgpu::BindGroup,
}

impl Renderer {
    pub fn new(
        window: Arc<Window>,
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Self {
        let size = window.inner_size();
        let surface = instance.create_surface(window).unwrap();

        let surface_caps = surface.get_capabilities(adapter);
        let surface_format = surface_caps.formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let camera_uniform = CameraUniform {
            view_proj: Mat4::IDENTITY,
            point_size: 0.1,
            _padding: [0.0; 3], // Initialize padding
        };

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("camera_bind_group_layout"),
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("camera_bind_group"),
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/shader.wgsl").into()),
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Point::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let point_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Point Buffer"),
            contents: bytemuck::cast_slice(&[Point::new(0.0, 0.0, 0.0)]),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        // 坐标轴渲染管线
        // 定义坐标轴的顶点数据 (6个顶点，3条线)
        // Y轴向上，Z轴向前（右手坐标系）
        const AXIS_LENGTH: f32 = 1.0;
        const THICKNESS: f32 = 0.02; // 坐标轴粗细
        const AXIS_VERTICES: &[LineVertex] = &[
            // X轴 (红色) - 由两个三角形组成的长方形
            LineVertex { position: [0.0, -THICKNESS, -THICKNESS], color: [1.0, 0.2, 0.2] },
            LineVertex { position: [AXIS_LENGTH, -THICKNESS, -THICKNESS], color: [1.0, 0.2, 0.2] },
            LineVertex { position: [AXIS_LENGTH,  THICKNESS, -THICKNESS], color: [1.0, 0.2, 0.2] },
            LineVertex { position: [AXIS_LENGTH,  THICKNESS, -THICKNESS], color: [1.0, 0.2, 0.2] },
            LineVertex { position: [0.0,  THICKNESS, -THICKNESS], color: [1.0, 0.2, 0.2] },
            LineVertex { position: [0.0, -THICKNESS, -THICKNESS], color: [1.0, 0.2, 0.2] },

            // Y轴 (绿色)
            LineVertex { position: [-THICKNESS, 0.0, -THICKNESS], color: [0.2, 1.0, 0.2] },
            LineVertex { position: [-THICKNESS, AXIS_LENGTH, -THICKNESS], color: [0.2, 1.0, 0.2] },
            LineVertex { position: [ THICKNESS, AXIS_LENGTH, -THICKNESS], color: [0.2, 1.0, 0.2] },
            LineVertex { position: [ THICKNESS, AXIS_LENGTH, -THICKNESS], color: [0.2, 1.0, 0.2] },
            LineVertex { position: [ THICKNESS, 0.0, -THICKNESS], color: [0.2, 1.0, 0.2] },
            LineVertex { position: [-THICKNESS, 0.0, -THICKNESS], color: [0.2, 1.0, 0.2] },

            // Z轴 (蓝色)
            LineVertex { position: [-THICKNESS, -THICKNESS, 0.0], color: [0.2, 0.2, 1.0] },
            LineVertex { position: [-THICKNESS, -THICKNESS, AXIS_LENGTH], color: [0.2, 0.2, 1.0] },
            LineVertex { position: [ THICKNESS, -THICKNESS, AXIS_LENGTH], color: [0.2, 0.2, 1.0] },
            LineVertex { position: [ THICKNESS, -THICKNESS, AXIS_LENGTH], color: [0.2, 0.2, 1.0] },
            LineVertex { position: [ THICKNESS, -THICKNESS, 0.0], color: [0.2, 0.2, 1.0] },
            LineVertex { position: [-THICKNESS, -THICKNESS, 0.0], color: [0.2, 0.2, 1.0] },
        ];
        let axis_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Axis Vertex Buffer"),
            contents: bytemuck::cast_slice(AXIS_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // 创建用于坐标轴模型变换的 Uniform Buffer 和 Bind Group
        let axis_model_matrix = Mat4::IDENTITY;
        let axis_model_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Axis Model Buffer"),
            contents: bytemuck::cast_slice(&[axis_model_matrix]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let axis_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("axis_bind_group_layout"),
            });

        let axis_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &axis_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: axis_model_buffer.as_entire_binding(),
            }],
            label: Some("axis_bind_group"),
        });

        // 创建坐标轴渲染管线
        let axis_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Axis Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/axis_shader.wgsl").into()),
        });

        let axis_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Axis Pipeline Layout"),
            // Group 0 for Camera (shared), Group 1 for Model (axis-specific)
            bind_group_layouts: &[&camera_bind_group_layout, &axis_bind_group_layout],
            push_constant_ranges: &[],
        });

        let axis_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Axis Render Pipeline"),
            layout: Some(&axis_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &axis_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<LineVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // Position
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        // Color
                        wgpu::VertexAttribute {
                            offset: size_of::<[f32; 3]>() as wgpu::BufferAddress,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &axis_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList, //
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });



        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            point_buffer,
            num_points: 0,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            // 初始化坐标轴渲染字段
            axis_render_pipeline,
            axis_vertex_buffer,
            axis_model_matrix,
            axis_model_buffer,
            axis_bind_group,
        }
    }

    pub fn update_axis_uniforms(&mut self, center: Vec3, cloud_size: f32) {
        let scale = cloud_size * 0.2; // 让坐标轴的大小与点云尺寸相关联, 0.2是可调系数
        self.axis_model_matrix = Mat4::from_translation(center) * Mat4::from_scale(Vec3::splat(scale.max(0.1)));
        self.queue.write_buffer(
            &self.axis_model_buffer,
            0,
            bytemuck::cast_slice(&[self.axis_model_matrix]),
        );
    }

    pub fn update_uniforms(&mut self, view_proj: Mat4, point_size: f32) {
        self.camera_uniform.view_proj = view_proj;
        self.camera_uniform.point_size = point_size;
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
    }

    pub fn update_point_cloud(&mut self, points: &[Point]) {
        if points.is_empty() {
            self.num_points = 0;
            return;
        }
        let buffer_size = size_of_val(points) as u64;
        if buffer_size > self.point_buffer.size() {
            self.point_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Point Buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.queue
            .write_buffer(&self.point_buffer, 0, bytemuck::cast_slice(points));
        self.num_points = points.len() as u32;
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(
        &mut self,
        egui_renderer: &mut egui_wgpu::Renderer,
        egui_primitives: &[egui::ClippedPrimitive],
        egui_screen_descriptor: &egui_wgpu::ScreenDescriptor,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Render Encoder"),
                });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.point_buffer.slice(..));
            render_pass.draw(0..4, 0..self.num_points);

            if self.num_points > 0 { // 只在有点云时才绘制
                render_pass.set_pipeline(&self.axis_render_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_bind_group(1, &self.axis_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.axis_vertex_buffer.slice(..));
                render_pass.draw(0..18, 0..1);
            }
        }

        {
            egui_renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut encoder,
                egui_primitives,
                egui_screen_descriptor,
            );
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Egui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            egui_renderer.render(&mut render_pass, egui_primitives, egui_screen_descriptor);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}