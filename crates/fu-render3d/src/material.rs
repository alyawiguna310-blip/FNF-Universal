use crate::resources::Handle;
use crate::shader::{ShaderId, ShaderParams};
use crate::{RenderError, Result};
use serde::{Deserialize, Serialize};

/// CPU-side RGBA8 texture. GPU upload belongs to a backend.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureData {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

impl TextureData {
    pub fn new(name: impl Into<String>, width: u32, height: u32, rgba8: Vec<u8>) -> Result<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if width == 0 || height == 0 || expected != Some(rgba8.len()) {
            return Err(RenderError::invalid(
                "texture",
                "size must be non-zero and data length must equal width*height*4",
            ));
        }
        Ok(Self {
            name: name.into(),
            width,
            height,
            rgba8,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    Opaque,
    Mask { cutoff: f32 },
    Blend,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub shader: ShaderId,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub base_color_texture: Option<Handle<TextureData>>,
    pub alpha_mode: AlphaMode,
    pub double_sided: bool,
    pub params: ShaderParams,
}

impl Material {
    pub fn new(name: impl Into<String>, shader: ShaderId) -> Self {
        Self {
            name: name.into(),
            shader,
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            emissive: [0.0; 3],
            base_color_texture: None,
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
            params: ShaderParams::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !self.base_color.iter().copied().all(unit) {
            return Err(RenderError::invalid(
                "material",
                "base_color must be in 0..=1",
            ));
        }
        if !unit(self.metallic) || !unit(self.roughness) {
            return Err(RenderError::invalid(
                "material",
                "metallic and roughness must be in 0..=1",
            ));
        }
        if !self.emissive.iter().all(|v| v.is_finite() && *v >= 0.0) {
            return Err(RenderError::invalid(
                "material",
                "emissive must be finite and >= 0",
            ));
        }
        if let AlphaMode::Mask { cutoff } = self.alpha_mode {
            if !unit(cutoff) {
                return Err(RenderError::invalid(
                    "material",
                    "mask cutoff must be in 0..=1",
                ));
            }
        }
        Ok(())
    }

    pub fn is_blended(&self) -> bool {
        matches!(self.alpha_mode, AlphaMode::Blend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate() {
        assert!(Material::new("m", ShaderId::lit()).validate().is_ok());
    }

    #[test]
    fn rejects_out_of_range_values() {
        let mut m = Material::new("m", ShaderId::lit());
        m.roughness = 2.0;
        assert!(m.validate().is_err());
        let mut m = Material::new("m", ShaderId::lit());
        m.alpha_mode = AlphaMode::Mask { cutoff: -0.1 };
        assert!(m.validate().is_err());
    }

    #[test]
    fn texture_length_is_checked() {
        assert!(TextureData::new("t", 2, 2, vec![0; 16]).is_ok());
        assert!(TextureData::new("t", 2, 2, vec![0; 15]).is_err());
        assert!(TextureData::new("t", 0, 2, vec![]).is_err());
    }
}
