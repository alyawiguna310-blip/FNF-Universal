//! Graphics quality settings, device capability limits and the shader effect
//! catalog for FNF Universal.
//!
//! This crate contains **no GPU code**. It decides *what* may be rendered
//! (presets, per-device limits, which effects are allowed) so that the future
//! renderer, the launcher UI and the mod adapters all agree. See
//! `docs/RENDERING_REFERENCE_VARVAR.md` and `docs/RENDERING_PLAN.md`.

pub mod device;
pub mod effects;
pub mod settings;

pub use device::{DeviceCaps, DeviceClass};
pub use effects::{allowed, find_by_shader_name, EffectCost, EffectInfo, EffectKind, UniformSpec, UniformType, CATALOG};
pub use settings::{
    AntiAliasing, CustomSettings, GraphicsSettings, LightingQuality, LoadOutcome, QualityPreset,
    ResolvedGraphics, SettingsError, ShadowQuality, TextureFilter, SETTINGS_VERSION,
};
