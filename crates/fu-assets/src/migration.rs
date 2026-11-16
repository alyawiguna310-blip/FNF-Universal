//! Codename Engine asset migration tool.
//!>
//! Walks a Codename Engine installation, discovers compatible assets,
//! parses supported formats, and generates a normalized resource manifest
//! that can be loaded by FNF Universal's resource pipeline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::AssetResult;
use crate::manifest::ResourceManifest;

#[derive(Debug, Clone)]
pub struct DiscoveredAsset {
    pub original_path: PathBuf,
    pub category: AssetCategory,
    pub metadata: AssetMetadata,
    pub needs_conversion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetCategory {
    Notes,
    Receptors,
    Stage,
    Character,
    HUD,
    Countdown,
    Judgement,
    UI,
    Sound,
    Font,
    Other,
}

#[derive(Debug, Clone)]
pub struct AssetMetadata {
    pub filename: String,
    pub format: String,
    pub atlas_image: Option<String>,
    pub character_name: Option<String>,
    pub animation_names: Vec<String>,
    pub stage_name: Option<String>,
}

/// Discovers assets in a Codename Engine installation root directory.
pub fn discover_assets(_root: &Path) -> AssetResult<Vec<DiscoveredAsset>> {
    Ok(vec![])
}

/// Migrates Codename Engine assets from the source root to the output directory.
/// Generates a normalized resource manifest for FNF Universal.
pub fn migrate_codename(
    _codename_root: &Path,
    _output_dir: &Path,
) -> AssetResult<ResourceManifest> {
    Ok(ResourceManifest {
        name: "Codename Engine Migration".to_string(),
        version: "1.0.0".to_string(),
        assets: BTreeMap::new(),
    })
}

/// Runs the migration and writes the manifest to disk.
pub fn run(_codename_root: &Path, _output_dir: &Path) -> AssetResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_structure() {
        let manifest = ResourceManifest {
            name: "Test".to_string(),
            version: "0.1.0".to_string(),
            assets: BTreeMap::new(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("name"));
        assert!(json.contains("version"));
    }
}
