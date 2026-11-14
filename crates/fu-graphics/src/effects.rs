//! Catalog of the shader effects found in the Vs Var Overhaul [Demo] mod.
//!
//! VERIFIED from the mod's .frag files: shader names and every uniform name and
//! type (see docs/RENDERING_REFERENCE_VARVAR.md).
//!
//! INFERRED, not measured: the `cost` of each effect. It is an estimate from
//! the amount of work in each shader (loops, layers) and must be replaced by
//! real frame-time measurements once a renderer exists.
//!
//! The original shaders are written in an OpenFL/Flixel GLSL dialect and cannot
//! run unchanged here. This catalog only describes their interface so the
//! settings, mod events and a future wgpu/WGSL port agree on names and
//! parameters.

use crate::settings::ResolvedGraphics;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectKind {
    AdjustColor,
    Bloom,
    ColoredVignette,
    DarkenButLights,
    Glitch,
    Heatwave,
    LineBoil,
    Saturation,
    Snow,
    StaticNoise,
    TapeNoise,
    Warp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniformType {
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    Sampler2D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniformSpec {
    pub name: &'static str,
    pub ty: UniformType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EffectCost {
    Cheap,
    Moderate,
    Expensive,
}

#[derive(Debug, Clone, Copy)]
pub struct EffectInfo {
    pub kind: EffectKind,
    /// The shader's file name without extension, as a mod refers to it.
    pub shader_name: &'static str,
    pub uniforms: &'static [UniformSpec],
    /// Estimate. See the module docs.
    pub cost: EffectCost,
}

const fn u(name: &'static str, ty: UniformType) -> UniformSpec {
    UniformSpec { name, ty }
}

use UniformType::{Bool, Float, Int, Sampler2D, Vec2, Vec3, Vec4};

static ADJUST_COLOR: [UniformSpec; 4] = [
    u("hue", Float),
    u("saturation", Float),
    u("brightness", Float),
    u("contrast", Float),
];
static BLOOM: [UniformSpec; 4] = [
    u("brightness", Float),
    u("directions", Float),
    u("quality", Int),
    u("size", Float),
];
static COLORED_VIGNETTE: [UniformSpec; 4] = [
    u("color", Vec3),
    u("amount", Float),
    u("strength", Float),
    // Spelled this way in the original shader.
    u("transperency", Bool),
];
static DARKEN_BUT_LIGHTS: [UniformSpec; 5] = [
    u("strength", Float),
    u("threshold", Float),
    u("tint", Vec4),
    u("gradientY", Float),
    u("gradientHeight", Float),
];
static GLITCH: [UniformSpec; 3] = [u("iTime", Float), u("AMT", Float), u("SPEED", Float)];
static HEATWAVE: [UniformSpec; 4] = [
    u("time", Float),
    u("strength", Float),
    u("speed", Float),
    u("even", Bool),
];
static LINE_BOIL: [UniformSpec; 5] = [
    u("time", Float),
    u("strength", Float),
    u("fps", Float),
    u("scale", Vec2),
    u("noise", Sampler2D),
];
static SATURATION: [UniformSpec; 2] = [u("sat", Float), u("contrast", Float)];
static SNOW: [UniformSpec; 14] = [
    u("time", Float),
    u("res", Vec2),
    u("cameraZoom", Float),
    u("cameraPosition", Vec2),
    u("STARTING_LAYERS", Int),
    u("flipY", Bool),
    u("snowMeltRect", Vec4),
    u("snowMelts", Bool),
    u("pixely", Bool),
    u("BRIGHT", Float),
    u("LAYERS", Int),
    u("DEPTH", Float),
    u("WIDTH", Float),
    u("SPEED", Float),
];
static STATIC_NOISE: [UniformSpec; 2] = [u("time", Float), u("strength", Float)];
static TAPE_NOISE: [UniformSpec; 3] = [u("time", Float), u("res", Vec2), u("strength", Float)];
static WARP: [UniformSpec; 1] = [u("distortion", Float)];

pub static CATALOG: [EffectInfo; 12] = [
    EffectInfo { kind: EffectKind::AdjustColor, shader_name: "adjustColor", uniforms: &ADJUST_COLOR, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::Bloom, shader_name: "bloom", uniforms: &BLOOM, cost: EffectCost::Moderate },
    EffectInfo { kind: EffectKind::ColoredVignette, shader_name: "coloredVignette", uniforms: &COLORED_VIGNETTE, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::DarkenButLights, shader_name: "darkenbutlights", uniforms: &DARKEN_BUT_LIGHTS, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::Glitch, shader_name: "glitching", uniforms: &GLITCH, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::Heatwave, shader_name: "heatwave", uniforms: &HEATWAVE, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::LineBoil, shader_name: "lineboil", uniforms: &LINE_BOIL, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::Saturation, shader_name: "saturation", uniforms: &SATURATION, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::Snow, shader_name: "snow", uniforms: &SNOW, cost: EffectCost::Expensive },
    EffectInfo { kind: EffectKind::StaticNoise, shader_name: "static", uniforms: &STATIC_NOISE, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::TapeNoise, shader_name: "tapenoise", uniforms: &TAPE_NOISE, cost: EffectCost::Cheap },
    EffectInfo { kind: EffectKind::Warp, shader_name: "warp", uniforms: &WARP, cost: EffectCost::Cheap },
];

/// Looks an effect up by the shader name a mod uses (case-insensitive).
pub fn find_by_shader_name(name: &str) -> Option<&'static EffectInfo> {
    CATALOG
        .iter()
        .find(|e| e.shader_name.eq_ignore_ascii_case(name))
}

impl EffectKind {
    pub fn info(self) -> Option<&'static EffectInfo> {
        CATALOG.iter().find(|e| e.kind == self)
    }
}

/// Whether `kind` may run under the resolved settings.
///
/// - Everything is off when shader effects are off.
/// - Cheap effects run whenever shader effects are on (so Low keeps them).
/// - Moderate and Expensive effects also need post-processing.
/// - Bloom additionally needs the bloom switch.
pub fn allowed(kind: EffectKind, g: &ResolvedGraphics) -> bool {
    if !g.shader_effects {
        return false;
    }
    let Some(info) = kind.info() else {
        return false;
    };
    let cost_ok = match info.cost {
        EffectCost::Cheap => true,
        EffectCost::Moderate | EffectCost::Expensive => g.post_processing,
    };
    cost_ok && (kind != EffectKind::Bloom || g.bloom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceCaps;
    use crate::settings::{GraphicsSettings, QualityPreset};
    use std::collections::HashSet;

    fn resolved(preset: QualityPreset) -> ResolvedGraphics {
        GraphicsSettings {
            preset,
            ..GraphicsSettings::default()
        }
        .resolve(&DeviceCaps::desktop_fixture())
    }

    #[test]
    fn the_catalog_has_the_twelve_shaders_of_the_reference_mod() {
        assert_eq!(CATALOG.len(), 12);
        let names: HashSet<_> = CATALOG.iter().map(|e| e.shader_name).collect();
        assert_eq!(names.len(), 12, "shader names must be unique");
        for e in CATALOG.iter() {
            let uniforms: HashSet<_> = e.uniforms.iter().map(|x| x.name).collect();
            assert_eq!(uniforms.len(), e.uniforms.len(), "{} has a duplicate uniform", e.shader_name);
            assert!(e.kind.info().is_some());
        }
    }

    #[test]
    fn uniform_counts_match_the_source_shaders() {
        let count = |n: &str| find_by_shader_name(n).unwrap().uniforms.len();
        assert_eq!(count("adjustColor"), 4);
        assert_eq!(count("bloom"), 4);
        assert_eq!(count("coloredVignette"), 4);
        assert_eq!(count("darkenbutlights"), 5);
        assert_eq!(count("glitching"), 3);
        assert_eq!(count("heatwave"), 4);
        assert_eq!(count("lineboil"), 5);
        assert_eq!(count("saturation"), 2);
        assert_eq!(count("snow"), 14);
        assert_eq!(count("static"), 2);
        assert_eq!(count("tapenoise"), 3);
        assert_eq!(count("warp"), 1);
    }

    #[test]
    fn lookup_by_the_name_a_mod_uses_ignores_case() {
        assert_eq!(find_by_shader_name("HEATWAVE").unwrap().kind, EffectKind::Heatwave);
        assert!(find_by_shader_name("nonexistent").is_none());
    }

    #[test]
    fn low_keeps_cheap_effects_and_drops_the_expensive_ones() {
        let low = resolved(QualityPreset::Low);
        assert!(allowed(EffectKind::Heatwave, &low));
        assert!(allowed(EffectKind::Saturation, &low));
        assert!(!allowed(EffectKind::Bloom, &low));
        assert!(!allowed(EffectKind::Snow, &low));
    }

    #[test]
    fn high_allows_everything() {
        let high = resolved(QualityPreset::High);
        for e in CATALOG.iter() {
            assert!(allowed(e.kind, &high), "{} should run on High", e.shader_name);
        }
    }

    #[test]
    fn the_master_switch_and_the_bloom_switch_are_respected() {
        let mut s = GraphicsSettings {
            preset: QualityPreset::Custom,
            ..GraphicsSettings::default()
        };
        s.custom.post_processing = true;
        s.custom.bloom = false;
        let r = s.resolve(&DeviceCaps::desktop_fixture());
        assert!(allowed(EffectKind::Snow, &r));
        assert!(!allowed(EffectKind::Bloom, &r));

        s.custom.shader_effects = false;
        let r = s.resolve(&DeviceCaps::desktop_fixture());
        for e in CATALOG.iter() {
            assert!(!allowed(e.kind, &r));
        }
    }
}
