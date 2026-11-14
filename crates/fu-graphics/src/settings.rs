use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::device::{DeviceCaps, DeviceClass};

pub const SETTINGS_VERSION: u32 = 1;

const MIN_RENDER_SCALE: f32 = 0.25;
const MAX_RENDER_SCALE: f32 = 2.0;
const MIN_FPS_LIMIT: u16 = 15;
const MAX_FPS_LIMIT: u16 = 360;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QualityPreset {
    /// Also the fallback for anything unreadable: the safe choice.
    #[default]
    Low,
    High,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowQuality {
    Off,
    Low,
    Medium,
    High,
}

impl ShadowQuality {
    /// Shadow map edge length in texels. Initial budgets, to be tuned by
    /// measurement on real hardware.
    pub fn map_size(self) -> u32 {
        match self {
            ShadowQuality::Off => 0,
            ShadowQuality::Low => 512,
            ShadowQuality::Medium => 1024,
            ShadowQuality::High => 2048,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingQuality {
    Basic,
    Standard,
    Full,
}

impl LightingQuality {
    /// Initial budgets, to be tuned by measurement on real hardware.
    pub fn max_point_lights(self) -> u8 {
        match self {
            LightingQuality::Basic => 2,
            LightingQuality::Standard => 4,
            LightingQuality::Full => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiAliasing {
    Off,
    Msaa2,
    Msaa4,
    Msaa8,
}

impl AntiAliasing {
    pub fn samples(self) -> u8 {
        match self {
            AntiAliasing::Off => 1,
            AntiAliasing::Msaa2 => 2,
            AntiAliasing::Msaa4 => 4,
            AntiAliasing::Msaa8 => 8,
        }
    }

    /// Largest supported setting that does not exceed `samples`.
    pub fn from_samples_floor(samples: u8) -> Self {
        match samples {
            0 | 1 => AntiAliasing::Off,
            2 | 3 => AntiAliasing::Msaa2,
            4..=7 => AntiAliasing::Msaa4,
            _ => AntiAliasing::Msaa8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureFilter {
    Nearest,
    Linear,
    Anisotropic,
}

fn floor_anisotropy(n: u8) -> u8 {
    match n {
        0 | 1 => 1,
        2..=3 => 2,
        4..=7 => 4,
        8..=15 => 8,
        _ => 16,
    }
}

/// Every individually configurable option (the Custom preset edits these; Low
/// and High are fixed sets of the same values).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomSettings {
    pub three_d: bool,
    pub shadow_quality: ShadowQuality,
    pub bloom: bool,
    pub anti_aliasing: AntiAliasing,
    pub texture_filter: TextureFilter,
    pub anisotropy: u8,
    /// Full-screen passes that cost more than a cheap effect.
    pub post_processing: bool,
    pub render_scale: f32,
    pub lighting: LightingQuality,
    /// Master switch for shader effects (mods' own effects included).
    pub shader_effects: bool,
    /// `None` means follow the display.
    pub fps_limit: Option<u16>,
}

impl Default for CustomSettings {
    fn default() -> Self {
        Self::low()
    }
}

impl CustomSettings {
    pub fn low() -> Self {
        Self {
            three_d: true,
            shadow_quality: ShadowQuality::Off,
            bloom: false,
            anti_aliasing: AntiAliasing::Off,
            texture_filter: TextureFilter::Linear,
            anisotropy: 1,
            post_processing: false,
            render_scale: 0.75,
            lighting: LightingQuality::Basic,
            shader_effects: true,
            fps_limit: None,
        }
    }

    pub fn high() -> Self {
        Self {
            three_d: true,
            shadow_quality: ShadowQuality::High,
            bloom: true,
            anti_aliasing: AntiAliasing::Msaa4,
            texture_filter: TextureFilter::Anisotropic,
            anisotropy: 8,
            post_processing: true,
            render_scale: 1.0,
            lighting: LightingQuality::Full,
            shader_effects: true,
            fps_limit: None,
        }
    }

    /// Brings out-of-range values (from a hand-edited file, say) back into range.
    fn sanitized(&self) -> Self {
        let mut s = self.clone();
        s.render_scale = if s.render_scale.is_finite() {
            s.render_scale.clamp(MIN_RENDER_SCALE, MAX_RENDER_SCALE)
        } else {
            1.0
        };
        s.anisotropy = floor_anisotropy(s.anisotropy);
        s.fps_limit = s.fps_limit.map(|f| f.clamp(MIN_FPS_LIMIT, MAX_FPS_LIMIT));
        s
    }

    /// Never exceeds what the device reports. Mobile is held to a lower
    /// ceiling even if the user picked High by hand.
    fn limited_by(&self, caps: &DeviceCaps) -> Self {
        let mut v = self.clone();
        let mobile = caps.class == DeviceClass::Mobile;

        if !caps.supports_shadow_maps {
            v.shadow_quality = ShadowQuality::Off;
        } else if mobile && v.shadow_quality > ShadowQuality::Medium {
            v.shadow_quality = ShadowQuality::Medium;
        }

        let mut msaa_cap = caps.max_msaa_samples.max(1);
        if mobile {
            msaa_cap = msaa_cap.min(4);
        }
        v.anti_aliasing = AntiAliasing::from_samples_floor(v.anti_aliasing.samples().min(msaa_cap));

        let mut aniso_cap = caps.max_anisotropy.max(1);
        if mobile {
            aniso_cap = aniso_cap.min(4);
            v.render_scale = v.render_scale.min(1.0);
        }
        v.anisotropy = floor_anisotropy(v.anisotropy.min(aniso_cap));
        v
    }
}

/// What is saved to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    pub version: u32,
    pub preset: QualityPreset,
    pub custom: CustomSettings,
    /// Accessibility. The reference mod scales its bloom by 0.25 when its own
    /// flashing option is on, so this is carried here for every renderer effect.
    pub reduce_flashing: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            preset: QualityPreset::Low,
            custom: CustomSettings::low(),
            reduce_flashing: false,
        }
    }
}

/// The values the renderer actually uses: preset applied, ranges fixed, and
/// limited by the device.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGraphics {
    pub preset: QualityPreset,
    pub three_d: bool,
    pub shadow_quality: ShadowQuality,
    pub bloom: bool,
    pub anti_aliasing: AntiAliasing,
    pub texture_filter: TextureFilter,
    pub anisotropy: u8,
    pub post_processing: bool,
    pub render_scale: f32,
    pub lighting: LightingQuality,
    pub shader_effects: bool,
    pub fps_limit: Option<u16>,
    pub reduce_flashing: bool,
}

impl ResolvedGraphics {
    /// Size of the world render target for a window of `base` pixels.
    pub fn render_target_size(&self, base_width: u32, base_height: u32, caps: &DeviceCaps) -> (u32, u32) {
        let max = caps.max_texture_size.max(1);
        let scale = |n: u32| -> u32 {
            let v = (n as f32 * self.render_scale).round();
            (v.max(1.0) as u32).min(max)
        };
        (scale(base_width), scale(base_height))
    }

    /// Multiplier for flashing effects such as bloom pulses.
    pub fn flashing_scale(&self) -> f32 {
        if self.reduce_flashing {
            0.25
        } else {
            1.0
        }
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings file is not valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("settings were saved by a newer version ({found}); this build understands up to {supported}")]
    TooNew { found: u32, supported: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadOutcome {
    pub settings: GraphicsSettings,
    pub warnings: Vec<String>,
}

impl GraphicsSettings {
    /// Desktop GPUs that can do shadow maps and 4x MSAA start on High. Mobile
    /// and anything weaker start on Low. High is never chosen for a mobile
    /// device automatically.
    pub fn default_for(caps: &DeviceCaps) -> Self {
        let capable = caps.class == DeviceClass::Desktop
            && caps.supports_shadow_maps
            && caps.max_msaa_samples >= 4;
        Self {
            preset: if capable { QualityPreset::High } else { QualityPreset::Low },
            ..Self::default()
        }
    }

    pub fn resolve(&self, caps: &DeviceCaps) -> ResolvedGraphics {
        let base = match self.preset {
            QualityPreset::Low => CustomSettings::low(),
            QualityPreset::High => CustomSettings::high(),
            QualityPreset::Custom => self.custom.sanitized(),
        };
        let v = base.limited_by(caps);
        let anisotropy = if v.texture_filter == TextureFilter::Anisotropic {
            v.anisotropy
        } else {
            1
        };
        ResolvedGraphics {
            preset: self.preset,
            three_d: v.three_d,
            shadow_quality: v.shadow_quality,
            bloom: v.bloom,
            anti_aliasing: v.anti_aliasing,
            texture_filter: v.texture_filter,
            anisotropy,
            post_processing: v.post_processing,
            render_scale: v.render_scale,
            lighting: v.lighting,
            shader_effects: v.shader_effects,
            fps_limit: v.fps_limit,
            reduce_flashing: self.reduce_flashing,
        }
    }

    pub fn to_json(&self) -> Result<String, SettingsError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self, SettingsError> {
        let mut settings: GraphicsSettings = serde_json::from_str(text)?;
        if settings.version > SETTINGS_VERSION {
            return Err(SettingsError::TooNew {
                found: settings.version,
                supported: SETTINGS_VERSION,
            });
        }
        settings.custom = settings.custom.sanitized();
        Ok(settings)
    }

    /// Reads saved settings, falling back to the device default when there is
    /// nothing saved or the file cannot be used. Never fails.
    pub fn load_or_default(saved: Option<&str>, caps: &DeviceCaps) -> LoadOutcome {
        match saved {
            None => LoadOutcome {
                settings: Self::default_for(caps),
                warnings: Vec::new(),
            },
            Some(text) => match Self::from_json(text) {
                Ok(settings) => LoadOutcome {
                    settings,
                    warnings: Vec::new(),
                },
                Err(e) => LoadOutcome {
                    settings: Self::default_for(caps),
                    warnings: vec![format!("{e}. Defaults for this device were used.")],
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop() -> DeviceCaps {
        DeviceCaps::desktop_fixture()
    }
    fn mobile() -> DeviceCaps {
        DeviceCaps::mobile_fixture()
    }
    fn with_preset(preset: QualityPreset) -> GraphicsSettings {
        GraphicsSettings {
            preset,
            ..GraphicsSettings::default()
        }
    }

    #[test]
    fn low_turns_off_the_expensive_things() {
        let r = with_preset(QualityPreset::Low).resolve(&desktop());
        assert_eq!(r.shadow_quality, ShadowQuality::Off);
        assert!(!r.bloom);
        assert!(!r.post_processing);
        assert!(r.render_scale < 1.0);
        assert_eq!(r.anti_aliasing, AntiAliasing::Off);
        assert_eq!(r.lighting, LightingQuality::Basic);
        assert!(r.three_d, "basic 3D stays available on Low");
    }

    #[test]
    fn high_turns_everything_on_for_a_capable_desktop() {
        let r = with_preset(QualityPreset::High).resolve(&desktop());
        assert_eq!(r.shadow_quality, ShadowQuality::High);
        assert!(r.bloom && r.post_processing && r.shader_effects);
        assert_eq!(r.anti_aliasing, AntiAliasing::Msaa4);
        assert_eq!(r.anisotropy, 8);
        assert_eq!(r.lighting, LightingQuality::Full);
        assert!((r.render_scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn high_is_never_chosen_for_mobile_automatically() {
        assert_eq!(GraphicsSettings::default_for(&mobile()).preset, QualityPreset::Low);
        // A desktop that cannot do shadow maps or 4x MSAA is not "capable" either.
        let weak = DeviceCaps::conservative(DeviceClass::Desktop);
        assert_eq!(GraphicsSettings::default_for(&weak).preset, QualityPreset::Low);
        assert_eq!(GraphicsSettings::default_for(&desktop()).preset, QualityPreset::High);
    }

    #[test]
    fn a_user_can_pick_high_on_mobile_but_it_is_held_to_a_lower_ceiling() {
        let r = with_preset(QualityPreset::High).resolve(&mobile());
        assert_eq!(r.shadow_quality, ShadowQuality::Medium);
        assert!(r.anti_aliasing <= AntiAliasing::Msaa4);
        assert!(r.anisotropy <= 4);
        assert!(r.render_scale <= 1.0);
    }

    #[test]
    fn the_device_always_wins_over_the_request() {
        let weak = DeviceCaps::conservative(DeviceClass::Desktop);
        let r = with_preset(QualityPreset::High).resolve(&weak);
        assert_eq!(r.shadow_quality, ShadowQuality::Off);
        assert_eq!(r.anti_aliasing, AntiAliasing::Off);
        assert_eq!(r.anisotropy, 1);
    }

    #[test]
    fn custom_values_are_brought_into_range() {
        let mut s = with_preset(QualityPreset::Custom);
        s.custom.render_scale = 9.0;
        s.custom.anisotropy = 5;
        s.custom.fps_limit = Some(1);
        let r = s.resolve(&desktop());
        assert!((r.render_scale - 2.0).abs() < f32::EPSILON);
        assert_eq!(r.fps_limit, Some(15));

        s.custom.fps_limit = Some(1000);
        s.custom.render_scale = f32::NAN;
        s.custom.texture_filter = TextureFilter::Anisotropic;
        let r = s.resolve(&desktop());
        assert_eq!(r.fps_limit, Some(360));
        assert!((r.render_scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(r.anisotropy, 4);
    }

    #[test]
    fn custom_render_scale_is_capped_on_mobile() {
        let mut s = with_preset(QualityPreset::Custom);
        s.custom.render_scale = 2.0;
        assert!(s.resolve(&mobile()).render_scale <= 1.0);
    }

    #[test]
    fn render_target_size_scales_and_respects_the_texture_limit() {
        let low = with_preset(QualityPreset::Low).resolve(&desktop());
        assert_eq!(low.render_target_size(1920, 1080, &desktop()), (1440, 810));
        let high = with_preset(QualityPreset::High).resolve(&desktop());
        assert_eq!(high.render_target_size(1920, 1080, &desktop()), (1920, 1080));

        let mut s = with_preset(QualityPreset::Custom);
        s.custom.render_scale = 2.0;
        let caps = DeviceCaps::conservative(DeviceClass::Desktop); // 4096 max
        let r = s.resolve(&caps);
        assert_eq!(r.render_target_size(4000, 100, &caps).0, 4096);
    }

    #[test]
    fn reduce_flashing_quarters_flash_effects() {
        let mut s = with_preset(QualityPreset::High);
        assert!((s.resolve(&desktop()).flashing_scale() - 1.0).abs() < f32::EPSILON);
        s.reduce_flashing = true;
        assert!((s.resolve(&desktop()).flashing_scale() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let mut s = with_preset(QualityPreset::Custom);
        s.custom.bloom = true;
        s.custom.fps_limit = Some(60);
        s.reduce_flashing = true;
        let json = s.to_json().unwrap();
        assert_eq!(GraphicsSettings::from_json(&json).unwrap(), s);
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let s = GraphicsSettings::from_json(r#"{"preset":"high","mystery":1}"#).unwrap();
        assert_eq!(s.preset, QualityPreset::High);
        assert_eq!(s.custom, CustomSettings::low());
    }

    #[test]
    fn unusable_files_fall_back_to_the_device_default_with_a_warning() {
        let broken = GraphicsSettings::load_or_default(Some("{ not json"), &mobile());
        assert_eq!(broken.settings.preset, QualityPreset::Low);
        assert_eq!(broken.warnings.len(), 1);

        let newer = GraphicsSettings::load_or_default(Some(r#"{"version":99}"#), &desktop());
        assert_eq!(newer.settings.preset, QualityPreset::High);
        assert_eq!(newer.warnings.len(), 1);

        let none = GraphicsSettings::load_or_default(None, &mobile());
        assert!(none.warnings.is_empty());
        assert_eq!(none.settings.preset, QualityPreset::Low);
    }

    #[test]
    fn shadow_and_light_budgets_grow_with_quality() {
        assert!(ShadowQuality::Low.map_size() < ShadowQuality::High.map_size());
        assert_eq!(ShadowQuality::Off.map_size(), 0);
        assert!(LightingQuality::Basic.max_point_lights() < LightingQuality::Full.max_point_lights());
        assert_eq!(AntiAliasing::from_samples_floor(3), AntiAliasing::Msaa2);
    }
}
