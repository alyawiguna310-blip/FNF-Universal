//! Read-only discovery of mod directories and their file trees.
//!
//! Discovery never follows symbolic links and never leaves the selected
//! directory. Every path that reaches the filesystem is either a fixed literal
//! name or a [`SafeRelativePath`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::path::{self, PathError, SafeRelativePath};
use crate::report::{DiagnosticKind, ImportReport};

/// File name of the Codename Engine mod autoload list.
pub const AUTOLOAD_FILE: &str = "autoload.txt";

/// Maximum recursion depth used when inventorying a mod.
pub const DEFAULT_MAX_DEPTH: usize = 32;

/// Maximum size of `autoload.txt` accepted, in bytes.
pub const MAX_AUTOLOAD_BYTES: u64 = 256 * 1024;

/// Directory names that are never mod content.
const SKIPPED_DIRECTORIES: [&str; 2] = [".git", "__MACOSX"];

/// A mod directory that was successfully resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredMod {
    /// Directory name.
    pub name: String,
    /// Canonical directory path.
    pub root: PathBuf,
    /// Entry as written in `autoload.txt`, or the directory name when the mod
    /// was found by scanning.
    pub entry: String,
}

/// Result of reading `mods/autoload.txt`.
#[derive(Debug, Clone, Default)]
pub struct ModsDiscovery {
    /// True when `autoload.txt` existed and was readable.
    pub autoload_present: bool,
    /// Mods in autoload order, or alphabetical order when scanning.
    pub mods: Vec<DiscoveredMod>,
    /// Discovery findings.
    pub report: ImportReport,
}

/// One inventoried file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModFile {
    /// Mod-relative path.
    pub relative: SafeRelativePath,
    /// Size in bytes.
    pub size: u64,
}

/// Recursive listing of a mod directory.
#[derive(Debug, Clone, Default)]
pub struct ModTree {
    /// Regular files, sorted by path.
    pub files: Vec<ModFile>,
    /// Directory paths, sorted.
    pub directories: Vec<String>,
    /// Scanning findings.
    pub report: ImportReport,
}

impl ModTree {
    /// True when the mod contains no files at all.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Case-sensitive lookup of one file.
    pub fn file(&self, relative: &str) -> Option<&ModFile> {
        self.files
            .iter()
            .find(|file| file.relative.as_str() == relative)
    }

    /// Case-insensitive lookup of one file.
    pub fn file_case_insensitive(&self, relative: &str) -> Option<&ModFile> {
        self.files
            .iter()
            .find(|file| file.relative.as_str().eq_ignore_ascii_case(relative))
    }

    /// Files directly inside `directory` (no recursion).
    pub fn files_in_directory<'a>(
        &'a self,
        directory: &'a str,
    ) -> impl Iterator<Item = &'a ModFile> + 'a {
        self.files
            .iter()
            .filter(move |file| file.relative.parent() == Some(directory))
    }

    /// True when `directory` exists in the scan.
    pub fn has_directory(&self, directory: &str) -> bool {
        self.directories
            .iter()
            .any(|candidate| candidate == directory)
    }
}

/// Opens a single mod directory, resolving it canonically.
///
/// Fails when the path is missing, is not a directory, or cannot be resolved.
pub fn open_mod_dir(directory: &Path) -> Result<DiscoveredMod, PathError> {
    let root = path::canonical_root(directory)?;
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    Ok(DiscoveredMod {
        entry: name.clone(),
        name,
        root,
    })
}

/// Reads `mods/autoload.txt` and resolves each listed mod directory.
///
/// When the autoload file is missing or unusable, the directory is scanned
/// instead so a usable import is still possible; the report records why.
pub fn discover_mods_root(mods_root: &Path) -> ModsDiscovery {
    let mut discovery = ModsDiscovery {
        report: ImportReport::with_target(mods_root.to_string_lossy().to_string()),
        ..ModsDiscovery::default()
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

    let autoload_path = root.join(AUTOLOAD_FILE);
    let autoload = match std::fs::symlink_metadata(&autoload_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            discovery.report.add_at(
                DiagnosticKind::UnsafePath,
                AUTOLOAD_FILE,
                "autoload list is a symbolic link and was not read",
            );
            None
        }
        Ok(metadata) if metadata.is_file() => match read_autoload(&autoload_path) {
            Ok(text) => Some(text),
            Err(message) => {
                discovery
                    .report
                    .add_at(DiagnosticKind::AutoloadUnreadable, AUTOLOAD_FILE, message);
                None
            }
        },
        Ok(_) => {
            discovery.report.add_at(
                DiagnosticKind::AutoloadUnreadable,
                AUTOLOAD_FILE,
                "autoload list is not a regular file",
            );
            None
        }
        Err(_) => None,
    };

    match autoload {
        Some(text) => {
            discovery.autoload_present = true;
            let mut seen = BTreeSet::new();
            for entry in autoload_entries(&text) {
                match SafeRelativePath::parse(&entry) {
                    Ok(relative) => match resolve_mod_entry(&root, &relative) {
                        Ok(discovered) => {
                            if seen.insert(discovered.root.clone()) {
                                discovery.mods.push(discovered);
                            } else {
                                discovery.report.add_at(
                                    DiagnosticKind::AutoloadEntryDuplicate,
                                    entry.clone(),
                                    "mod is listed more than once in autoload.txt",
                                );
                            }
                        }
                        Err(diagnostic) => discovery.report.push(diagnostic),
                    },
                    Err(error) => discovery.report.add_at(
                        DiagnosticKind::AutoloadEntryMalformed,
                        entry.clone(),
                        format!("entry rejected: {error}"),
                    ),
                }
            }
            if discovery.mods.is_empty() {
                discovery.report.add_at(
                    DiagnosticKind::AutoloadMissing,
                    AUTOLOAD_FILE,
                    "no usable mod entries were found in the autoload list",
                );
            }
        }
        None => {
            if !discovery
                .report
                .contains_code(DiagnosticKind::AutoloadUnreadable.code())
            {
                discovery.report.add_at(
                    DiagnosticKind::AutoloadMissing,
                    AUTOLOAD_FILE,
                    "autoload list is missing; scanning the mods directory instead",
                );
            }
            discovery.mods = scan_for_mods(&root, &mut discovery.report);
            for module in &discovery.mods {
                discovery.report.push(
                    crate::report::Diagnostic::new(
                        DiagnosticKind::UnknownMetadata,
                        format!(
                            "mod directory '{}' discovered by scanning because autoload.txt is absent",
                            module.entry
                        ),
                    )
                    .with_severity(crate::report::Severity::Info),
                );
            }
        }
    }

    discovery.report.sort();
    discovery
}

fn read_autoload(path: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_AUTOLOAD_BYTES {
        return Err(format!(
            "autoload list is {} bytes, above the {MAX_AUTOLOAD_BYTES} byte limit",
            metadata.len()
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|_| "autoload list is not valid UTF-8".to_string())
}

/// Splits an autoload file into entries.
///
/// Blank lines and `#`/`;` comment lines are skipped, matching the tolerance a
/// hand-edited list needs.
pub fn autoload_entries(source: &str) -> Vec<String> {
    source
        .lines()
        .map(|line| line.trim_start_matches('\u{feff}').trim())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#') && !line.starts_with(';'))
        .map(|line| line.to_string())
        .collect()
}

fn resolve_mod_entry(
    root: &Path,
    relative: &SafeRelativePath,
) -> Result<DiscoveredMod, crate::report::Diagnostic> {
    let candidate = path::resolve(root, relative);
    let metadata = std::fs::symlink_metadata(&candidate).map_err(|_| {
        crate::report::Diagnostic::new(
            DiagnosticKind::AutoloadEntryMissing,
            "listed in autoload.txt but no such directory exists",
        )
        .at(relative.to_string())
    })?;

    if metadata.file_type().is_symlink() {
        let resolved = std::fs::canonicalize(&candidate).map_err(|error| {
            crate::report::Diagnostic::new(
                DiagnosticKind::UnsafePath,
                format!("symbolic link could not be resolved: {error}"),
            )
            .at(relative.to_string())
        })?;
        let resolved = path::simplify_path(&resolved);
        if !resolved.starts_with(root) {
            return Err(crate::report::Diagnostic::new(
                DiagnosticKind::UnsafePath,
                "symbolic link points outside the mods directory",
            )
            .at(relative.to_string()));
        }
        return open_mod_dir(&resolved).map_err(|error| {
            crate::report::Diagnostic::new(
                DiagnosticKind::AutoloadEntryMissing,
                format!("mod directory is unusable: {error}"),
            )
            .at(relative.to_string())
        });
    }

    if !metadata.is_dir() {
        return Err(crate::report::Diagnostic::new(
            DiagnosticKind::AutoloadEntryMalformed,
            "autoload entry is not a directory",
        )
        .at(relative.to_string()));
    }

    // Defense in depth: the lexical join is already safe, but a directory could
    // still be reached through a junction that canonicalizes elsewhere.
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
        crate::report::Diagnostic::new(
            DiagnosticKind::AutoloadEntryMissing,
            format!("mod directory could not be resolved: {error}"),
        )
        .at(relative.to_string())
    })?;
    let canonical = path::simplify_path(&canonical);
    if !canonical.starts_with(root) {
        return Err(crate::report::Diagnostic::new(
            DiagnosticKind::UnsafePath,
            "mod directory resolves outside the mods directory",
        )
        .at(relative.to_string()));
    }

    open_mod_dir(&canonical).map_err(|error| {
        crate::report::Diagnostic::new(
            DiagnosticKind::AutoloadEntryMissing,
            format!("mod directory is unusable: {error}"),
        )
        .at(relative.to_string())
    })
}

fn scan_for_mods(root: &Path, report: &mut ImportReport) -> Vec<DiscoveredMod> {
    let mut mods = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        report.add(
            DiagnosticKind::ScanIoError,
            "mods directory could not be listed",
        );
        return mods;
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if file_type.is_symlink() {
            let name = entry.file_name().to_string_lossy().to_string();
            report.add_at(
                DiagnosticKind::ScanSymlinkSkipped,
                name,
                "symbolic link was not followed during discovery",
            );
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || SKIPPED_DIRECTORIES.contains(&name.as_str()) {
            continue;
        }
        candidates.push(entry.path());
    }
    candidates.sort();

    for candidate in candidates {
        match open_mod_dir(&candidate) {
            Ok(discovered) => mods.push(discovered),
            Err(error) => report.add_at(
                DiagnosticKind::ScanIoError,
                candidate.file_name().map_or_else(
                    || candidate.to_string_lossy().to_string(),
                    |name| name.to_string_lossy().to_string(),
                ),
                format!("directory could not be opened: {error}"),
            ),
        }
    }
    mods
}

/// Recursively lists a mod directory without following symbolic links.
pub fn scan_mod_tree(root: &Path, max_depth: usize) -> ModTree {
    let mut tree = ModTree::default();
    let max_depth = max_depth.max(1);
    let mut depth_reported = false;

    let walker = WalkDir::new(root)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            !SKIPPED_DIRECTORIES
                .iter()
                .any(|skipped| entry.file_name() == *skipped)
        });

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                let location = error
                    .path()
                    .map(|path| path::display_relative(root, path))
                    .unwrap_or_else(|| "<unknown>".to_string());
                tree.report.add_at(
                    DiagnosticKind::ScanIoError,
                    location,
                    format!("entry could not be read: {error}"),
                );
                continue;
            }
        };

        if entry.depth() == 0 {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(root)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"));

        let Some(relative) = relative else {
            tree.report.add(
                DiagnosticKind::ScanIoError,
                "an entry did not resolve inside the mod directory",
            );
            continue;
        };

        let file_type = entry.file_type();
        if file_type.is_symlink() {
            tree.report.add_at(
                DiagnosticKind::ScanSymlinkSkipped,
                relative.clone(),
                "symbolic link was not followed and is not part of the import",
            );
            continue;
        }

        if file_type.is_dir() {
            if entry.depth() == max_depth && !depth_reported {
                depth_reported = true;
                tree.report.add_at(
                    DiagnosticKind::ScanDepthLimit,
                    relative.clone(),
                    format!("directories deeper than {max_depth} levels were not scanned"),
                );
            }
            tree.directories.push(relative);
            continue;
        }

        if !file_type.is_file() {
            tree.report.add_at(
                DiagnosticKind::UnsupportedFeature,
                relative.clone(),
                "special filesystem entry was ignored",
            );
            continue;
        }

        let Ok(relative) = SafeRelativePath::parse(&relative) else {
            tree.report.add_at(
                DiagnosticKind::UnsafePath,
                relative,
                "path is not usable as a mod-relative path",
            );
            continue;
        };
        let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        tree.files.push(ModFile { relative, size });
    }

    tree.files
        .sort_by(|left, right| left.relative.cmp(&right.relative));
    tree.directories.sort();
    tree.report.sort();
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn fixture() -> TempDir {
        let temp = TempDir::new("discovery").unwrap();
        temp.write(
            "mods/a/data/config/modpack.ini",
            "[Common]\nAPI_VERSION=3\n",
        )
        .unwrap();
        temp.write("mods/b/songs/x/meta.json", "{}").unwrap();
        temp.write("mods/autoload.txt", "a\nb\n").unwrap();
        temp
    }

    #[test]
    fn reads_autoload_entries_and_skips_noise() {
        let entries = autoload_entries("\u{feff}mod-a\r\n\r\n# comment\n; other\n  mod b  \n");
        assert_eq!(entries, vec!["mod-a", "mod b"]);
    }

    #[test]
    fn discovers_mods_in_autoload_order() {
        let temp = fixture();
        let discovery = discover_mods_root(&temp.join("mods"));
        assert!(discovery.autoload_present);
        let names: Vec<_> = discovery
            .mods
            .iter()
            .map(|module| module.name.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(discovery.mods[1].entry, "b");
        assert!(!discovery.report.has_errors(), "{:?}", discovery.report);
    }

    #[test]
    fn falls_back_to_scanning_without_autoload() {
        let temp = fixture();
        std::fs::remove_file(temp.join("mods/autoload.txt")).unwrap();
        let discovery = discover_mods_root(&temp.join("mods"));
        assert!(!discovery.autoload_present);
        assert!(discovery.report.contains_code("autoload-missing"));
        let names: Vec<_> = discovery
            .mods
            .iter()
            .map(|module| module.name.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn reports_missing_and_unsafe_autoload_entries() {
        let temp = fixture();
        temp.write(
            "mods/autoload.txt",
            "a\nmissing-mod\n../escape\nC:\\absolute\n",
        )
        .unwrap();
        let discovery = discover_mods_root(&temp.join("mods"));
        assert_eq!(discovery.mods.len(), 1);
        assert!(discovery.report.contains_code("autoload-entry-missing"));
        assert!(discovery.report.contains_code("autoload-entry-malformed"));
        assert!(discovery.report.has_errors());
    }

    #[test]
    fn ignores_entries_that_are_not_directories() {
        let temp = fixture();
        temp.write("mods/not-a-mod.txt", "x").unwrap();
        temp.write("mods/autoload.txt", "not-a-mod.txt").unwrap();
        let discovery = discover_mods_root(&temp.join("mods"));
        assert!(discovery.mods.is_empty());
        assert!(discovery.report.contains_code("autoload-entry-malformed"));
    }

    #[test]
    fn reports_unreadable_autoload() {
        let temp = fixture();
        temp.write_bytes("mods/autoload.txt", &[0xff, 0xfe, 0x00])
            .unwrap();
        let discovery = discover_mods_root(&temp.join("mods"));
        assert!(!discovery.autoload_present);
        assert!(discovery.report.contains_code("autoload-unreadable"));
        assert_eq!(discovery.mods.len(), 2, "scan fallback still finds mods");
    }

    #[test]
    fn scans_a_mod_tree_and_sorts_it() {
        let temp = fixture();
        let root = path::canonical_root(&temp.join("mods/a")).unwrap();
        let tree = scan_mod_tree(&root, DEFAULT_MAX_DEPTH);
        let paths: Vec<_> = tree
            .files
            .iter()
            .map(|file| file.relative.as_str())
            .collect();
        assert_eq!(paths, vec!["data/config/modpack.ini"]);
        assert!(tree.has_directory("data"));
        assert!(tree.has_directory("data/config"));
        assert!(tree.file("data/config/modpack.ini").is_some());
        assert!(tree
            .file_case_insensitive("DATA/config/modpack.ini")
            .is_some());
        assert_eq!(tree.files_in_directory("data/config").count(), 1);
    }

    #[test]
    fn reports_depth_limit() {
        let temp = TempDir::new("depth").unwrap();
        temp.write("mod/a/b/c/d/file.txt", "x").unwrap();
        let root = path::canonical_root(&temp.join("mod")).unwrap();
        let tree = scan_mod_tree(&root, 2);
        assert!(tree.report.contains_code("scan-depth-limit"));
        assert!(tree.file("a/b/c/d/file.txt").is_none());
    }

    #[test]
    fn opening_a_missing_mod_dir_fails() {
        let temp = TempDir::new("missing").unwrap();
        assert!(open_mod_dir(&temp.join("nope")).is_err());
        assert!(open_mod_dir(temp.path()).is_ok());
    }

    #[test]
    fn duplicate_autoload_entries_are_reported_once() {
        let temp = fixture();
        temp.write("mods/autoload.txt", "a\na\n").unwrap();
        let discovery = discover_mods_root(&temp.join("mods"));
        assert_eq!(discovery.mods.len(), 1);
        assert!(discovery.report.contains_code("autoload-entry-duplicate"));
    }
}
