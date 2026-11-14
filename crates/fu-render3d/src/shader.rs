//! Shader identity, runtime parameters and a source registry.
//!
//! Parameters are name/value pairs so effects can be driven the way Codename
//! Engine's `CustomShader` sets uniforms by name (for example
//! `darkenbutlights`: strength, threshold, tint, gradientY, gradientHeight).
//! WGSL is stored as text; it is NOT parsed or validated here because that
//! needs a backend (naga/wgpu).

use crate::{RenderError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShaderId(pub String);

impl ShaderId {
    pub const UNLIT: &'static str = "builtin/unlit";
    pub const LIT: &'static str = "builtin/lit";

    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn unlit() -> Self {
        Self::new(Self::UNLIT)
    }
    pub fn lit() -> Self {
        Self::new(Self::LIT)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderParamValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Bool(bool),
}

pub type ShaderParams = BTreeMap<String, ShaderParamValue>;

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderSource {
    pub wgsl: String,
    /// Starts at 1 and increases on every `replace`, so backends can invalidate caches.
    pub revision: u32,
}

#[derive(Debug, Default)]
pub struct ShaderRegistry {
    sources: BTreeMap<ShaderId, ShaderSource>,
}

fn validate(id: &ShaderId, wgsl: &str) -> Result<()> {
    let ok = !id.0.is_empty()
        && id
            .0
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'));
    if !ok {
        return Err(RenderError::invalid(
            "shader id",
            "use ASCII letters, digits, '/', '_', '-' or '.'",
        ));
    }
    if wgsl.trim().is_empty() || wgsl.contains('\0') {
        return Err(RenderError::invalid(
            "shader source",
            "empty or contains NUL",
        ));
    }
    Ok(())
}

impl ShaderRegistry {
    pub fn register(&mut self, id: ShaderId, wgsl: impl Into<String>) -> Result<()> {
        let wgsl = wgsl.into();
        validate(&id, &wgsl)?;
        if self.sources.contains_key(&id) {
            return Err(RenderError::invalid(
                "shader",
                format!("'{}' is already registered", id.0),
            ));
        }
        self.sources.insert(id, ShaderSource { wgsl, revision: 1 });
        Ok(())
    }

    /// Insert or overwrite; returns the new revision.
    pub fn replace(&mut self, id: ShaderId, wgsl: impl Into<String>) -> Result<u32> {
        let wgsl = wgsl.into();
        validate(&id, &wgsl)?;
        let revision = self.sources.get(&id).map_or(1, |s| s.revision + 1);
        self.sources.insert(id, ShaderSource { wgsl, revision });
        Ok(revision)
    }

    pub fn get(&self, id: &ShaderId) -> Option<&ShaderSource> {
        self.sources.get(id)
    }

    pub fn contains(&self, id: &ShaderId) -> bool {
        self.sources.contains_key(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &ShaderId> {
        self.sources.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_rejects_duplicates_and_bad_input() {
        let mut registry = ShaderRegistry::default();
        registry.register(ShaderId::lit(), "// wgsl").unwrap();
        assert!(registry.register(ShaderId::lit(), "// wgsl").is_err());
        assert!(registry.register(ShaderId::new(""), "x").is_err());
        assert!(registry.register(ShaderId::new("bad id"), "x").is_err());
        assert!(registry.register(ShaderId::new("ok"), "  ").is_err());
    }

    #[test]
    fn replace_bumps_revision() {
        let mut registry = ShaderRegistry::default();
        assert_eq!(registry.replace(ShaderId::unlit(), "a").unwrap(), 1);
        assert_eq!(registry.replace(ShaderId::unlit(), "b").unwrap(), 2);
        assert_eq!(registry.get(&ShaderId::unlit()).unwrap().wgsl, "b");
    }

    #[test]
    fn param_values_serialize() {
        let value = ShaderParamValue::Vec4([0.0, 0.5, 1.0, 0.9]);
        let json = serde_json::to_string(&value).unwrap();
        let back: ShaderParamValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }
}
