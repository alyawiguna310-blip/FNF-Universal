use fu_render3d::backend::{BackendCapabilities, Composition, FramePacket, RenderBackend};
use fu_render3d::light::LightKind;
use fu_render3d::material::{AlphaMode, Material, TextureData};
use fu_render3d::mesh::MeshData;
use fu_render3d::resources::Handle;
use fu_render3d::shader::{ShaderId, ShaderRegistry};
use fu_render3d::{RenderError, Result};
use glam::Vec3;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

use crate::types::*;

pub const UNLIT_WGSL: &str = include_str!("shaders/unlit.wgsl");
pub const LIT_WGSL: &str = include_str!("shaders/lit.wgsl");

/// Helper to register the builtin shaders (`builtin/unlit` and `builtin/lit`) into a `ShaderRegistry`.
pub fn register_builtin_shaders(registry: &mut ShaderRegistry) -> Result<()> {
    if !registry.contains(&ShaderId::unlit()) {
        registry.register(ShaderId::unlit(), UNLIT_WGSL)?;
    }
    if !registry.contains(&ShaderId::lit()) {
        registry.register(ShaderId::lit(), LIT_WGSL)?;
    }
    Ok(())
}

pub struct GpuMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub index_format: wgpu::IndexFormat,
}

pub struct GpuTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

pub struct GpuMaterial {
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub last_uniform: MaterialUniform,
    pub texture_handle: Option<Handle<TextureData>>,
}

pub struct GpuModelSlot {
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

#[derive(Clone, Debug)]
pub struct PipelineKey {
    pub shader_id: ShaderId,
    pub shader_revision: u32,
    pub alpha_mode: AlphaMode,
    pub msaa_samples: u32,
    pub color_format: wgpu::TextureFormat,
}

impl std::hash::Hash for PipelineKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.shader_id.hash(state);
        self.shader_revision.hash(state);
        match self.alpha_mode {
            AlphaMode::Opaque => 0u8.hash(state),
            AlphaMode::Mask { cutoff } => (1u8, cutoff.to_bits()).hash(state),
            AlphaMode::Blend => 2u8.hash(state),
        }
        self.msaa_samples.hash(state);
        self.color_format.hash(state);
    }
}

impl PartialEq for PipelineKey {
    fn eq(&self, other: &Self) -> bool {
        self.shader_id == other.shader_id
            && self.shader_revision == other.shader_revision
            && self.alpha_mode == other.alpha_mode
            && self.msaa_samples == other.msaa_samples
            && self.color_format == other.color_format
    }
}

impl Eq for PipelineKey {}

pub struct WgpuBackend {
    pub instance: wgpu::Instance,
    pub adapter: Arc<wgpu::Adapter>,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub capabilities: BackendCapabilities,
    pub color_format: wgpu::TextureFormat,

    // Layouts
    pub camera_bind_group_layout: wgpu::BindGroupLayout,
    pub model_bind_group_layout: wgpu::BindGroupLayout,
    pub material_bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline_layout: wgpu::PipelineLayout,

    // Caches
    pub mesh_cache: HashMap<Handle<MeshData>, GpuMesh>,
    pub texture_cache: HashMap<Handle<TextureData>, GpuTexture>,
    pub material_cache: HashMap<Handle<Material>, GpuMaterial>,
    pub pipeline_cache: HashMap<PipelineKey, Arc<wgpu::RenderPipeline>>,
    pub default_texture: GpuTexture,

    // Active Render Targets (for headless/offscreen)
    pub current_color_texture: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
    pub current_depth_texture: Option<(u32, u32, u32, wgpu::Texture, wgpu::TextureView)>,
    pub current_msaa_texture: Option<(u32, u32, u32, wgpu::Texture, wgpu::TextureView)>,

    // External target override if rendering directly to a provided TextureView (e.g. from a window surface)
    pub target_view_override: Option<wgpu::TextureView>,

    // Persistent uniform buffers & bind groups
    /// Persistent camera uniform buffer (UNIFORM | COPY_DST). Written each frame via queue.write_buffer.
    pub camera_uniform_buf: wgpu::Buffer,
    /// Persistent lighting uniform buffer (UNIFORM | COPY_DST). Written each frame via queue.write_buffer.
    pub lighting_uniform_buf: wgpu::Buffer,
    /// Cached bind group for the persistent camera+lighting buffers. Created once at startup.
    pub camera_lighting_bind_group: wgpu::BindGroup,

    // Scratch buffers & pools for allocations reduction in frequently executed paths
    pub model_uniform_pool: Vec<GpuModelSlot>,
    pub scratch_pipelines: Vec<Arc<wgpu::RenderPipeline>>,
}

impl WgpuBackend {
    /// Creates a headless WgpuBackend (picks default adapter and high-perf/compatible device).
    pub fn new_headless() -> Result<Self> {
        pollster::block_on(Self::new_headless_async())
    }

    pub async fn new_headless_async() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| RenderError::Backend("failed to find suitable GPU adapter".into()))?;

        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("fu-render3d-wgpu Headless Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| RenderError::Backend(format!("failed to create device: {e}")))?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let adapter = Arc::new(adapter);

        Self::build_internals(
            instance,
            adapter,
            device,
            queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
    }

    /// Creates a [`WgpuBackend`] from pre-existing wgpu components.
    ///
    /// Used by [`crate::surface::WindowedRenderer`], which creates its own
    /// instance, surface-compatible adapter, device and queue, then delegates
    /// GPU layout and resource management here.
    pub fn from_parts(
        instance: wgpu::Instance,
        adapter: Arc<wgpu::Adapter>,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        color_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        Self::build_internals(instance, adapter, device, queue, color_format)
    }

    fn build_internals(
        instance: wgpu::Instance,
        adapter: Arc<wgpu::Adapter>,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        color_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let limits = adapter.limits();
        let capabilities = BackendCapabilities {
            max_texture_size: limits.max_texture_dimension_2d,
            max_msaa_samples: 4,
            supports_shadows: false, // Shadows disabled until actual shadow mapping pass exists
        };

        // Bind Group Layouts
        // Group 0: Camera (binding 0) + Lighting (binding 1)
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Camera & Lighting Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
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

        // Group 1: Model Uniforms (binding 0)
        let model_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Model Bind Group Layout"),
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
            });

        // Group 2: Material Uniforms (binding 0) + Texture (binding 1) + Sampler (binding 2)
        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Material Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render3D Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &model_bind_group_layout,
                &material_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

        // Default 1x1 white texture
        let default_texture_data = [255u8, 255u8, 255u8, 255u8];
        let default_texture = Self::create_texture_from_bytes(
            &device,
            &queue,
            1,
            1,
            &default_texture_data,
            "Default White Texture",
        )?;

        // Persistent uniform buffers: avoid per-frame GPU buffer allocations
        let camera_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Uniform Buffer (persistent)"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let lighting_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Lighting Uniform Buffer (persistent)"),
            size: std::mem::size_of::<LightingUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_lighting_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera & Lighting Bind Group (persistent)"),
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: lighting_uniform_buf.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            capabilities,
            color_format,
            camera_bind_group_layout,
            model_bind_group_layout,
            material_bind_group_layout,
            pipeline_layout,
            mesh_cache: HashMap::new(),
            texture_cache: HashMap::new(),
            material_cache: HashMap::new(),
            pipeline_cache: HashMap::new(),
            default_texture,
            current_color_texture: None,
            current_depth_texture: None,
            current_msaa_texture: None,
            target_view_override: None,
            camera_uniform_buf,
            lighting_uniform_buf,
            camera_lighting_bind_group,
            model_uniform_pool: Vec::new(),
            scratch_pipelines: Vec::new(),
        })
    }

    /// Sets the color format for the backend, invalidating current color targets if changed.
    pub fn set_color_format(&mut self, format: wgpu::TextureFormat) {
        if self.color_format != format {
            self.color_format = format;
            self.current_color_texture = None;
            self.current_msaa_texture = None;
        }
    }

    /// Reads the offscreen colour target back into CPU memory as RGBA8 rows.
    ///
    /// Intended for headless verification: it only works after rendering into a
    /// backend-owned offscreen target, not when a window surface is the target.
    pub fn read_target_pixels(&self) -> Result<(u32, u32, Vec<u8>)> {
        let Some((width, height, texture, _view)) = &self.current_color_texture else {
            return Err(RenderError::Backend(
                "no offscreen colour target to read back".into(),
            ));
        };
        let (width, height) = (*width, *height);
        let unpadded_row = width as usize * 4;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded_row = unpadded_row.div_ceil(alignment) * alignment;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Offscreen Readback Buffer"),
            size: (padded_row * height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Offscreen Readback Encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .map_err(|error| RenderError::Backend(format!("readback mapping lost: {error}")))?
            .map_err(|error| {
                RenderError::Backend(format!("readback buffer mapping failed: {error:?}"))
            })?;

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity(unpadded_row * height as usize);
        for row in 0..height as usize {
            let start = row * padded_row;
            pixels.extend_from_slice(&mapped[start..start + unpadded_row]);
        }
        drop(mapped);
        buffer.unmap();
        Ok((width, height, pixels))
    }

    /// Prunes and clears cached GPU materials, pipelines, meshes, and textures.
    pub fn clear_caches(&mut self) {
        self.mesh_cache.clear();
        self.texture_cache.clear();
        self.material_cache.clear();
        self.pipeline_cache.clear();
    }

    fn create_texture_from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba8: &[u8],
        label: &str,
    ) -> Result<GpuTexture> {
        if width == 0 || height == 0 {
            return Err(RenderError::Invalid {
                what: "texture",
                reason: "dimensions must be non-zero".into(),
            });
        }
        let max_dim = device.limits().max_texture_dimension_2d;
        if width > max_dim || height > max_dim {
            return Err(RenderError::Invalid {
                what: "texture",
                reason: format!(
                    "texture dimension {width}x{height} exceeds device limit {max_dim}"
                ),
            });
        }
        let expected_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| RenderError::Invalid {
                what: "texture",
                reason: "texture dimensions overflow".into(),
            })?;
        if rgba8.len() < expected_len {
            return Err(RenderError::Invalid {
                what: "texture",
                reason: format!(
                    "texture data buffer length {} is smaller than required {}",
                    rgba8.len(),
                    expected_len
                ),
            });
        }

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba8,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(GpuTexture {
            texture,
            view,
            sampler,
        })
    }

    pub(crate) fn ensure_targets(&mut self, width: u32, height: u32, msaa_samples: u32) {
        // Depth texture
        let need_depth = match &self.current_depth_texture {
            Some((w, h, samples, _, _)) => *w != width || *h != height || *samples != msaa_samples,
            None => true,
        };
        if need_depth {
            let depth_tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Depth Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: msaa_samples,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.current_depth_texture = Some((width, height, msaa_samples, depth_tex, depth_view));
        }

        // Color texture (if no override)
        if self.target_view_override.is_none() {
            let need_color = match &self.current_color_texture {
                Some((w, h, _, _)) => *w != width || *h != height,
                None => true,
            };
            if need_color {
                let color_tex = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Offscreen Color Texture"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.color_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());
                self.current_color_texture = Some((width, height, color_tex, color_view));
            }
        }

        // MSAA texture
        if msaa_samples > 1 {
            let need_msaa = match &self.current_msaa_texture {
                Some((w, h, s, _, _)) => *w != width || *h != height || *s != msaa_samples,
                None => true,
            };
            if need_msaa {
                let msaa_tex = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("MSAA Color Texture"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: msaa_samples,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.color_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());
                self.current_msaa_texture =
                    Some((width, height, msaa_samples, msaa_tex, msaa_view));
            }
        }
    }

    fn get_or_compile_pipeline(
        &mut self,
        key: PipelineKey,
        wgsl_source: &str,
    ) -> Result<Arc<wgpu::RenderPipeline>> {
        if let Some(pipe) = self.pipeline_cache.get(&key) {
            return Ok(Arc::clone(pipe));
        }

        let shader_module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(key.shader_id.as_str()),
                source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
            });

        let blend_state = match key.alpha_mode {
            AlphaMode::Blend => Some(wgpu::BlendState::ALPHA_BLENDING),
            AlphaMode::Opaque | AlphaMode::Mask { .. } => Some(wgpu::BlendState::REPLACE),
        };

        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(key.shader_id.as_str()),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_module,
                    entry_point: Some("vs_main"),
                    buffers: &[GpuVertex::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: key.color_format,
                        blend: blend_state,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: matches!(
                        key.alpha_mode,
                        AlphaMode::Opaque | AlphaMode::Mask { .. }
                    ),
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: key.msaa_samples,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            });

        let pipe_arc = Arc::new(pipeline);
        self.pipeline_cache.insert(key, Arc::clone(&pipe_arc));
        Ok(pipe_arc)
    }
}

impl RenderBackend for WgpuBackend {
    fn name(&self) -> &str {
        "wgpu"
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities
    }

    fn render(&mut self, frame: &FramePacket<'_>) -> Result<()> {
        let width = frame.viewport.width;
        let height = frame.viewport.height;
        if width == 0 || height == 0 {
            return Ok(());
        }

        let msaa_samples = frame.quality.msaa_samples.max(1) as u32;
        self.ensure_targets(width, height, msaa_samples);

        // Upload meshes if needed
        let all_draw_items = frame
            .queue
            .opaque
            .iter()
            .chain(frame.queue.transparent.iter());

        for item in all_draw_items.clone() {
            if !self.mesh_cache.contains_key(&item.mesh) {
                if let Some(mesh_data) = frame.meshes.get(item.mesh) {
                    let vertices: Vec<GpuVertex> = mesh_data
                        .vertices()
                        .iter()
                        .map(|v| GpuVertex {
                            position: v.position,
                            normal: v.normal,
                            uv: v.uv,
                        })
                        .collect();

                    let vertex_buffer =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some(mesh_data.name()),
                                contents: bytemuck::cast_slice(&vertices),
                                usage: wgpu::BufferUsages::VERTEX,
                            });

                    let index_buffer =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some(&format!("{} indices", mesh_data.name())),
                                contents: bytemuck::cast_slice(mesh_data.indices()),
                                usage: wgpu::BufferUsages::INDEX,
                            });

                    self.mesh_cache.insert(
                        item.mesh,
                        GpuMesh {
                            vertex_buffer,
                            index_buffer,
                            index_count: mesh_data.indices().len() as u32,
                            index_format: wgpu::IndexFormat::Uint32,
                        },
                    );
                }
            }

            // Upload textures if material has texture
            if let Some(mat) = frame.materials.get(item.material) {
                if let Some(tex_handle) = mat.base_color_texture {
                    if !self.texture_cache.contains_key(&tex_handle) {
                        if let Some(tex_data) = frame.textures.get(tex_handle) {
                            let gpu_tex = Self::create_texture_from_bytes(
                                &self.device,
                                &self.queue,
                                tex_data.width,
                                tex_data.height,
                                &tex_data.rgba8,
                                &tex_data.name,
                            )?;
                            self.texture_cache.insert(tex_handle, gpu_tex);
                        }
                    }
                }
            }
        }

        // Camera Uniform — write to persistent buffer
        let view_proj = frame.projection * frame.view;
        let camera_uniform = CameraUniform {
            view_proj: view_proj.to_cols_array(),
            camera_pos: frame.camera_position.into(),
            _pad0: 0.0,
        };
        self.queue.write_buffer(
            &self.camera_uniform_buf,
            0,
            bytemuck::bytes_of(&camera_uniform),
        );

        // Lighting Uniform — write to persistent buffer
        let mut lighting_uniform = LightingUniform {
            ambient_color_intensity: [
                frame.ambient_color[0],
                frame.ambient_color[1],
                frame.ambient_color[2],
                frame.ambient_intensity,
            ],
            light_count: [frame.lights.len().min(MAX_LIGHTS) as u32, 0, 0, 0],
            ..Default::default()
        };

        for (i, light) in frame.lights.iter().take(MAX_LIGHTS).enumerate() {
            let (kind_num, range, inner, outer) = match light.kind {
                LightKind::Directional => (0.0f32, -1.0f32, 0.0, 0.0),
                LightKind::Point { range } => (1.0f32, range, 0.0, 0.0),
                LightKind::Spot {
                    range,
                    inner_cone,
                    outer_cone,
                } => (2.0f32, range, inner_cone.cos(), outer_cone.cos()),
            };

            let pos = light.transform.translation;
            let dir = light.transform.rotation * Vec3::NEG_Z;

            lighting_uniform.lights[i] = LightDataUniform {
                position_range: [pos.x, pos.y, pos.z, range],
                color_intensity: [
                    light.color[0],
                    light.color[1],
                    light.color[2],
                    light.intensity,
                ],
                direction_kind: [dir.x, dir.y, dir.z, kind_num],
                spot_angles: [inner, outer, 0.0, 0.0],
            };
        }

        self.queue.write_buffer(
            &self.lighting_uniform_buf,
            0,
            bytemuck::bytes_of(&lighting_uniform),
        );

        // Pre-compile / fetch pipelines into scratch buffer to avoid per-frame vector allocations
        let total_items = frame.queue.opaque.len() + frame.queue.transparent.len();
        self.scratch_pipelines.clear();
        self.scratch_pipelines.reserve(total_items);

        for item in all_draw_items.clone() {
            let Some(mat) = frame.materials.get(item.material) else {
                continue;
            };
            let (source_str, revision) = if let Some(src) = frame.shaders.get(&item.key.shader) {
                (src.wgsl.as_str(), src.revision)
            } else if item.key.shader.as_str() == ShaderId::UNLIT {
                (UNLIT_WGSL, 1)
            } else if item.key.shader.as_str() == ShaderId::LIT {
                (LIT_WGSL, 1)
            } else {
                return Err(RenderError::NotFound(format!(
                    "shader {}",
                    item.key.shader.as_str()
                )));
            };

            let pipeline_key = PipelineKey {
                shader_id: item.key.shader.clone(),
                shader_revision: revision,
                alpha_mode: mat.alpha_mode,
                msaa_samples,
                color_format: self.color_format,
            };

            let pipeline = self.get_or_compile_pipeline(pipeline_key, source_str)?;
            self.scratch_pipelines.push(pipeline);
        }

        // Ensure model uniform buffer pool has enough slots for all draw items
        while self.model_uniform_pool.len() < total_items {
            let idx = self.model_uniform_pool.len();
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Model Uniform Buffer (slot {idx})")),
                size: std::mem::size_of::<ModelUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("Model Bind Group (slot {idx})")),
                layout: &self.model_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            self.model_uniform_pool
                .push(GpuModelSlot { buffer, bind_group });
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Frame Encoder"),
            });

        let target_view = if let Some(view) = &self.target_view_override {
            view
        } else {
            &self.current_color_texture.as_ref().unwrap().3
        };

        let depth_view = &self.current_depth_texture.as_ref().unwrap().4;

        // Setup color attachment (accounting for MSAA if needed)
        let (color_attachment, resolve_target) = if msaa_samples > 1 {
            (
                &self.current_msaa_texture.as_ref().unwrap().4,
                Some(target_view),
            )
        } else {
            (target_view, None)
        };

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D Scene Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_attachment,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: match frame.composition {
                                Composition::Scene3dOnly => 1.0,
                                Composition::Scene3dUnder2d | Composition::Scene3dOver2d => 0.0,
                            },
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

            render_pass.set_bind_group(0, &self.camera_lighting_bind_group, &[]);

            // Draw items (opaque then transparent)
            let mut pipe_idx = 0;
            for (slot_idx, item) in all_draw_items.enumerate() {
                let Some(mesh) = self.mesh_cache.get(&item.mesh) else {
                    continue;
                };
                let Some(mat) = frame.materials.get(item.material) else {
                    continue;
                };

                // Model Uniform — write to slot in reused model uniform pool
                let normal_mat = item.world.inverse().transpose();
                let model_uniform = ModelUniform {
                    model: item.world.to_cols_array(),
                    normal_mat: normal_mat.to_cols_array(),
                };
                let slot = &self.model_uniform_pool[slot_idx];
                self.queue
                    .write_buffer(&slot.buffer, 0, bytemuck::bytes_of(&model_uniform));

                // Material Uniform & Bind Group — cached in self.material_cache
                let has_texture = mat.base_color_texture.is_some();
                let cutoff = match mat.alpha_mode {
                    AlphaMode::Mask { cutoff } => cutoff,
                    _ => 0.0,
                };
                let mat_uniform = MaterialUniform {
                    base_color: mat.base_color,
                    emissive_roughness: [
                        mat.emissive[0],
                        mat.emissive[1],
                        mat.emissive[2],
                        mat.roughness,
                    ],
                    params: [
                        mat.metallic,
                        cutoff,
                        if has_texture { 1.0 } else { 0.0 },
                        0.0,
                    ],
                };

                let gpu_tex = match mat
                    .base_color_texture
                    .and_then(|h| self.texture_cache.get(&h))
                {
                    Some(t) => t,
                    None => &self.default_texture,
                };

                // Retrieve or initialize cached GPU material
                let gpu_mat = if let Some(cached) = self.material_cache.get_mut(&item.material) {
                    if cached.last_uniform != mat_uniform {
                        self.queue.write_buffer(
                            &cached.uniform_buffer,
                            0,
                            bytemuck::bytes_of(&mat_uniform),
                        );
                        cached.last_uniform = mat_uniform;
                    }
                    if cached.texture_handle != mat.base_color_texture {
                        cached.bind_group =
                            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some(&format!("Material Bind Group ({})", mat.name)),
                                layout: &self.material_bind_group_layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: cached.uniform_buffer.as_entire_binding(),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::TextureView(&gpu_tex.view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: wgpu::BindingResource::Sampler(&gpu_tex.sampler),
                                    },
                                ],
                            });
                        cached.texture_handle = mat.base_color_texture;
                    }
                    cached
                } else {
                    let uniform_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("Material Uniform Buffer ({})", mat.name)),
                        size: std::mem::size_of::<MaterialUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue
                        .write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&mat_uniform));
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("Material Bind Group ({})", mat.name)),
                        layout: &self.material_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: uniform_buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&gpu_tex.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&gpu_tex.sampler),
                            },
                        ],
                    });
                    self.material_cache.insert(
                        item.material,
                        GpuMaterial {
                            uniform_buffer,
                            bind_group,
                            last_uniform: mat_uniform,
                            texture_handle: mat.base_color_texture,
                        },
                    );
                    self.material_cache.get_mut(&item.material).unwrap()
                };

                let pipeline = &self.scratch_pipelines[pipe_idx];
                pipe_idx += 1;

                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(1, &slot.bind_group, &[]);
                render_pass.set_bind_group(2, &gpu_mat.bind_group, &[]);
                render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                render_pass.set_index_buffer(mesh.index_buffer.slice(..), mesh.index_format);
                render_pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        Ok(())
    }
}
