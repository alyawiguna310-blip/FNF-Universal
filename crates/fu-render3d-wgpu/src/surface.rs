//! Desktop surface presentation path for `fu-render3d-wgpu` using `winit 0.30` and `wgpu 24`.

use std::sync::Arc;
use winit::window::Window;

use fu_render3d::backend::Viewport;
use fu_render3d::camera::Camera;
use fu_render3d::renderer::Renderer3D;
use fu_render3d::{RenderError, Result};

use crate::backend::WgpuBackend;

/// A rectangle of the window surface in physical pixels. The origin is the
/// top-left corner and y grows downwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One draw of the scene inside [`WindowedRenderer::render_passes`]: the camera it
/// is seen through and the layers it shows (`node.layer_mask & layer_mask != 0`).
#[derive(Debug, Clone, Copy)]
pub struct ScenePass<'a> {
    pub camera: &'a Camera,
    pub layer_mask: u32,
}

/// Selects an optimal surface format from the list of supported surface formats.
///
/// If `prefer_srgb` is true, an sRGB format (e.g. `Bgra8UnormSrgb` or `Rgba8UnormSrgb`) is preferred.
/// If no sRGB candidate is available, or if `prefer_srgb` is false, it returns the first candidate.
/// Returns `None` if `candidates` is empty.
pub(crate) fn select_surface_format(
    candidates: &[wgpu::TextureFormat],
    prefer_srgb: bool,
) -> Option<wgpu::TextureFormat> {
    if candidates.is_empty() {
        return None;
    }
    if prefer_srgb {
        candidates
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| candidates.first().copied())
    } else {
        candidates.first().copied()
    }
}

/// Selects a supported present mode based on VSync preference.
///
/// - If `vsync` is true, returns `wgpu::PresentMode::Fifo`.
/// - If `vsync` is false, prefers `Mailbox`, then `Immediate`, and falls back to `Fifo`.
pub(crate) fn select_present_mode(
    supported: &[wgpu::PresentMode],
    vsync: bool,
) -> wgpu::PresentMode {
    if vsync {
        wgpu::PresentMode::Fifo
    } else if supported.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else if supported.contains(&wgpu::PresentMode::Immediate) {
        wgpu::PresentMode::Immediate
    } else {
        wgpu::PresentMode::Fifo
    }
}

/// Renderer-side desktop presentation manager.
///
/// Wraps an `Arc<winit::window::Window>`, a `wgpu::Surface<'static>`, and a configured
/// [`WgpuBackend`]. Handles window resizing, VSync configuration, zero-size suspension,
/// and surface error recovery (`Lost`, `Outdated`, `Timeout`, `OutOfMemory`).
pub struct WindowedRenderer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    backend: WgpuBackend,
    vsync: bool,
    width: u32,
    height: u32,
    configured: bool,
}

impl WindowedRenderer {
    /// Creates a new [`WindowedRenderer`] for a desktop window.
    pub fn new(window: Arc<Window>, vsync: bool) -> Result<Self> {
        pollster::block_on(Self::new_async(window, vsync))
    }

    /// Asynchronously creates a new [`WindowedRenderer`] for a desktop window.
    pub async fn new_async(window: Arc<Window>, vsync: bool) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| RenderError::Backend(format!("failed to create WGPU surface: {e}")))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| {
                RenderError::Backend("failed to find suitable GPU adapter for surface".into())
            })?;

        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("fu-render3d-wgpu Windowed Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| RenderError::Backend(format!("failed to create device: {e}")))?;

        let adapter = Arc::new(adapter);
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let surface_caps = surface.get_capabilities(&adapter);
        let format = select_surface_format(&surface_caps.formats, true)
            .ok_or_else(|| RenderError::Backend("no compatible surface format found".into()))?;
        let present_mode = select_present_mode(&surface_caps.present_modes, vsync);

        let inner_size = window.inner_size();
        let width = inner_size.width;
        let height = inner_size.height;

        let alpha_mode = surface_caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let configured = if width > 0 && height > 0 {
            surface.configure(&device, &surface_config);
            true
        } else {
            false
        };

        let backend = WgpuBackend::from_parts(instance, adapter, device, queue, format)?;

        Ok(Self {
            window,
            surface,
            surface_config,
            backend,
            vsync,
            width,
            height,
            configured,
        })
    }

    /// Access the underlying [`winit::window::Window`].
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Access the underlying [`WgpuBackend`].
    pub fn backend(&self) -> &WgpuBackend {
        &self.backend
    }

    /// Mutably access the underlying [`WgpuBackend`].
    pub fn backend_mut(&mut self) -> &mut WgpuBackend {
        &mut self.backend
    }

    /// Current surface dimensions `(width, height)`.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Current VSync setting.
    pub fn vsync(&self) -> bool {
        self.vsync
    }

    /// Updates the VSync setting and reconfigures the surface if necessary.
    pub fn set_vsync(&mut self, vsync: bool) {
        if self.vsync == vsync {
            return;
        }
        self.vsync = vsync;
        let surface_caps = self.surface.get_capabilities(&self.backend.adapter);
        let new_mode = select_present_mode(&surface_caps.present_modes, vsync);
        if self.surface_config.present_mode != new_mode {
            self.surface_config.present_mode = new_mode;
            if self.configured && self.width > 0 && self.height > 0 {
                self.surface
                    .configure(&self.backend.device, &self.surface_config);
            }
        }
    }

    /// Resizes the presentation surface.
    ///
    /// If the new size is zero in either dimension, surface reconfiguration is deferred
    /// and rendering will be suspended until a non-zero size is set.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;

        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface
                .configure(&self.backend.device, &self.surface_config);
            self.configured = true;
        }
    }

    /// Acquires the surface texture to draw into.
    ///
    /// - Returns `Ok(None)` when rendering should be skipped: the window has zero
    ///   width or height, or acquiring the texture timed out.
    /// - Reconfigures the surface on `Lost` or `Outdated` and retries once.
    /// - Returns a clear fatal error on `OutOfMemory` and on other surface errors.
    fn acquire_frame(&mut self) -> Result<Option<wgpu::SurfaceTexture>> {
        let (w, h) = (self.width, self.height);
        if w == 0 || h == 0 {
            // Window is minimized or zero-sized; suspend rendering safely.
            return Ok(None);
        }

        if !self.configured {
            self.surface_config.width = w;
            self.surface_config.height = h;
            self.surface
                .configure(&self.backend.device, &self.surface_config);
            self.configured = true;
        }

        let surface_texture = match self.surface.get_current_texture() {
            Ok(tex) => tex,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // Reconfigure and retry once
                self.surface
                    .configure(&self.backend.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    Ok(tex) => tex,
                    Err(wgpu::SurfaceError::Timeout) => return Ok(None),
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        return Err(RenderError::Backend(
                            "out of memory while acquiring surface texture".into(),
                        ));
                    }
                    Err(e) => {
                        return Err(RenderError::Backend(format!(
                            "surface recovery failed: {e}"
                        )));
                    }
                }
            }
            Err(wgpu::SurfaceError::Timeout) => {
                // Skip frame safely
                return Ok(None);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err(RenderError::Backend(
                    "out of memory while acquiring surface texture".into(),
                ));
            }
            Err(e) => {
                return Err(RenderError::Backend(format!("surface error: {e}")));
            }
        };
        Ok(Some(surface_texture))
    }

    /// Renders a frame from a [`Renderer3D`] and [`Camera`] to the window surface.
    ///
    /// Requirements satisfied:
    /// - Suspends rendering when the window has zero width or height (returns `Ok(())`).
    /// - Reconfigures on `Lost` or `Outdated` errors and retries once.
    /// - Skips the frame safely on `Timeout`.
    /// - Returns a clear fatal error on `OutOfMemory`.
    pub fn render_frame(&mut self, renderer: &Renderer3D, camera: &Camera) -> Result<()> {
        let Some(surface_texture) = self.acquire_frame()? else {
            return Ok(());
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.backend.target_view_override = Some(view);

        let viewport = Viewport {
            width: self.width,
            height: self.height,
        };

        let render_result = renderer.render(camera, viewport, &mut self.backend);

        self.backend.target_view_override = None;

        render_result?;
        surface_texture.present();
        Ok(())
    }

    /// Renders several passes of one scene into a single frame.
    ///
    /// Each [`ScenePass`] draws the layers it asks for through its own camera. The
    /// first pass clears the whole surface; every later pass draws over what the
    /// earlier ones left (with its own depth buffer), so a world pass followed by a
    /// screen-space pass keeps the second from being moved or zoomed by the first
    /// camera.
    ///
    /// With a `region`, every pass draws only into that rectangle of the surface
    /// and its projection takes the region's aspect ratio, while the rest of the
    /// surface keeps the clear colour (letterbox or pillarbox bars). Without one,
    /// the passes cover the whole surface at its own aspect ratio.
    ///
    /// Suspends rendering and recovers from surface errors like
    /// [`WindowedRenderer::render_frame`]. Nothing is drawn or presented for an
    /// empty `passes` slice.
    pub fn render_passes(
        &mut self,
        renderer: &Renderer3D,
        passes: &[ScenePass<'_>],
        region: Option<PixelRegion>,
    ) -> Result<()> {
        if passes.is_empty() {
            return Ok(());
        }
        let Some(surface_texture) = self.acquire_frame()? else {
            return Ok(());
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.backend.target_view_override = Some(view);

        let target = Viewport {
            width: self.width,
            height: self.height,
        };
        let aspect = match region {
            Some(region) if region.height > 0 => region.width as f32 / region.height as f32,
            _ => target.aspect(),
        };
        self.backend.render_region = region.map(|r| [r.x, r.y, r.width, r.height]);

        let mut result: Result<()> = Ok(());
        for (index, pass) in passes.iter().enumerate() {
            self.backend.preserve_color = index > 0;
            result =
                renderer.render_masked(pass.camera, target, aspect, pass.layer_mask, &mut self.backend);
            if result.is_err() {
                break;
            }
        }

        self.backend.target_view_override = None;
        self.backend.render_region = None;
        self.backend.preserve_color = false;

        result?;
        surface_texture.present();
        Ok(())
    }
}
