// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Device wgpu, renderizado de mallas 3D y pase de presentación estéreo.
//!
//! Este módulo implementa el pipeline de renderizado del motor:
//! - Negocia adaptadores, features y gestiona superficies wgpu.
//! - Soporta buffer de profundidad (Depth32Float) para ordenamiento 3D real.
//! - Renderiza mallas 3D deterministas con iluminación difusa, especular y niebla espacial.
//! - Conecta páginas de geometría transmitidas por la isla C++ (`TLXM`).
//! - Compone presentación mono y estéreo side-by-side (`--stereo`) con proyección por ojo.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::config::Config;
use crate::xr::{Eye, StereoFrame};

/// Shader único del preview estéreo / fondo de confort.
pub const STEREO_WGSL: &str = include_str!("../../../shaders/stereo.wgsl");

/// Shader del pipeline de renderizado de mallas 3D.
pub const SCENE3D_WGSL: &str = include_str!("../../../shaders/scene3d.wgsl");

/// Magic identificador para páginas de geometría con formato de malla (`"TLXM"`).
pub const MESH_PAGE_MAGIC: u32 = 0x544C_584D;

const UPLOAD_RING_BYTES: u64 = 32 << 20; // 32 MiB

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    resolution: [f32; 2],
    time: f32,
    eye: f32,
    ipd: f32,
    fov_y: f32,
    is_stereo: f32,
    _pad: f32,
}

const _: () = assert!(std::mem::size_of::<Globals>() == 32);

/// Uniforms de escena 3D pasados al shader de mallas por ojo.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SceneUniforms {
    pub view_proj: [f32; 16],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
}

/// Vértice estándar para mallas 3D con normales y color por vértice.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

/// Cabecera empaquetada en páginas de geometría de mallas transmitidas.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MeshPageHeader {
    pub magic: u32,
    pub vertex_count: u32,
    pub index_count: u32,
    pub _pad: u32,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub depth_view: wgpu::TextureView,
    depth_texture: wgpu::Texture,
}

impl Gpu {
    pub async fn new(window: Arc<Window>, cfg: &Config) -> Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window)
            .context("no se pudo crear la surface")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("no hay GPU compatible (¿drivers?)")?;

        let available = adapter.features();
        let mut required = wgpu::Features::empty();
        if available.contains(wgpu::Features::TIMESTAMP_QUERY) {
            required |= wgpu::Features::TIMESTAMP_QUERY;
        }

        let adapter_info = adapter.get_info();
        tracing_init(&adapter_info);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("tlaxtrelom.device"),
                    required_features: required,
                    required_limits: adapter.limits(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .context("request_device falló")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .context("la surface no expone ningún formato de textura")?;

        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: if cfg.mode.is_stereo() { 1 } else { 2 },
        };
        surface.configure(&device, &config);

        let (depth_texture, depth_view) = create_depth_texture(&device, width, height);

        Ok(Self {
            device,
            queue,
            surface,
            config,
            depth_view,
            depth_texture,
        })
    }

    /// Devuelve `true` si hubo reconfiguración real.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        if width == self.config.width && height == self.config.height {
            return false;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);

        let (depth_texture, depth_view) = create_depth_texture(&self.device, width, height);
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
        true
    }

    /// Recuperación idempotente tras `Lost`/`Outdated`.
    pub fn recover(&mut self) {
        self.surface.configure(&self.device, &self.config);
        let (depth_texture, depth_view) =
            create_depth_texture(&self.device, self.config.width, self.config.height);
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
    }
}

fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tlaxtrelom.depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn tracing_init(info: &wgpu::AdapterInfo) {
    eprintln!(
        "[gpu] {} ({:?}, {:?}) driver={}",
        info.name, info.backend, info.device_type, info.driver
    );
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    pipeline_3d: wgpu::RenderPipeline,
    bind_groups: [wgpu::BindGroup; 2],
    bind_groups_3d: [wgpu::BindGroup; 2],
    globals: [wgpu::Buffer; 2],
    scene_buffers: [wgpu::Buffer; 2],
    scene_vertex_buffer: wgpu::Buffer,
    scene_index_buffer: wgpu::Buffer,
    scene_index_count: u32,
    streamed_vertex_buffer: wgpu::Buffer,
    streamed_index_buffer: wgpu::Buffer,
    streamed_index_count: u32,
    upload: wgpu::Buffer,
    upload_offset: u64,
    upload_scratch: Vec<u8>,
    uploaded_bytes: u64,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // ── 1. Pipeline de presentación y fondo estéreo ──────────────────────
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tlaxtrelom.stereo"),
            source: wgpu::ShaderSource::Wgsl(STEREO_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tlaxtrelom.globals.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let make_uniform = |label: &'static str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let globals = [
            make_uniform("tlaxtrelom.globals.left"),
            make_uniform("tlaxtrelom.globals.right"),
        ];

        let make_bind = |label: &'static str, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let bind_groups = [
            make_bind("tlaxtrelom.bg.left", &globals[0]),
            make_bind("tlaxtrelom.bg.right", &globals[1]),
        ];

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tlaxtrelom.pipeline.layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tlaxtrelom.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── 2. Pipeline de renderizado de mallas 3D ──────────────────────────
        let shader_3d = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tlaxtrelom.scene3d"),
            source: wgpu::ShaderSource::Wgsl(SCENE3D_WGSL.into()),
        });

        let bgl_3d = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tlaxtrelom.scene3d.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let make_scene_buf = |label: &'static str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<SceneUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let scene_buffers = [
            make_scene_buf("tlaxtrelom.scene.left"),
            make_scene_buf("tlaxtrelom.scene.right"),
        ];

        let bind_groups_3d = [
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tlaxtrelom.scene_bg.left"),
                layout: &bgl_3d,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene_buffers[0].as_entire_binding(),
                }],
            }),
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tlaxtrelom.scene_bg.right"),
                layout: &bgl_3d,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene_buffers[1].as_entire_binding(),
                }],
            }),
        ];

        let layout_3d = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tlaxtrelom.pipeline_3d.layout"),
            bind_group_layouts: &[&bgl_3d],
            push_constant_ranges: &[],
        });

        let vertex_layout_3d = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Normal
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        };

        let pipeline_3d = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tlaxtrelom.pipeline_3d"),
            layout: Some(&layout_3d),
            vertex: wgpu::VertexState {
                module: &shader_3d,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout_3d],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_3d,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── 3. Construcción de la escena 3D base y buffers de mallas ──────────
        let (scene_vertices, scene_indices) = create_demo_scene();
        let scene_index_count = scene_indices.len() as u32;

        let scene_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tlaxtrelom.scene.vertices"),
            contents: bytemuck::cast_slice(&scene_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let scene_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tlaxtrelom.scene.indices"),
            contents: bytemuck::cast_slice(&scene_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Buffers para geometría dinámica transmitida desde la isla C++
        let streamed_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.streamed.vertices"),
            size: 65536 * std::mem::size_of::<MeshVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let streamed_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.streamed.indices"),
            size: 131072 * std::mem::size_of::<u16>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ── 4. Ring buffer de subida de geometría ─────────────────────────────
        let upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.upload.ring"),
            size: UPLOAD_RING_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            pipeline_3d,
            bind_groups,
            bind_groups_3d,
            globals,
            scene_buffers,
            scene_vertex_buffer,
            scene_index_buffer,
            scene_index_count,
            streamed_vertex_buffer,
            streamed_index_buffer,
            streamed_index_count: 0,
            upload,
            upload_offset: 0,
            upload_scratch: Vec::with_capacity(64 * 1024),
            uploaded_bytes: 0,
        }
    }

    pub fn update(
        &self,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        time: f32,
        frame: &StereoFrame,
    ) {
        let resolution = [width.max(1) as f32, height.max(1) as f32];
        let fov = if frame.left.view_proj[5].is_finite() && frame.left.view_proj[5] > 1e-6 {
            2.0 * (1.0 / frame.left.view_proj[5]).atan()
        } else {
            90.0f32.to_radians()
        };

        let light_dir = [0.4f32, 0.8f32, 0.5f32, 0.0f32];

        if frame.stereo {
            write_globals(
                queue,
                &self.globals[0],
                Globals {
                    resolution,
                    time,
                    eye: Eye::Left.shader_index(),
                    ipd: frame.ipd_m,
                    fov_y: fov,
                    is_stereo: 1.0,
                    _pad: 0.0,
                },
            );
            write_globals(
                queue,
                &self.globals[1],
                Globals {
                    resolution,
                    time,
                    eye: Eye::Right.shader_index(),
                    ipd: frame.ipd_m,
                    fov_y: fov,
                    is_stereo: 1.0,
                    _pad: 0.0,
                },
            );

            let left_scene = SceneUniforms {
                view_proj: frame.left.view_proj,
                camera_pos: [
                    frame.left.position[0],
                    frame.left.position[1],
                    frame.left.position[2],
                    time,
                ],
                light_dir: [light_dir[0], light_dir[1], light_dir[2], 0.0],
            };
            let right_scene = SceneUniforms {
                view_proj: frame.right.view_proj,
                camera_pos: [
                    frame.right.position[0],
                    frame.right.position[1],
                    frame.right.position[2],
                    time,
                ],
                light_dir: [light_dir[0], light_dir[1], light_dir[2], 1.0],
            };
            queue.write_buffer(&self.scene_buffers[0], 0, bytemuck::bytes_of(&left_scene));
            queue.write_buffer(&self.scene_buffers[1], 0, bytemuck::bytes_of(&right_scene));
        } else {
            let mono = Globals {
                resolution,
                time,
                eye: Eye::Mono.shader_index(),
                ipd: 0.0,
                fov_y: fov,
                is_stereo: 0.0,
                _pad: 0.0,
            };
            write_globals(queue, &self.globals[0], mono);
            write_globals(queue, &self.globals[1], mono);

            let mono_scene = SceneUniforms {
                view_proj: frame.left.view_proj,
                camera_pos: [
                    frame.left.position[0],
                    frame.left.position[1],
                    frame.left.position[2],
                    time,
                ],
                light_dir: [light_dir[0], light_dir[1], light_dir[2], 2.0],
            };
            queue.write_buffer(&self.scene_buffers[0], 0, bytemuck::bytes_of(&mono_scene));
            queue.write_buffer(&self.scene_buffers[1], 0, bytemuck::bytes_of(&mono_scene));
        }
    }

    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        stereo: bool,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tlaxtrelom.present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.008,
                        g: 0.010,
                        b: 0.016,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let half = width / 2;
        if stereo {
            // ── Ojo izquierdo ──
            pass.set_scissor_rect(0, 0, half.max(1), height.max(1));
            // Fondo / confort
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[0], &[]);
            pass.draw(0..3, 0..1);
            // Mallas 3D
            pass.set_pipeline(&self.pipeline_3d);
            pass.set_bind_group(0, &self.bind_groups_3d[0], &[]);
            pass.set_vertex_buffer(0, self.scene_vertex_buffer.slice(..));
            pass.set_index_buffer(self.scene_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.scene_index_count, 0, 0..1);
            if self.streamed_index_count > 0 {
                pass.set_vertex_buffer(0, self.streamed_vertex_buffer.slice(..));
                pass.set_index_buffer(
                    self.streamed_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                pass.draw_indexed(0..self.streamed_index_count, 0, 0..1);
            }

            // ── Ojo derecho ──
            pass.set_scissor_rect(half, 0, half.max(1), height.max(1));
            // Fondo / confort
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[1], &[]);
            pass.draw(0..3, 0..1);
            // Mallas 3D
            pass.set_pipeline(&self.pipeline_3d);
            pass.set_bind_group(0, &self.bind_groups_3d[1], &[]);
            pass.set_vertex_buffer(0, self.scene_vertex_buffer.slice(..));
            pass.set_index_buffer(self.scene_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.scene_index_count, 0, 0..1);
            if self.streamed_index_count > 0 {
                pass.set_vertex_buffer(0, self.streamed_vertex_buffer.slice(..));
                pass.set_index_buffer(
                    self.streamed_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                pass.draw_indexed(0..self.streamed_index_count, 0, 0..1);
            }
        } else {
            // ── Mono ──
            pass.set_scissor_rect(0, 0, width.max(1), height.max(1));
            // Fondo
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[0], &[]);
            pass.draw(0..3, 0..1);
            // Mallas 3D
            pass.set_pipeline(&self.pipeline_3d);
            pass.set_bind_group(0, &self.bind_groups_3d[0], &[]);
            pass.set_vertex_buffer(0, self.scene_vertex_buffer.slice(..));
            pass.set_index_buffer(self.scene_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.scene_index_count, 0, 0..1);
            if self.streamed_index_count > 0 {
                pass.set_vertex_buffer(0, self.streamed_vertex_buffer.slice(..));
                pass.set_index_buffer(
                    self.streamed_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                pass.draw_indexed(0..self.streamed_index_count, 0, 0..1);
            }
        }
    }

    /// Sube bytes de una página de geometría al ring de staging.
    /// Si los datos corresponden a un paquete de malla virtual (`TLXM`), se
    /// actualiza el buffer GPU de geometría transmitida.
    pub fn upload_page(&mut self, queue: &wgpu::Queue, data: &[u8]) -> usize {
        if data.is_empty() {
            return 0;
        }
        let aligned = (data.len() + 3) & !3;
        if aligned as u64 > UPLOAD_RING_BYTES {
            return 0;
        }
        if self.upload_offset + aligned as u64 > UPLOAD_RING_BYTES {
            self.upload_offset = 0;
        }

        self.upload_scratch.clear();
        self.upload_scratch.extend_from_slice(data);
        self.upload_scratch.resize(aligned, 0);

        queue.write_buffer(&self.upload, self.upload_offset, &self.upload_scratch);
        self.upload_offset += aligned as u64;
        self.uploaded_bytes = self.uploaded_bytes.saturating_add(data.len() as u64);

        // Detección de mallas virtuales `TLXM` transmitidas desde la isla de geometría
        if data.len() >= std::mem::size_of::<MeshPageHeader>() {
            let header: MeshPageHeader =
                bytemuck::cast_slice(&data[..std::mem::size_of::<MeshPageHeader>()])[0];
            if header.magic == MESH_PAGE_MAGIC {
                let v_start = std::mem::size_of::<MeshPageHeader>();
                let v_len = header.vertex_count as usize * std::mem::size_of::<MeshVertex>();
                let i_start = v_start + v_len;
                let i_len = header.index_count as usize * std::mem::size_of::<u16>();
                if data.len() >= i_start + i_len {
                    let v_bytes = &data[v_start..v_start + v_len];
                    let i_bytes = &data[i_start..i_start + i_len];
                    queue.write_buffer(&self.streamed_vertex_buffer, 0, v_bytes);
                    queue.write_buffer(&self.streamed_index_buffer, 0, i_bytes);
                    self.streamed_index_count = header.index_count;
                }
            }
        }

        data.len()
    }

    pub fn uploaded_bytes(&self) -> u64 {
        self.uploaded_bytes
    }
}

fn write_globals(queue: &wgpu::Queue, buffer: &wgpu::Buffer, globals: Globals) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(&globals));
}

/// Construye la escena 3D procedural de demostración con suelo, monolito, cubos y pirámide.
fn create_demo_scene() -> (Vec<MeshVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // 1. Suelo / Rejilla espacial en Y = 0.0
    let step = 4.0f32;
    let min_x = -24.0f32;
    let max_x = 24.0f32;
    let min_z = -8.0f32;
    let max_z = 36.0f32;

    let mut z = min_z;
    while z < max_z {
        let mut x = min_x;
        while x < max_x {
            let is_runway = x.abs() < 2.0;
            let checker = ((x / step).floor() as i32 + (z / step).floor() as i32) % 2 == 0;
            let color = if is_runway {
                [0.10, 0.45, 0.55]
            } else if checker {
                [0.05, 0.08, 0.14]
            } else {
                [0.03, 0.05, 0.09]
            };

            let base = vertices.len() as u16;
            let norm = [0.0, 1.0, 0.0];
            vertices.push(MeshVertex {
                position: [x, 0.0, z],
                normal: norm,
                color,
            });
            vertices.push(MeshVertex {
                position: [x + step, 0.0, z],
                normal: norm,
                color,
            });
            vertices.push(MeshVertex {
                position: [x + step, 0.0, z + step],
                normal: norm,
                color,
            });
            vertices.push(MeshVertex {
                position: [x, 0.0, z + step],
                normal: norm,
                color,
            });

            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            x += step;
        }
        z += step;
    }

    // 2. Monolito central de datos a 8 metros
    push_box(
        &mut vertices,
        &mut indices,
        [0.0, 2.0, 8.0],
        [0.9, 4.0, 0.9],
        [0.08, 0.65, 0.85],
    );

    // 3. Cubo flotante naranja a la izquierda (3.5 m a la izquierda, 6 m adelante)
    push_box(
        &mut vertices,
        &mut indices,
        [-3.5, 1.6, 6.0],
        [1.2, 1.2, 1.2],
        [0.95, 0.45, 0.12],
    );

    // 4. Cubo flotante magenta a la derecha (3.5 m a la derecha, 6 m adelante)
    push_box(
        &mut vertices,
        &mut indices,
        [3.5, 1.6, 6.0],
        [1.2, 1.2, 1.2],
        [0.85, 0.22, 0.88],
    );

    // 5. Pilares guía a los costados del camino
    for z_pos in [4.0, 10.0, 16.0, 22.0] {
        push_box(
            &mut vertices,
            &mut indices,
            [-2.2, 0.8, z_pos],
            [0.4, 1.6, 0.4],
            [0.25, 0.70, 0.50],
        );
        push_box(
            &mut vertices,
            &mut indices,
            [2.2, 0.8, z_pos],
            [0.4, 1.6, 0.4],
            [0.25, 0.70, 0.50],
        );
    }

    // 6. Pirámide al fondo (18 m adelante)
    let pyr_cx = 0.0f32;
    let pyr_cy = 0.0f32;
    let pyr_cz = 18.0f32;
    let pyr_h = 3.5f32;
    let pyr_b = 2.0f32;
    let tip = [pyr_cx, pyr_cy + pyr_h, pyr_cz];
    let p0 = [pyr_cx - pyr_b, pyr_cy, pyr_cz - pyr_b];
    let p1 = [pyr_cx + pyr_b, pyr_cy, pyr_cz - pyr_b];
    let p2 = [pyr_cx + pyr_b, pyr_cy, pyr_cz + pyr_b];
    let p3 = [pyr_cx - pyr_b, pyr_cy, pyr_cz + pyr_b];
    let pyr_color = [0.15, 0.85, 0.40];

    push_tri(&mut vertices, &mut indices, tip, p1, p0, pyr_color);
    push_tri(&mut vertices, &mut indices, tip, p2, p1, pyr_color);
    push_tri(&mut vertices, &mut indices, tip, p3, p2, pyr_color);
    push_tri(&mut vertices, &mut indices, tip, p0, p3, pyr_color);

    // 7. Ejes de orientación espacial cerca del punto de inicio
    push_box(
        &mut vertices,
        &mut indices,
        [0.5, 0.1, 1.2],
        [0.8, 0.08, 0.08],
        [1.0, 0.15, 0.15],
    ); // Eje +X (Rojo)
    push_box(
        &mut vertices,
        &mut indices,
        [0.1, 0.5, 1.2],
        [0.08, 0.8, 0.08],
        [0.15, 1.0, 0.15],
    ); // Eje +Y (Verde)
    push_box(
        &mut vertices,
        &mut indices,
        [0.1, 0.1, 1.6],
        [0.08, 0.08, 0.8],
        [0.20, 0.45, 1.0],
    ); // Eje +Z (Azul / Frente)

    (vertices, indices)
}

fn push_box(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    center: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
) {
    let hx = size[0] * 0.5;
    let hy = size[1] * 0.5;
    let hz = size[2] * 0.5;
    let cx = center[0];
    let cy = center[1];
    let cz = center[2];

    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [cx - hx, cy - hy, cz + hz],
                [cx + hx, cy - hy, cz + hz],
                [cx + hx, cy + hy, cz + hz],
                [cx - hx, cy + hy, cz + hz],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [cx + hx, cy - hy, cz - hz],
                [cx - hx, cy - hy, cz - hz],
                [cx - hx, cy + hy, cz - hz],
                [cx + hx, cy + hy, cz - hz],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [cx + hx, cy - hy, cz + hz],
                [cx + hx, cy - hy, cz - hz],
                [cx + hx, cy + hy, cz - hz],
                [cx + hx, cy + hy, cz + hz],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [cx - hx, cy - hy, cz - hz],
                [cx - hx, cy - hy, cz + hz],
                [cx - hx, cy + hy, cz + hz],
                [cx - hx, cy + hy, cz - hz],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [cx - hx, cy + hy, cz + hz],
                [cx + hx, cy + hy, cz + hz],
                [cx + hx, cy + hy, cz - hz],
                [cx - hx, cy + hy, cz - hz],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [cx - hx, cy - hy, cz - hz],
                [cx + hx, cy - hy, cz - hz],
                [cx + hx, cy - hy, cz + hz],
                [cx - hx, cy - hy, cz + hz],
            ],
        ),
    ];

    for (norm, pts) in faces {
        let base = vertices.len() as u16;
        for pt in pts {
            vertices.push(MeshVertex {
                position: pt,
                normal: norm,
                color,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn push_tri(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    color: [f32; 3],
) {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let len = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2])
        .sqrt()
        .max(1e-6);
    let norm = [cross[0] / len, cross[1] / len, cross[2] / len];
    let base = vertices.len() as u16;
    vertices.push(MeshVertex {
        position: a,
        normal: norm,
        color,
    });
    vertices.push(MeshVertex {
        position: b,
        normal: norm,
        color,
    });
    vertices.push(MeshVertex {
        position: c,
        normal: norm,
        color,
    });
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

/// Serializa vértices e índices en un paquete binario de página de geometría virtual `TLXM`.
pub fn create_mesh_page(vertices: &[MeshVertex], indices: &[u16]) -> Vec<u8> {
    let header = MeshPageHeader {
        magic: MESH_PAGE_MAGIC,
        vertex_count: vertices.len() as u32,
        index_count: indices.len() as u32,
        _pad: 0,
    };
    let mut out = Vec::with_capacity(
        std::mem::size_of::<MeshPageHeader>()
            + std::mem::size_of_val(vertices)
            + std::mem::size_of_val(indices),
    );
    out.extend_from_slice(bytemuck::bytes_of(&header));
    out.extend_from_slice(bytemuck::cast_slice(vertices));
    out.extend_from_slice(bytemuck::cast_slice(indices));
    out
}

/// Enumera adaptadores sin crear una ventana. Lo usa `tlaxtrelom --probe` para
/// diagnosticar drivers en máquinas de CI o en un headset antes de abrir XR.
pub fn probe() -> Result<Vec<String>> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapters = instance.enumerate_adapters(wgpu::Backends::all());
    if adapters.is_empty() {
        bail!("wgpu no encontró ningún adaptador (¿drivers Vulkan/DX12/Metal?)");
    }

    let mut out = Vec::with_capacity(adapters.len());
    for adapter in adapters {
        let info = adapter.get_info();
        out.push(format!(
            "{} | backend={:?} | tipo={:?} | driver={} {} | vendor=0x{:04x} device=0x{:04x}",
            info.name,
            info.backend,
            info.device_type,
            info.driver,
            info.driver_info,
            info.vendor,
            info.device
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgsl_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(STEREO_WGSL).expect("WGSL estéreo inválido");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("WGSL estéreo no valida");
    }

    #[test]
    fn scene3d_wgsl_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(SCENE3D_WGSL).expect("Scene 3D WGSL inválido");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("Scene 3D WGSL no valida");
    }

    #[test]
    fn globals_layout_is_stable() {
        assert_eq!(std::mem::size_of::<Globals>(), 32);
        assert_eq!(std::mem::align_of::<Globals>(), 4);
    }

    #[test]
    fn mesh_page_roundtrip() {
        let vertices = [
            MeshVertex {
                position: [0.0, 1.0, 2.0],
                normal: [0.0, 1.0, 0.0],
                color: [1.0, 0.5, 0.2],
            },
            MeshVertex {
                position: [1.0, 0.0, 2.0],
                normal: [0.0, 1.0, 0.0],
                color: [0.2, 0.8, 1.0],
            },
        ];
        let indices = [0u16, 1u16];
        let bytes = create_mesh_page(&vertices, &indices);
        assert!(bytes.len() >= std::mem::size_of::<MeshPageHeader>());
        let header: MeshPageHeader =
            bytemuck::cast_slice(&bytes[..std::mem::size_of::<MeshPageHeader>()])[0];
        assert_eq!(header.magic, MESH_PAGE_MAGIC);
        assert_eq!(header.vertex_count, 2);
        assert_eq!(header.index_count, 2);
    }
}
