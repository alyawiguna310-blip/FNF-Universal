//! Resource manifests: logical names mapped to virtual paths.
//!
//! Gameplay code asks for `note.left`, never `/defaults/images/notes/left.png`.
//! A manifest owns that mapping, so resource layout can change without touching
//! gameplay, and a resource pack or mod can add keys or remap them.
//!
//! Layers are merged in load order and the **last** layer wins, matching the VFS
//! rule that a higher-priority mount shadows a lower one.

use std::collections::BTreeMap;
use std::fmt;

use fu_vfs::VfsPath;
use serde::Deserialize;

use crate::error::{AssetError, AssetResult};

/// A parsed `manifest.json` describing logical resources.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct ResourceManifest {
    /// Human readable pack name.
    #[serde(default)]
    pub name: String,
    /// Pack version string, informational only.
    #[serde(default)]
    pub version: String,
    /// Logical key to virtual path mapping.
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
}

impl ResourceManifest {
    /// Parses manifest JSON, validating every entry.
    pub fn from_json(bytes: &[u8]) -> AssetResult<Self> {
        let manifest: Self = serde_json::from_slice(bytes).map_err(|source| AssetError::Json {
            path: VfsPath::new("/manifest.json").expect("static path"),
            source,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Resolves a virtual path for `key`, and reports the error context.
    pub fn lookup(&self, key: &str) -> Option<&str> {
        self.assets.get(key).map(String::as_str)
    }

    /// Adds or replaces one entry. The path is validated immediately.
    pub fn insert(&mut self, key: impl Into<String>, path: impl Into<String>) -> AssetResult<()> {
        let key = key.into();
        let path = path.into();
        validate_entry(&key, &path)?;
        self.assets.insert(key, path);
        Ok(())
    }

    /// Merges `other` over this manifest.
    ///
    /// Returns the keys whose path changed, which is what diagnostics and tests
    /// report as "this pack overrides the default".
    pub fn merge_over(&mut self, other: &Self) -> Vec<String> {
        let mut overridden = Vec::new();
        for (key, path) in &other.assets {
            match self.assets.get(key) {
                Some(existing) if existing != path => overridden.push(key.clone()),
                None => overridden.push(key.clone()),
                Some(_) => {}
            }
        }
        for (key, path) in &other.assets {
            self.assets.insert(key.clone(), path.clone());
        }
        overridden.sort();
        overridden
    }

    /// Keys in this manifest.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.assets.keys().map(String::as_str)
    }

    /// Rejects empty keys and paths that are not valid absolute virtual paths.
    pub fn validate(&self) -> AssetResult<()> {
        for (key, path) in &self.assets {
            validate_entry(key, path)?;
        }
        Ok(())
    }
}

fn validate_entry(key: &str, path: &str) -> AssetResult<()> {
    if key.trim().is_empty() {
        return Err(AssetError::InvalidManifest {
            reason: "a resource key is empty".to_string(),
        });
    }
    if key.contains(char::is_whitespace) {
        return Err(AssetError::InvalidManifest {
            reason: format!("resource key '{key}' contains whitespace"),
        });
    }
    if !path.starts_with('/') {
        return Err(AssetError::InvalidManifest {
            reason: format!("resource '{key}' path '{path}' must be an absolute virtual path"),
        });
    }
    VfsPath::new(path).map_err(|error| AssetError::InvalidManifest {
        reason: format!("resource '{key}' path '{path}' is invalid: {error}"),
    })?;
    Ok(())
}

/// A resource resolved to a path and the layer that supplied it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedResource<'a> {
    /// Virtual path to read.
    pub path: &'a str,
    /// Name of the layer that defined the winning entry.
    pub source: &'a str,
}

/// Ordered resource manifest layers, highest priority last.
#[derive(Debug, Clone, Default)]
pub struct ResourceStack {
    layers: Vec<Layer>,
}

#[derive(Debug, Clone)]
struct Layer {
    source: String,
    manifest: ResourceManifest,
}

impl ResourceStack {
    /// An empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a layer on top of the ones already present.
    pub fn push(&mut self, source: impl Into<String>, manifest: ResourceManifest) {
        self.layers.push(Layer {
            source: source.into(),
            manifest,
        });
    }

    /// Number of layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// True when no layer was added.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Source names in resolution order (lowest priority first).
    pub fn sources(&self) -> impl Iterator<Item = &str> {
        self.layers.iter().map(|layer| layer.source.as_str())
    }

    /// Resolves `key` against the highest-priority layer that defines it.
    pub fn resolve(&self, key: &str) -> Option<ResolvedResource<'_>> {
        self.layers.iter().rev().find_map(|layer| {
            layer.manifest.lookup(key).map(|path| ResolvedResource {
                path,
                source: layer.source.as_str(),
            })
        })
    }

    /// Resolved path for `key`, ignoring which layer supplied it.
    pub fn path(&self, key: &str) -> Option<&str> {
        self.resolve(key).map(|resolved| resolved.path)
    }

    /// Every key visible across all layers, highest priority wins.
    pub fn merged(&self) -> ResourceManifest {
        let mut merged = ResourceManifest::default();
        for layer in &self.layers {
            for (key, path) in &layer.manifest.assets {
                merged.assets.insert(key.clone(), path.clone());
            }
        }
        merged
    }
}

impl fmt::Display for ResourceStack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} manifest layer(s): {}",
            self.layers.len(),
            self.layers
                .iter()
                .map(|layer| layer.source.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> ResourceManifest {
        ResourceManifest::from_json(json.as_bytes()).expect("manifest")
    }

    const DEFAULT: &str = r#"{
        "name": "Default",
        "version": "1.0.0",
        "assets": {
            "note.left": "/defaults/images/notes/note_left.png",
            "stage.default": "/defaults/images/stage/stage.png"
        }
    }"#;

    const MOD: &str = r#"{
        "name": "Mod",
        "assets": {
            "note.left": "/mods/mine/note_left.png"
        }
    }"#;

    #[test]
    fn parses_and_validates_entries() {
        let parsed = manifest(DEFAULT);
        assert_eq!(parsed.name, "Default");
        assert_eq!(
            parsed.lookup("note.left"),
            Some("/defaults/images/notes/note_left.png")
        );
        assert_eq!(parsed.keys().count(), 2);
    }

    #[test]
    fn rejects_invalid_manifest_entries() {
        let error =
            ResourceManifest::from_json(br#"{"assets":{"note.left":"relative.png"}}"#).unwrap_err();
        assert!(error.to_string().contains("absolute"), "got: {error}");

        let error = ResourceManifest::from_json(br#"{"assets":{"":"/a.png"}}"#).unwrap_err();
        assert!(error.to_string().contains("empty"), "got: {error}");

        let error =
            ResourceManifest::from_json(br#"{"assets":{"a":"/../escape.png"}}"#).unwrap_err();
        assert!(error.to_string().contains("invalid"), "got: {error}");

        let error = ResourceManifest::from_json(b"not json").unwrap_err();
        assert!(error.to_string().contains("deserialize"), "got: {error}");
    }

    #[test]
    fn later_layers_override_earlier_ones() {
        let mut stack = ResourceStack::new();
        stack.push("defaults", manifest(DEFAULT));
        stack.push("mod", manifest(MOD));

        let resolved = stack.resolve("note.left").expect("note.left");
        assert_eq!(resolved.path, "/mods/mine/note_left.png");
        assert_eq!(resolved.source, "mod");

        let stage = stack.resolve("stage.default").expect("stage");
        assert_eq!(stage.source, "defaults", "unshadowed keys fall through");

        assert_eq!(stack.resolve("missing.key"), None);
        assert_eq!(stack.sources().count(), 2);
        assert_eq!(stack.merged().keys().count(), 2);
    }

    #[test]
    fn merge_reports_overridden_keys() {
        let mut base = manifest(DEFAULT);
        let overridden = base.merge_over(&manifest(MOD));
        assert_eq!(overridden, vec!["note.left".to_string()]);
        assert_eq!(base.lookup("note.left"), Some("/mods/mine/note_left.png"));
        assert_eq!(
            base.lookup("stage.default"),
            Some("/defaults/images/stage/stage.png")
        );
    }
}
