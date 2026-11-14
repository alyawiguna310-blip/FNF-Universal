//! The seam between this crate and a GPU implementation.
//!
//! A backend (for example a future wgpu one) receives an immutable
//! `FramePacket` per frame. It owns GPU resources and caches keyed by handles
//! and shader revisions; this crate never touches the GPU.

use crate::light::Light;
use crate::material::{Material, TextureData};
use crate::mesh::MeshData;
use crate::quality::QualitySettings;
use crate::queue::DrawQueue;
use crate::resources::Store;
use crate::shader::ShaderRegistry;
use crate::Result;
use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

/// How the 3D output relates to the host's 2D layer. The host/engine performs
/// the actual compositing; this only states the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Composition {
    /// 3D is drawn first; the host draws 2D (UI, notes, sprites) over it.
    #[default]
    Scene3dUnder2d,
    /// 2D is drawn first; 3D is drawn over it.
    Scene3dOver2d,
    /// 3D output replaces the frame.
    Scene3dOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub max_texture_size: u32,
    pub max_msaa_samples: u8,
    pub supports_shadows: bool,
}

impl BackendCapabilities {
    /// Reduce requested quality to what the backend can actually do.
    pub fn clamp_quality(&self, quality: &QualitySettings) -> QualitySettings {
        let mut out = quality.clone();
        out.max_texture_size = out.max_texture_size.min(self.max_texture_size);
        while out.msaa_samples > 1 && out.msaa_samples > self.max_msaa_samples {
            out.msaa_samples /= 2;
        }
        if !self.supports_shadows {
            out.shadow_map_size = None;
        }
        out
    }
}

pub struct FramePacket<'a> {
    pub viewport: Viewport,
    pub composition: Composition,
    pub view: Mat4,
    pub projection: Mat4,
    pub camera_position: Vec3,
    /// Already clamped to the backend's capabilities.
    pub quality: &'a QualitySettings,
    pub queue: &'a DrawQueue,
    pub lights: Vec<&'a Light>,
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
    pub meshes: &'a Store<MeshData>,
    pub materials: &'a Store<Material>,
    pub textures: &'a Store<TextureData>,
    pub shaders: &'a ShaderRegistry,
}

pub trait RenderBackend {
    fn name(&self) -> &str;
    fn capabilities(&self) -> BackendCapabilities;
    fn render(&mut self, frame: &FramePacket<'_>) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::{Platform, QualityLevel};

    #[test]
    fn clamp_quality_respects_backend_limits() {
        let caps = BackendCapabilities {
            max_texture_size: 1024,
            max_msaa_samples: 2,
            supports_shadows: false,
        };
        let q = QualitySettings::preset(QualityLevel::High, Platform::Desktop);
        let clamped = caps.clamp_quality(&q);
        assert_eq!(clamped.max_texture_size, 1024);
        assert_eq!(clamped.msaa_samples, 2);
        assert_eq!(clamped.shadow_map_size, None);
        assert_eq!(clamped.max_lights, q.max_lights);
    }

    #[test]
    fn viewport_aspect() {
        assert_eq!(
            Viewport {
                width: 1920,
                height: 1080
            }
            .aspect(),
            1920.0 / 1080.0
        );
    }
}
