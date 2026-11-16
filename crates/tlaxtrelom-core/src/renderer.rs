// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
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
use crate::dimension::{hyper_scene, hypercube, DimensionConfig};
use crate::mesh::{create_demo_scene, create_flat_scene, MeshVertex};
use crate::xr::{Eye, StereoFrame};

/// Shader único del preview estéreo / fondo de confort.
pub const STEREO_WGSL: &str = include_str!("../../../shaders/stereo.wgsl");

/// Shader del pipeline de renderizado de mallas 3D.
pub const SCENE3D_WGSL: &str = include_str!("../../../shaders/scene3d.wgsl");

/// Shader del pase de presentación (tonemap del HDR offscreen).
pub const PRESENT_WGSL: &str = include_str!("../../../shaders/present.wgsl");

/// Exposición aplicada en el tonemap. Calibra el brillo del preview de
/// demostración; los materiales reales deberían traer la suya propia.
pub const HDR_EXPOSURE: f32 = 1.08;

/// Magic identificador para páginas de geometría con formato de malla (`"TLXM"`).
pub const MESH_PAGE_MAGIC: u32 = 0x544C_584D;

const UPLOAD_RING_BYTES: u64 = 32 << 20; // 32 MiB
const STREAMED_MAX_VERTICES: usize = 65_536;
const STREAMED_MAX_INDICES: usize = 131_072;
/// Capacidad holgada para el penteract (80 aristas ⇒ 1920 vértices / 2880 índices).
const HYPER_MAX_VERTICES: usize = 4_096;
const HYPER_MAX_INDICES: usize = 8_192;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    resolution: [f32; 2],
    time: f32,
    eye: f32,
    ipd: f32,
    fov_y: f32,
    is_stereo: f32,
    /// Código de dimensión para el shader de fondo (2.0, 2.5, 3.0, 4.0, 4.5, 5.0).
    dimension: f32,
}

const _: () = assert!(std::mem::size_of::<Globals>() == 32);

/// Uniforms de escena 3D pasados al shader de mallas por ojo.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SceneUniforms {
    pub view_proj: [f32; 16],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    /// x = tiempo, y = código de dimensión, z = blend del modo fraccionario
    /// (3.5d: orto↔perspectiva, 4.5d: 3D↔4D), w = unlit (1 = 2D).
    pub params: [f32; 4],
    /// x = inicio de la niebla, y = rampa, z = densidad máxima, w = reserva.
    /// Depende del alcance del sector, no del shader.
    pub fog: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<SceneUniforms>() == 128);

/// Uniforms del pase de presentación.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct PresentUniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub exposure: f32,
}

const _: () = assert!(std::mem::size_of::<PresentUniforms>() == 16);

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
    /// Formato de profundidad negociado con el adaptador.
    pub depth_format: wgpu::TextureFormat,
    /// Sample count efectivo de MSAA (1 = sin MSAA).
    pub samples: u32,
    /// Target HDR lineal del frame offscreen (resuelto a un sample).
    hdr_texture: wgpu::Texture,
    /// Vista del HDR que lee el pase de presentación.
    pub hdr_view: wgpu::TextureView,
    /// Vista multisample del HDR; `None` si `samples == 1`.
    hdr_msaa_view: Option<wgpu::TextureView>,
    hdr_msaa_texture: Option<wgpu::Texture>,
    /// Formato flotante negociado para el HDR.
    pub hdr_format: wgpu::TextureFormat,
}

/// Attachment del frame offscreen HDR y del swapchain, con sus dimensiones.
///
/// Son dos conjuntos porque el HDR vive en una textura fuera del swapchain:
/// el scene pass escribe lineal aquí y el pase de presentación lo lee.
pub struct HdrFrame<'a> {
    pub color: &'a wgpu::TextureView,
    pub resolve: Option<&'a wgpu::TextureView>,
    pub depth: &'a wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

pub struct SwapFrame<'a> {
    pub color: &'a wgpu::TextureView,
    pub resolve: Option<&'a wgpu::TextureView>,
    pub width: u32,
    pub height: u32,
}

impl Gpu {
    pub async fn new(window: Arc<Window>, cfg: &Config) -> Result<Self> {
        let size = window.inner_size();
        let mut width = size.width.max(1);
        let mut height = size.height.max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window)
            .context("no se pudo crear la surface")?;

        // Preferimos la GPU discreta; si no existe (portátiles híbridos,
        // contenedores, VMs), reintentamos con la integrada antes de fallar.
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
        {
            Some(adapter) => adapter,
            None => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .context("no hay GPU compatible (¿drivers?)")?,
        };

        let available = adapter.features();
        let mut required = wgpu::Features::empty();
        if available.contains(wgpu::Features::TIMESTAMP_QUERY) {
            required |= wgpu::Features::TIMESTAMP_QUERY;
        }
        // `negotiate_samples` consulta capacidades por adaptador (p. ej. MSAA
        // 2x en Depth32Float). Sin esta feature el device sólo garantiza 1x y
        // 4x, y crear el attachment falla en tiempo de ejecución.
        if available.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
            required |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
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

        // Ningún tamaño puede exceder el límite del adaptador: una ventana
        // maximizada en un monitor 8K no debe provocar un error de validación.
        let max_dim = adapter.limits().max_texture_dimension_2d.max(1);
        width = width.min(max_dim);
        height = height.min(max_dim);

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

        let depth_format = pick_depth_format(&adapter);
        // El MSAA vive en el target HDR, no en el swapchain: el tonemap es un
        // triángulo fullscreen que ya lee geometría resolvida. Se negocia
        // contra el propio formato HDR.
        let hdr_format_base = pick_hdr_format(&adapter, 1);
        let samples = negotiate_samples(&adapter, hdr_format_base, depth_format, cfg.msaa);
        let hdr_format = pick_hdr_format(&adapter, samples);
        let (depth_texture, depth_view) =
            create_depth_texture(&device, width, height, depth_format, samples);
        let (hdr_texture, hdr_view) = create_hdr_texture(&device, width, height, hdr_format, 1);
        let (hdr_msaa_texture, hdr_msaa_view) = if samples > 1 {
            let (t, v) = create_hdr_texture(&device, width, height, hdr_format, samples);
            (Some(t), Some(v))
        } else {
            (None, None)
        };

        eprintln!(
            "[gpu] superficie {:?} {}x{} | HDR {:?} | MSAA {}x | depth {:?}",
            format, width, height, hdr_format, samples, depth_format
        );

        Ok(Self {
            device,
            queue,
            surface,
            config,
            depth_view,
            depth_texture,
            depth_format,
            samples,
            hdr_texture,
            hdr_view,
            hdr_msaa_view,
            hdr_msaa_texture,
            hdr_format,
        })
    }

    /// Attachment del frame offscreen para el pase de escena.
    pub fn hdr_frame(&self) -> HdrFrame<'_> {
        // Sin MSAA el propio target ya es de un sample: resolverse a sí mismo
        // sería un error de validación.
        let resolve = if self.samples > 1 {
            Some(&self.hdr_view)
        } else {
            None
        };
        HdrFrame {
            color: self.hdr_msaa_view.as_ref().unwrap_or(&self.hdr_view),
            resolve,
            depth: &self.depth_view,
            width: self.config.width,
            height: self.config.height,
        }
    }

    /// Attachment del swapchain para el pase de presentación. Siempre de un
    /// sample: el tonemap no tiene geometría que antialias, el MSAA real ya se
    /// resolvió en el target HDR.
    pub fn swap_frame<'a>(&'a self, surface_view: &'a wgpu::TextureView) -> SwapFrame<'a> {
        SwapFrame {
            color: surface_view,
            resolve: None,
            width: self.config.width,
            height: self.config.height,
        }
    }

    /// Devuelve `true` si hubo reconfiguración real.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        let max_dim = self.device.limits().max_texture_dimension_2d.max(1);
        let width = width.min(max_dim);
        let height = height.min(max_dim);
        if width == self.config.width && height == self.config.height {
            return false;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.recreate_attachments();
        true
    }

    /// Recuperación idempotente tras `Lost`/`Outdated`.
    pub fn recover(&mut self) {
        self.surface.configure(&self.device, &self.config);
        self.recreate_attachments();
    }

    fn recreate_attachments(&mut self) {
        let (depth_texture, depth_view) = create_depth_texture(
            &self.device,
            self.config.width,
            self.config.height,
            self.depth_format,
            self.samples,
        );
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
        let (hdr_texture, hdr_view) = create_hdr_texture(
            &self.device,
            self.config.width,
            self.config.height,
            self.hdr_format,
            1,
        );
        self.hdr_texture = hdr_texture;
        self.hdr_view = hdr_view;
        if self.samples > 1 {
            let (t, v) = create_hdr_texture(
                &self.device,
                self.config.width,
                self.config.height,
                self.hdr_format,
                self.samples,
            );
            self.hdr_msaa_texture = Some(t);
            self.hdr_msaa_view = Some(v);
        }
    }
}

fn pick_depth_format(adapter: &wgpu::Adapter) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureFormat::Depth24Plus,
        wgpu::TextureFormat::Depth16Unorm,
    ]
    .into_iter()
    .find(|format| {
        adapter
            .get_texture_format_features(*format)
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    })
    .unwrap_or(wgpu::TextureFormat::Depth32Float)
}

/// MSAA solo si color y profundidad soportan el mismo sample count y el color
/// puede resolverse automáticamente en el swapchain.
fn negotiate_samples(
    adapter: &wgpu::Adapter,
    color: wgpu::TextureFormat,
    depth: wgpu::TextureFormat,
    requested: u32,
) -> u32 {
    if requested <= 1 {
        return 1;
    }
    let color_features = adapter.get_texture_format_features(color);
    let depth_features = adapter.get_texture_format_features(depth);
    let can_resolve = color_features
        .flags
        .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE);
    if !can_resolve {
        return 1;
    }
    for count in [4u32, 2u32] {
        if count <= requested
            && color_features.flags.sample_count_supported(count)
            && depth_features.flags.sample_count_supported(count)
        {
            return count;
        }
    }
    1
}

fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    samples: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tlaxtrelom.depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples.max(1),
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Crea el target HDR lineal: se renderiza en él y se lee en el tonemap.
fn create_hdr_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    samples: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    create_hdr_texture_with_usage(
        device,
        width,
        height,
        format,
        samples,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    )
}

fn create_hdr_texture_with_usage(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    samples: u32,
    usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tlaxtrelom.hdr"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples.max(1),
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Elige el formato flotante del HDR entre los que se pueden renderizar y
/// muestrear. `Rgba16Float` es el presupuesto; si el adaptador no lo acepta,
/// `Rgba32Float`; y si tampoco, se queda en 8 bits y el tonemap pierde parte
/// de su gracia, pero el motor sigue dibujando.
fn pick_hdr_format(adapter: &wgpu::Adapter, samples: u32) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
    ]
    .into_iter()
    .find(|format| hdr_format_usable(adapter, *format, samples))
    .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb)
}

fn hdr_format_usable(adapter: &wgpu::Adapter, format: wgpu::TextureFormat, samples: u32) -> bool {
    let texture_features = adapter.get_texture_format_features(format);
    let flags = texture_features.flags;
    let usages = texture_features.allowed_usages;
    usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        && usages.contains(wgpu::TextureUsages::TEXTURE_BINDING)
        && flags.sample_count_supported(samples.max(1))
        // Con MSAA el HDR se resuelve a sí mismo en su versión de un sample;
        // sin MULTISAMPLE_RESOLVE ese pase es inválido.
        && (samples <= 1 || flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
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
    pipeline_present: wgpu::RenderPipeline,
    bind_groups: [wgpu::BindGroup; 2],
    bind_groups_3d: [wgpu::BindGroup; 2],
    bind_group_present: wgpu::BindGroup,
    globals: [wgpu::Buffer; 2],
    scene_buffers: [wgpu::Buffer; 2],
    present_buffer: wgpu::Buffer,
    scene_vertex_buffer: wgpu::Buffer,
    scene_index_buffer: wgpu::Buffer,
    scene_index_count: u32,
    streamed_vertex_buffer: wgpu::Buffer,
    streamed_index_buffer: wgpu::Buffer,
    streamed_index_count: u32,
    flat_vertex_buffer: wgpu::Buffer,
    flat_index_buffer: wgpu::Buffer,
    flat_index_count: u32,
    hyper_vertex_buffer: wgpu::Buffer,
    hyper_index_buffer: wgpu::Buffer,
    hyper_index_count: u32,
    upload: wgpu::Buffer,
    upload_offset: u64,
    upload_scratch: Vec<u8>,
    uploaded_bytes: u64,
}

impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        hdr_format: wgpu::TextureFormat,
        hdr_view: &wgpu::TextureView,
        samples: u32,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let multisample = wgpu::MultisampleState {
            count: samples.max(1),
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
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
                    format: hdr_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample,
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
                    format: hdr_format,
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
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample,
            multiview: None,
            cache: None,
        });

        // ── 3. Pase de presentación: tonemap del HDR offscreen ───────────────
        let shader_present = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tlaxtrelom.present"),
            source: wgpu::ShaderSource::Wgsl(PRESENT_WGSL.into()),
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tlaxtrelom.hdr.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let present_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.present.uniforms"),
            size: std::mem::size_of::<PresentUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl_present = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tlaxtrelom.present.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group_present = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tlaxtrelom.present.bind"),
            layout: &bgl_present,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(hdr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: present_buffer.as_entire_binding(),
                },
            ],
        });

        let layout_present = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tlaxtrelom.pipeline_present.layout"),
            bind_group_layouts: &[&bgl_present],
            push_constant_ranges: &[],
        });

        let pipeline_present = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tlaxtrelom.pipeline_present"),
            layout: Some(&layout_present),
            vertex: wgpu::VertexState {
                module: &shader_present,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_present,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // ── 4. Construcción de las escenas base y buffers de mallas ───────────
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

        // Escena plana 2D (coordenadas NDC) y politopo hiperdimensional.
        let (flat_vertices, flat_indices) = create_flat_scene();
        let flat_index_count = flat_indices.len() as u32;
        let flat_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tlaxtrelom.flat.vertices"),
            contents: bytemuck::cast_slice(&flat_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let flat_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tlaxtrelom.flat.indices"),
            contents: bytemuck::cast_slice(&flat_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let hyper_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.hyper.vertices"),
            size: (HYPER_MAX_VERTICES * std::mem::size_of::<MeshVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hyper_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.hyper.indices"),
            size: (HYPER_MAX_INDICES * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Buffers para geometría dinámica transmitida desde la isla C++
        let streamed_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.streamed.vertices"),
            size: (STREAMED_MAX_VERTICES * std::mem::size_of::<MeshVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let streamed_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.streamed.indices"),
            size: (STREAMED_MAX_INDICES * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ── 5. Ring buffer de subida de geometría ─────────────────────────────
        let upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.upload.ring"),
            size: UPLOAD_RING_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            pipeline_3d,
            pipeline_present,
            bind_groups,
            bind_groups_3d,
            bind_group_present,
            globals,
            scene_buffers,
            present_buffer,
            scene_vertex_buffer,
            scene_index_buffer,
            scene_index_count,
            streamed_vertex_buffer,
            streamed_index_buffer,
            streamed_index_count: 0,
            flat_vertex_buffer,
            flat_index_buffer,
            flat_index_count,
            hyper_vertex_buffer,
            hyper_index_buffer,
            hyper_index_count: 0,
            upload,
            upload_offset: 0,
            upload_scratch: Vec::with_capacity(64 * 1024),
            uploaded_bytes: 0,
        }
    }

    /// Actualiza uniforms por ojo y regenera el politopo hiperdimensional.
    ///
    /// `dimension` debe venir ya animada (`DimensionConfig::animated`); este
    /// método no acumula tiempo propio para que el render sea determinista
    /// respecto al reloj del host.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        time: f32,
        frame: &StereoFrame,
        dimension: DimensionConfig,
    ) {
        let resolution = [width.max(1) as f32, height.max(1) as f32];
        let fov = if frame.left.view_proj[5].is_finite() && frame.left.view_proj[5] > 1e-6 {
            2.0 * (1.0 / frame.left.view_proj[5]).atan()
        } else {
            90.0f32.to_radians()
        };

        let dimension_code = dimension.mode.shader_code();
        let unlit = if dimension.mode.is_flat() { 1.0 } else { 0.0 };
        let params = [time, dimension_code, dimension.blend, unlit];
        let fog = frame.fog;
        let light_dir = [0.4f32, 0.8f32, 0.5f32, 0.0f32];

        let camera_for = |eye: [f32; 3]| -> [f32; 3] {
            match dimension.mode {
                // La escena 2D vive en NDC; la "cámara" es solo una referencia
                // estable para especular/niebla (que además está desactivada).
                crate::dimension::DimensionMode::Flat2D => [0.0, 0.0, 2.5],
                // Isométrica que sigue el offset local, coherente con
                // `xr::isometric_view_projection`. El 3.5d usa el mismo
                // encuadre, sólo cambia la fuerza de perspectiva.
                crate::dimension::DimensionMode::Isometric25D
                | crate::dimension::DimensionMode::Fractional35D => {
                    [eye[0] - 14.0, eye[1] + 11.0, eye[2] - 10.0]
                }
                _ => eye,
            }
        };

        let left_cam = camera_for(frame.left.position);
        let right_cam = camera_for(frame.right.position);

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
                    dimension: dimension_code,
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
                    dimension: dimension_code,
                },
            );

            let left_scene = SceneUniforms {
                view_proj: frame.left.view_proj,
                camera_pos: [left_cam[0], left_cam[1], left_cam[2], time],
                light_dir: [light_dir[0], light_dir[1], light_dir[2], 0.0],
                params,
                fog,
            };
            let right_scene = SceneUniforms {
                view_proj: frame.right.view_proj,
                camera_pos: [right_cam[0], right_cam[1], right_cam[2], time],
                light_dir: [light_dir[0], light_dir[1], light_dir[2], 1.0],
                params,
                fog,
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
                dimension: dimension_code,
            };
            write_globals(queue, &self.globals[0], mono);
            write_globals(queue, &self.globals[1], mono);

            let mono_scene = SceneUniforms {
                view_proj: frame.left.view_proj,
                camera_pos: [left_cam[0], left_cam[1], left_cam[2], time],
                light_dir: [light_dir[0], light_dir[1], light_dir[2], 2.0],
                params,
                fog,
            };
            queue.write_buffer(&self.scene_buffers[0], 0, bytemuck::bytes_of(&mono_scene));
            queue.write_buffer(&self.scene_buffers[1], 0, bytemuck::bytes_of(&mono_scene));
        }

        self.update_hyper(queue, dimension);

        queue.write_buffer(
            &self.present_buffer,
            0,
            bytemuck::bytes_of(&PresentUniforms {
                resolution: [width.max(1) as f32, height.max(1) as f32],
                time,
                exposure: HDR_EXPOSURE,
            }),
        );
    }

    fn update_hyper(&mut self, queue: &wgpu::Queue, dimension: DimensionConfig) {
        if !dimension.mode.needs_hyper_scene() {
            self.hyper_index_count = 0;
            return;
        }
        let mesh = hypercube(dimension.mode.spatial_dimension());
        let (vertices, indices) = hyper_scene(&mesh, &dimension, [0.0, 2.4, 7.0], 1.35);
        if vertices.is_empty()
            || indices.is_empty()
            || vertices.len() > HYPER_MAX_VERTICES
            || indices.len() > HYPER_MAX_INDICES
            || indices.len() % 3 != 0
            || indices.iter().any(|&i| i as usize >= vertices.len())
        {
            // Nunca subimos geometría que el buffer no pueda contener: mejor
            // ocultar el politopo un frame que provocar un error de validación.
            self.hyper_index_count = 0;
            return;
        }
        queue.write_buffer(
            &self.hyper_vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );
        queue.write_buffer(&self.hyper_index_buffer, 0, bytemuck::cast_slice(&indices));
        self.hyper_index_count = indices.len() as u32;
    }

    /// Dibuja el frame: escena al HDR offscreen y tonemap al swapchain.
    /// Requiere dos pases porque el destino del scene pass ya no es el
    /// swapchain; el pase de presentación es el único que comprime rango.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        hdr: &HdrFrame<'_>,
        swap: &SwapFrame<'_>,
        stereo: bool,
        flat: bool,
    ) {
        self.draw_scene(encoder, hdr, stereo, flat);
        self.draw_present(encoder, swap);
    }

    /// Pase de escena: fondo + mallas en lineal al target HDR.
    pub fn draw_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        hdr: &HdrFrame<'_>,
        stereo: bool,
        flat: bool,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tlaxtrelom.scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: hdr.color,
                resolve_target: hdr.resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.004,
                        g: 0.006,
                        b: 0.012,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: hdr.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let half = hdr.width / 2;
        if stereo {
            pass.set_scissor_rect(0, 0, half.max(1), hdr.height.max(1));
            self.draw_eye(&mut pass, 0, flat);
            pass.set_scissor_rect(half, 0, half.max(1), hdr.height.max(1));
            self.draw_eye(&mut pass, 1, flat);
        } else {
            pass.set_scissor_rect(0, 0, hdr.width.max(1), hdr.height.max(1));
            self.draw_eye(&mut pass, 0, flat);
        }
    }

    /// Pase de presentación: tonemap del HDR al destino dado.
    pub fn draw_present(&self, encoder: &mut wgpu::CommandEncoder, swap: &SwapFrame<'_>) {
        let mut present = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tlaxtrelom.present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: swap.color,
                resolve_target: swap.resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        present.set_pipeline(&self.pipeline_present);
        present.set_bind_group(0, &self.bind_group_present, &[]);
        present.draw(0..3, 0..1);
    }

    fn draw_eye(&self, pass: &mut wgpu::RenderPass<'_>, eye_index: usize, flat: bool) {
        // Fondo de presentación / confort (no escribe profundidad).
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_groups[eye_index], &[]);
        pass.draw(0..3, 0..1);

        pass.set_pipeline(&self.pipeline_3d);
        pass.set_bind_group(0, &self.bind_groups_3d[eye_index], &[]);

        if flat {
            // Modo 2D: escena plana en NDC, sin cámara ni geometría de mundo.
            pass.set_vertex_buffer(0, self.flat_vertex_buffer.slice(..));
            pass.set_index_buffer(self.flat_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.flat_index_count, 0, 0..1);
            return;
        }

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

        if self.hyper_index_count > 0 {
            pass.set_vertex_buffer(0, self.hyper_vertex_buffer.slice(..));
            pass.set_index_buffer(self.hyper_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.hyper_index_count, 0, 0..1);
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
                let v_count = header.vertex_count as usize;
                let i_count = header.index_count as usize;
                let v_start = std::mem::size_of::<MeshPageHeader>();
                let v_len = v_count.checked_mul(std::mem::size_of::<MeshVertex>());
                let i_len = i_count.checked_mul(std::mem::size_of::<u16>());
                let end = v_len.and_then(|v| {
                    i_len.and_then(|i| v_start.checked_add(v).and_then(|x| x.checked_add(i)))
                });

                let fits = v_count > 0
                    && v_count <= STREAMED_MAX_VERTICES
                    && i_count > 0
                    && i_count <= STREAMED_MAX_INDICES
                    && i_count % 3 == 0
                    && end.is_some_and(|end| data.len() >= end);

                if fits {
                    let (Some(v_len), Some(_i_len), Some(end)) = (v_len, i_len, end) else {
                        return data.len();
                    };
                    let i_start = v_start + v_len;
                    let v_bytes = &data[v_start..v_start + v_len];
                    let i_bytes = &data[i_start..end];
                    // Un índice fuera de rango provocaría una lectura OOB de
                    // vértices en la GPU: se descarta la página completa.
                    let indices_ok = i_bytes
                        .chunks_exact(2)
                        .all(|c| (u16::from_le_bytes([c[0], c[1]]) as usize) < v_count);
                    if indices_ok {
                        queue.write_buffer(&self.streamed_vertex_buffer, 0, v_bytes);
                        queue.write_buffer(&self.streamed_index_buffer, 0, i_bytes);
                        self.streamed_index_count = i_count as u32;
                    }
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
    fn present_wgsl_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(PRESENT_WGSL).expect("present.wgsl inválido");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("WGSL de presentación no valida");
    }

    #[test]
    fn globals_layout_is_stable() {
        assert_eq!(std::mem::size_of::<Globals>(), 32);
        assert_eq!(std::mem::align_of::<Globals>(), 4);
    }

    #[test]
    fn scene_uniforms_layout_is_stable() {
        assert_eq!(std::mem::size_of::<SceneUniforms>(), 128);
        assert_eq!(std::mem::align_of::<SceneUniforms>(), 4);
    }

    #[test]
    fn present_uniforms_layout_is_stable() {
        assert_eq!(std::mem::size_of::<PresentUniforms>(), 16);
        assert_eq!(std::mem::align_of::<PresentUniforms>(), 4);
    }

    /// Render offscreen de un frame completo (scene → HDR → present) con
    /// lectura de vuelta de los píxeles. Requiere GPU, por eso corre sólo bajo
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore = "necesita GPU; en CI corre con el runner gráfico"]
    fn gpu_frame_renders_and_presents_nonblack() {
        let pixels = render_offscreen_frame(1);
        assert_luma_in_expected_range(&pixels);
    }

    /// Mismo frame por el camino MSAA: el pase de escena escribe en un target
    /// multisample y resuelve al HDR de un sample, y el tonemap lee ése. Es la
    /// configuración real de `--msaa 4`, y la que más fácil se rompe con un
    /// "resolve a sí mismo" mal parametrizado.
    #[test]
    #[ignore = "necesita GPU; en CI corre con el runner gráfico"]
    fn gpu_frame_renders_and_presents_nonblack_msaa4() {
        let pixels = render_offscreen_frame(4);
        assert_luma_in_expected_range(&pixels);
    }

    /// 2x no está garantizado por la especificación WebGPU (sólo 1x y 4x lo
    /// están): requiere `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` y que el
    /// depth format lo acepte. `--msaa 2` panicaba en `create_texture` antes de
    /// negociar esto bien.
    #[test]
    #[ignore = "necesita GPU; en CI corre con el runner gráfico"]
    fn gpu_frame_renders_and_presents_nonblack_msaa2() {
        let pixels = render_offscreen_frame(2);
        assert_luma_in_expected_range(&pixels);
    }

    fn render_offscreen_frame(requested_msaa: u32) -> Vec<[f32; 3]> {
        let size = 64u32;
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("sin adaptador GPU para la prueba offscreen");
        let mut required_features = wgpu::Features::empty();
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            required_features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tlaxtrelom.test.device"),
                required_features,
                ..Default::default()
            },
            None,
        ))
        .expect("request_device offscreen");

        let depth_format = pick_depth_format(&adapter);
        let present_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let hdr_format_base = pick_hdr_format(&adapter, 1);
        let samples = negotiate_samples(&adapter, hdr_format_base, depth_format, requested_msaa);
        let hdr_format = pick_hdr_format(&adapter, samples);

        let (_hdr_texture, hdr_view) = create_hdr_texture(&device, size, size, hdr_format, 1);
        let (_hdr_msaa_texture, hdr_msaa_view) = if samples > 1 {
            let (t, v) = create_hdr_texture(&device, size, size, hdr_format, samples);
            (Some(t), Some(v))
        } else {
            (None, None)
        };
        let (_depth_texture, depth_view) =
            create_depth_texture(&device, size, size, depth_format, samples);
        let (present_texture, present_view) = create_hdr_texture_with_usage(
            &device,
            size,
            size,
            present_format,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );

        let mut renderer = Renderer::new(
            &device,
            present_format,
            hdr_format,
            &hdr_view,
            samples,
            depth_format,
        );
        let frame = crate::xr::StereoFrame::mono(
            crate::xr::HeadPose {
                position: [0.0, 1.6, 0.0],
                yaw: 0.0,
            },
            1.0,
            90.0f32.to_radians(),
            0,
            std::time::Instant::now(),
        );
        renderer.update(
            &queue,
            size,
            size,
            0.0,
            &frame,
            crate::dimension::DimensionConfig::default(),
        );

        let hdr = HdrFrame {
            color: hdr_msaa_view.as_ref().unwrap_or(&hdr_view),
            resolve: if samples > 1 { Some(&hdr_view) } else { None },
            depth: &depth_view,
            width: size,
            height: size,
        };
        let swap = SwapFrame {
            color: &present_view,
            resolve: None,
            width: size,
            height: size,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.draw_scene(&mut encoder, &hdr, false, false);
        renderer.draw_present(&mut encoder, &swap);
        queue.submit(Some(encoder.finish()));

        let bytes = read_back(&device, &queue, &present_texture, size, present_format);
        let pixels: Vec<[f32; 3]> = chunks_to_linear(&bytes, present_format);
        assert_eq!(pixels.len(), (size * size) as usize);
        pixels
    }

    fn assert_luma_in_expected_range(pixels: &[[f32; 3]]) {
        // El fondo de la escena no es negro absoluto: el tonemap está escribiendo.
        // Valores de referencia medidos en HW real (RADV RENOIR): un frame
        // correcto del demo queda en ~0.21 de luma media y ~0.78 máxima.
        let luma = |p: &[f32; 3]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        let max_luma = pixels.iter().map(luma).fold(0.0f32, f32::max);
        assert!(max_luma > 0.01, "frame de presentación totalmente negro");
        let avg_luma = pixels.iter().map(luma).sum::<f32>() / pixels.len() as f32;
        assert!(
            (0.05..0.9).contains(&avg_luma),
            "luma media fuera de rango: {avg_luma:.4}"
        );

        // El tonemap no debe empastar todo a blanco ni dejar huecos de cero.
        let saturated = pixels.iter().filter(|p| luma(p) > 0.999).count();
        assert!(
            saturated * 20 < pixels.len(),
            "el tonemap saturó {} de {} píxeles",
            saturated,
            pixels.len()
        );
        let black = pixels.iter().filter(|p| luma(p) < 0.001).count();
        assert!(
            black * 4 < pixels.len(),
            "{} de {} píxeles quedaron en negro puro: el clear o el pase de \
             presentación no se ejecutó",
            black,
            pixels.len()
        );
    }

    fn read_back(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        size: u32,
        format: wgpu::TextureFormat,
    ) -> Vec<u8> {
        let bytes_per_texel = format
            .block_copy_size(Some(wgpu::TextureAspect::All))
            .unwrap_or(4);
        // Las copias texture→buffer exigen que `bytes_per_row` sea múltiplo de
        // `COPY_BYTES_PER_ROW_ALIGNMENT` (256); 64 px × 4 B = 256 B, ya alineado.
        let bytes_per_row = size * bytes_per_texel;
        assert_eq!(
            bytes_per_row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
            0,
            "el readback necesita filas alineadas a 256 B"
        );
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tlaxtrelom.test.readback"),
            size: u64::from(bytes_per_row * size),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(size),
                },
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        let _ = device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .expect("canal de mapeo cerrado")
            .expect("no se pudo mapear el readback");
        let out = slice.get_mapped_range().to_vec();
        staging.unmap();
        out
    }

    /// Convierte el readback a lineal `[0, 1]` por píxel.
    fn chunks_to_linear(bytes: &[u8], format: wgpu::TextureFormat) -> Vec<[f32; 3]> {
        let half = |b: [u8; 2]| {
            let v = u16::from_le_bytes(b);
            let sign = if v & 0x8000 != 0 { -1.0 } else { 1.0 };
            let exp = ((v >> 10) & 0x1f) as i32;
            let frac = (v & 0x3ff) as f32;
            if exp == 0 {
                sign * frac * 2f32.powi(-14)
            } else if exp == 31 {
                f32::INFINITY
            } else {
                sign * (1.0 + frac / 1024.0) * 2f32.powi(exp - 15)
            }
        };
        let srgb = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let float16 = format == wgpu::TextureFormat::Rgba16Float;
        bytes
            .chunks_exact(4)
            .map(|c| {
                if float16 {
                    [half([c[0], c[1]]), half([c[2], c[3]]), 0.0]
                } else {
                    [
                        srgb(c[0] as f32 / 255.0),
                        srgb(c[1] as f32 / 255.0),
                        srgb(c[2] as f32 / 255.0),
                    ]
                }
            })
            .collect()
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
