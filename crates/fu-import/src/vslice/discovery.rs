//! Discovery of V-Slice mod directories.
//!
//! A V-Slice mod is a directory inside the game's `mods/` folder that contains
//! `_polymod_meta.json`. Newer builds also accept `manifest.json`, so both are
//! treated as proof that a directory is a mod. Discovery is flat: V-Slice loads
//! one directory per mod and does not read a `mods/autoload.txt` list.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::discovery::{open_mod_dir, DiscoveredMod};
use crate::path;
use crate::report::{DiagnosticKind, ImportReport, Severity};
use crate::vslice::model::{VSLICE_MANIFEST_FILE, VSLICE_METADATA_FILE};

/// Directory names that are never mod content.
const SKIPPED_DIRECTORIES: [&str; 2] = [".git", "__MACOSX"];

/// Every mod reachable from a `mods/` directory.
#[derive(Debug, Clone, Default)]
pub struct VsliceDiscovery {
    /// Mods in directory-name order.
    pub mods: Vec<DiscoveredMod>,
    /// Discovery findings.
    pub report: ImportReport,
}

/// The metadata file discovered at the root of a mod directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataKind {
    /// `_polymod_meta.json`, the metadata file every V-Slice build reads.
    Polymod,
    /// `manifest.json`, used by newer builds.
    Manifest,
}

impl MetadataKind {
    /// File name of this metadata kind.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Polymod => VSLICE_METADATA_FILE,
            Self::Manifest => VSLICE_MANIFEST_FILE,
        }
    }
}

/// Detects which mod metadata file a directory contains.
///
/// Symbolic links are rejected rather than followed, so a linked metadata file
/// cannot redirect a read outside the mod.
pub fn metadata_kind(directory: &Path) -> Option<MetadataKind> {
    for kind in [MetadataKind::Polymod, MetadataKind::Manifest] {
        let candidate = directory.join(kind.file_name());
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                return Some(kind)
            }
            _ => {}
        }
    }
    None
}

/// True when `path` is a directory that looks like a V-Slice mod.
pub fn is_vslice_directory(path: &Path) -> bool {
    path.is_dir() && metadata_kind(path).is_some()
}

/// Opens one mod directory, resolving it canonically.
pub fn open_vslice_mod_dir(directory: &Path) -> Result<DiscoveredMod, path::PathError> {
    open_mod_dir(directory)
}

/// Scans a `mods/` directory for V-Slice mods.
///
/// Directories without a recognized metadata file are reported at info
/// severity and skipped: they may belong to another engine.
pub fn discover_vslice_mods(mods_root: &Path) -> VsliceDiscovery {
    let mut discovery = VsliceDiscovery {
        report: ImportReport::with_target(mods_root.to_string_lossy().to_string()),
        ..VsliceDiscovery::default()
    };

    let root = match path::canonical_root(mods_root) {
        Ok(root) => root,
        Err(error) => {
            discovery.report.add(
                DiagnosticKind::ModRootMissing,
                format!("mods directory is unusable: {error}"),
            );
            return discovery;
        }
    };

    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            discovery.report.add(
                DiagnosticKind::ScanIoError,
                format!("mods directory could not be listed: {error}"),
            );
            return discovery;
        }
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if file_type.is_symlink() {
            discovery.report.add_at(
                DiagnosticKind::ScanSymlinkSkipped,
                name,
                "symbolic link was not followed during discovery",
            );
            continue;
        }
        if !file_type.is_dir()
            || name.starts_with('.')
            || SKIPPED_DIRECTORIES.contains(&name.as_str())
        {
            continue;
        }
        candidates.push(entry.path());
    }
    candidates.sort();

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for candidate in candidates {
        if metadata_kind(&candidate).is_none() {
            discovery.report.add_at(
                DiagnosticKind::MissingMetadata,
                relative_name(&root, &candidate),
                "directory has no V-Slice metadata file and was skipped",
            );
            let last = discovery
                .report
                .diagnostics_mut()
                .last_mut()
                .expect("just pushed");
            last.severity = Severity::Info;
            continue;
        }
        match open_vslice_mod_dir(&candidate) {
            Ok(discovered) => {
                if seen.insert(discovered.root.clone()) {
                    discovery.mods.push(discovered);
                }
            }
            Err(error) => discovery.report.add_at(
                DiagnosticKind::ScanIoError,
                relative_name(&root, &candidate),
                format!("mod directory could not be opened: {error}"),
            ),
        }
    }

    if discovery.mods.is_empty() {
        discovery.report.add_at(
            DiagnosticKind::MissingMetadata,
            VSLICE_METADATA_FILE,
            "no V-Slice mods were found in the mods directory",
        );
    }

    discovery.report.sort();
    discovery
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn detects_polymod_metadata_first() {
        let temp = TempDir::new("vslice-meta").unwrap();
        temp.write("mod/_polymod_meta.json", "{}").unwrap();
        temp.write("mod/manifest.json", "{}").unwrap();
        assert_eq!(
            metadata_kind(&temp.join("mod")),
            Some(MetadataKind::Polymod)
        );
        assert!(is_vslice_directory(&temp.join("mod")));
    }

    #[test]
    fn accepts_manifest_only_mods() {
        let temp = TempDir::new("vslice-manifest").unwrap();
        temp.write("mod/manifest.json", "{}").unwrap();
        assert_eq!(
            metadata_kind(&temp.join("mod")),
            Some(MetadataKind::Manifest)
        );
    }

    #[test]
    fn ignores_directories_without_metadata() {
        let temp = TempDir::new("vslice-skip").unwrap();
        temp.write("mods/not-vslice/data/x.txt", "x").unwrap();
        temp.write("mods/good/_polymod_meta.json", "{}").unwrap();
        let discovered = discover_vslice_mods(&temp.join("mods"));
        assert_eq!(discovered.mods.len(), 1);
        assert_eq!(discovered.mods[0].name, "good");
        assert!(discovered.report.contains_code("missing-metadata"));
        assert!(!discovered.report.has_errors(), "{:?}", discovered.report);
    }

    #[test]
    fn a_missing_mods_root_is_an_error() {
        let temp = TempDir::new("vslice-missing").unwrap();
        let discovered = discover_vslice_mods(&temp.join("nope"));
        assert!(discovered.mods.is_empty());
        assert!(discovered.report.contains_code("mod-root-missing"));
        assert!(discovered.report.has_errors());
    }

    #[test]
    fn an_empty_mods_root_reports_no_mods() {
        let temp = TempDir::new("vslice-empty").unwrap();
        temp.mkdir("mods").unwrap();
        let discovered = discover_vslice_mods(&temp.join("mods"));
        assert!(discovered.mods.is_empty());
        assert!(discovered.report.contains_code("missing-metadata"));
    }
}
