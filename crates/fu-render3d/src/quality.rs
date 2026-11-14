//! Graphics quality configuration. Presets are starting points; a backend's
//! reported capabilities can still clamp them (see `BackendCapabilities`).

use crate::{RenderError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Desktop,
    Mobile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualitySettings {
    pub max_lights: usize,
    /// Shadow map edge in pixels (power of two) or `None` to disable shadows.
    pub shadow_map_size: Option<u32>,
    pub msaa_samples: u8,
    pub render_scale: f32,
    pub post_effects: bool,
    pub max_texture_size: u32,
}

impl QualitySettings {
    pub fn preset(level: QualityLevel, platform: Platform) -> Self {
        let base = match level {
            QualityLevel::Low => Self {
                max_lights: 2,
                shadow_map_size: None,
                msaa_samples: 1,
                render_scale: 0.75,
                post_effects: false,
                max_texture_size: 1024,
            },
            QualityLevel::Medium => Self {
                max_lights: 4,
                shadow_map_size: Some(1024),
                msaa_samples: 1,
                render_scale: 1.0,
                post_effects: true,
                max_texture_size: 2048,
            },
            QualityLevel::High => Self {
                max_lights: 8,
                shadow_map_size: Some(2048),
                msaa_samples: 4,
                render_scale: 1.0,
                post_effects: true,
                max_texture_size: 4096,
            },
        };
        base.clamped_for(platform)
    }

    /// Conservative ceilings for mobile GPUs and memory budgets.
    pub fn clamped_for(mut self, platform: Platform) -> Self {
        if platform == Platform::Mobile {
            self.max_lights = self.max_lights.min(4);
            self.shadow_map_size = self.shadow_map_size.map(|s| s.min(1024));
            self.msaa_samples = self.msaa_samples.min(4);
            self.max_texture_size = self.max_texture_size.min(2048);
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.max_lights > 64 {
            return Err(RenderError::invalid("quality", "max_lights must be <= 64"));
        }
        if let Some(size) = self.shadow_map_size {
            if !size.is_power_of_two() || !(256..=8192).contains(&size) {
                return Err(RenderError::invalid(
                    "quality",
                    "shadow_map_size must be a power of two in 256..=8192",
                ));
            }
        }
        if !matches!(self.msaa_samples, 1 | 2 | 4 | 8) {
            return Err(RenderError::invalid(
                "quality",
                "msaa_samples must be 1, 2, 4 or 8",
            ));
        }
        if !(self.render_scale.is_finite() && (0.25..=2.0).contains(&self.render_scale)) {
            return Err(RenderError::invalid(
                "quality",
                "render_scale must be in 0.25..=2.0",
            ));
        }
        if !self.max_texture_size.is_power_of_two() || self.max_texture_size < 64 {
            return Err(RenderError::invalid(
                "quality",
                "max_texture_size must be a power of two >= 64",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_validate_on_both_platforms() {
        for level in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
            for platform in [Platform::Desktop, Platform::Mobile] {
                assert!(QualitySettings::preset(level, platform).validate().is_ok());
            }
        }
    }

    #[test]
    fn mobile_high_is_capped() {
        let q = QualitySettings::preset(QualityLevel::High, Platform::Mobile);
        assert_eq!(q.max_lights, 4);
        assert_eq!(q.shadow_map_size, Some(1024));
        assert_eq!(q.msaa_samples, 4);
        assert_eq!(q.max_texture_size, 2048);
        let d = QualitySettings::preset(QualityLevel::High, Platform::Desktop);
        assert_eq!(d.max_lights, 8);
    }

    #[test]
    fn rejects_invalid_values() {
        let mut q = QualitySettings::preset(QualityLevel::Medium, Platform::Desktop);
        q.msaa_samples = 3;
        assert!(q.validate().is_err());
        let mut q = QualitySettings::preset(QualityLevel::Medium, Platform::Desktop);
        q.shadow_map_size = Some(1000);
        assert!(q.validate().is_err());
    }

    #[test]
    fn json_round_trip_uses_camel_case() {
        let q = QualitySettings::preset(QualityLevel::Medium, Platform::Desktop);
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains("shadowMapSize"));
        let back: QualitySettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, q);
    }
}
