use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStatus {
    ManifestOnly,
    Experimental,
    Integrated,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime: String,
    pub platforms: Vec<String>,
    pub integration_status: IntegrationStatus,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid manifest: {0}")]
    Invalid(String),
    #[error("duplicate engine id: {0}")]
    Duplicate(String),
    #[error("engine not found: {0}")]
    NotFound(String),
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
}

impl EngineManifest {
    pub fn validate(&self) -> Result<(), EngineError> {
        let id_ok = !self.id.is_empty()
            && self.id.chars().all(|c| c.is_ascii_lowercase()
                || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
        if !id_ok { return Err(EngineError::Invalid("id must use lowercase ASCII, digits, '.', '_' or '-'".into())); }
        if self.name.trim().is_empty() || self.version.trim().is_empty() || self.runtime.trim().is_empty() {
            return Err(EngineError::Invalid("name, version and runtime are required".into()));
        }
        if self.platforms.is_empty() { return Err(EngineError::Invalid("at least one platform is required".into())); }
        if self.platforms.iter().any(|p| !matches!(p.as_str(), "android" | "windows" | "linux")) {
            return Err(EngineError::Invalid("platform must be android, windows or linux".into()));
        }
        Ok(())
    }

    pub fn supports(&self, platform: &str) -> bool {
        self.platforms.iter().any(|p| p == platform)
    }
}

#[derive(Default)]
pub struct EngineRegistry {
    engines: BTreeMap<String, EngineManifest>,
}

impl EngineRegistry {
    pub fn register(&mut self, manifest: EngineManifest) -> Result<(), EngineError> {
        manifest.validate()?;
        if self.engines.contains_key(&manifest.id) {
            return Err(EngineError::Duplicate(manifest.id));
        }
        self.engines.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<&EngineManifest, EngineError> {
        self.engines.get(id).ok_or_else(|| EngineError::NotFound(id.into()))
    }

    pub fn list(&self) -> Vec<&EngineManifest> { self.engines.values().collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> EngineManifest {
        EngineManifest {
            id: "vslice".into(), name: "V-Slice".into(), version: "0.1".into(),
            runtime: "haxe".into(), platforms: vec!["android".into()],
            integration_status: IntegrationStatus::ManifestOnly,
            capabilities: vec![], notes: None,
        }
    }
    #[test] fn validates_manifest() { assert!(sample().validate().is_ok()); }
    #[test] fn rejects_duplicate_ids() {
        let mut r = EngineRegistry::default();
        r.register(sample()).unwrap();
        assert!(matches!(r.register(sample()), Err(EngineError::Duplicate(_))));
    }
    #[test] fn platform_check() { assert!(sample().supports("android")); }
}
