use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::backend::{BackendDirEntry, VfsBackend};
use crate::error::{VfsError, VfsResult};
use crate::path::VfsPath;
use crate::types::{CaseSensitivity, MountPolicy, VfsFileType, VfsMetadata};

/// Backs a VFS mount with a host directory on disk.
///
/// Ensures paths never escape the configured root directory through
/// canonicalization, ancestor validation, and post-write containment checks.
pub struct PhysicalMount {
    root: PathBuf,
    canonical_root: PathBuf,
    policy: MountPolicy,
    case_sensitivity: CaseSensitivity,
}

impl PhysicalMount {
    /// Creates a new physical mount rooted at `root`.
    ///
    /// The root directory must exist.
    pub fn new(
        root: impl AsRef<Path>,
        policy: MountPolicy,
        case_sensitivity: CaseSensitivity,
    ) -> VfsResult<Self> {
        let root = root.as_ref().to_path_buf();
        if !root.exists() {
            return Err(VfsError::Io {
                path: root.display().to_string(),
                message: "mount root directory does not exist".into(),
            });
        }
        if !root.is_dir() {
            return Err(VfsError::Io {
                path: root.display().to_string(),
                message: "mount root is not a directory".into(),
            });
        }

        let canonical_root = fs::canonicalize(&root).map_err(|e| map_io_error(e, &root, "/"))?;

        Ok(Self {
            root,
            canonical_root,
            policy,
            case_sensitivity,
        })
    }

    /// Access the root path on the host.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Access the canonical root path on the host.
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    /// Resolves a relative virtual path to an absolute host path, preventing traversal and symlink escape.
    ///
    /// Handles both existing paths and nonexistent paths (by validating the nearest existing ancestor).
    fn resolve_path(&self, rel_path: &str) -> VfsResult<PathBuf> {
        let vpath = VfsPath::new(rel_path)?;
        if vpath.is_root() {
            return Ok(self.canonical_root.clone());
        }

        let mut current = self.canonical_root.clone();

        for component in vpath.components() {
            if self.case_sensitivity == CaseSensitivity::Sensitive {
                current.push(component);
            } else {
                let direct = current.join(component);
                if direct.exists() {
                    current = direct;
                } else if current.is_dir() {
                    match find_case_insensitive_entry(&current, component)? {
                        Some(matched) => current = matched,
                        None => current.push(component),
                    }
                } else {
                    current.push(component);
                }
            }

            // If the intermediate path exists on disk, immediately verify it doesn't escape root
            if current.exists() {
                let canonical =
                    fs::canonicalize(&current).map_err(|e| map_io_error(e, &current, rel_path))?;
                if !canonical.starts_with(&self.canonical_root) {
                    return Err(VfsError::PathTraversal(format!(
                        "path '{}' escaped mount root via symlink",
                        rel_path
                    )));
                }
                current = canonical;
            }
        }

        // Final verification for existing vs nonexistent paths
        if current.exists() {
            let canonical =
                fs::canonicalize(&current).map_err(|e| map_io_error(e, &current, rel_path))?;
            if !canonical.starts_with(&self.canonical_root) {
                return Err(VfsError::PathTraversal(format!(
                    "path '{}' escaped mount root via symlink",
                    rel_path
                )));
            }
            Ok(canonical)
        } else {
            // For nonexistent paths (e.g. pending write or create_dir), validate the nearest existing ancestor
            let nearest_ancestor = find_nearest_existing_ancestor(&current)?;
            let canonical_ancestor = fs::canonicalize(&nearest_ancestor)
                .map_err(|e| map_io_error(e, &nearest_ancestor, rel_path))?;
            if !canonical_ancestor.starts_with(&self.canonical_root) {
                return Err(VfsError::PathTraversal(format!(
                    "path '{}' escaped mount root via ancestor symlink",
                    rel_path
                )));
            }
            Ok(current)
        }
    }

    fn extract_canonical_rel_path(&self, path: &Path, fallback_rel: &str) -> String {
        if let Ok(stripped) = path.strip_prefix(&self.canonical_root) {
            let parts: Vec<String> = stripped
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            if !parts.is_empty() {
                return parts.join("/");
            }
        }
        fallback_rel.to_string()
    }
}

/// Finds the nearest existing ancestor directory by walking up `parent()` until `exists()` is true.
fn find_nearest_existing_ancestor(path: &Path) -> VfsResult<PathBuf> {
    let mut current = path;
    while !current.exists() {
        match current.parent() {
            Some(parent) => current = parent,
            None => {
                return Err(VfsError::PathTraversal(
                    "path has no existing ancestor within filesystem".into(),
                ))
            }
        }
    }
    Ok(current.to_path_buf())
}

/// Searches a directory for an entry matching `component` case-insensitively.
///
/// Priority:
/// 1. Exact case match if present.
/// 2. Deterministic sort of case-insensitive matches (lexicographical first).
fn find_case_insensitive_entry(dir: &Path, component: &str) -> VfsResult<Option<PathBuf>> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(map_io_error(e, dir, component));
        }
    };

    let mut matches = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| map_io_error(e, dir, component))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == component {
            // Exact match priority!
            return Ok(Some(entry.path()));
        }
        if name_str.eq_ignore_ascii_case(component) {
            matches.push(entry.path());
        }
    }

    if matches.is_empty() {
        Ok(None)
    } else {
        // Deterministic tie-breaker: sort lexicographically by file name
        matches.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        Ok(Some(matches.remove(0)))
    }
}

fn map_io_error(e: std::io::Error, path: &Path, rel_path: &str) -> VfsError {
    match e.kind() {
        std::io::ErrorKind::NotFound => {
            VfsError::NotFound(VfsPath::new(rel_path).unwrap_or_else(|_| VfsPath::root()))
        }
        std::io::ErrorKind::PermissionDenied => {
            VfsError::AccessDenied(format!("{}: permission denied", path.display()))
        }
        _ => VfsError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        },
    }
}

impl VfsBackend for PhysicalMount {
    fn backend_type(&self) -> &'static str {
        "physical"
    }

    fn exists(&self, rel_path: &str) -> bool {
        match self.resolve_path(rel_path) {
            Ok(path) => path.exists(),
            Err(_) => false,
        }
    }

    fn metadata(&self, rel_path: &str) -> VfsResult<VfsMetadata> {
        let path = self.resolve_path(rel_path)?;
        let meta = fs::metadata(&path).map_err(|e| map_io_error(e, &path, rel_path))?;

        let file_type = if meta.is_dir() {
            VfsFileType::Directory
        } else {
            VfsFileType::File
        };

        let modified_epoch_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);

        let read_only = !self.policy.is_writable() || meta.permissions().readonly();

        Ok(VfsMetadata {
            file_type,
            size_bytes: meta.len(),
            read_only,
            modified_epoch_ms,
        })
    }

    fn read(&self, rel_path: &str) -> VfsResult<Vec<u8>> {
        self.read_with_canonical_path(rel_path)
            .map(|(bytes, _)| bytes)
    }

    fn read_with_canonical_path(&self, rel_path: &str) -> VfsResult<(Vec<u8>, String)> {
        let path = self.resolve_path(rel_path)?;
        let bytes = fs::read(&path).map_err(|e| map_io_error(e, &path, rel_path))?;
        let canonical_rel = self.extract_canonical_rel_path(&path, rel_path);
        Ok((bytes, canonical_rel))
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> VfsResult<()> {
        if !self.policy.is_writable() {
            return Err(VfsError::ReadOnlyMount {
                mount_id: self.root.display().to_string(),
                path: VfsPath::new(rel_path).unwrap_or_else(|_| VfsPath::root()),
            });
        }

        let path = self.resolve_path(rel_path)?;
        if let Some(parent) = path.parent() {
            // Verify nearest existing ancestor before creating directories
            let nearest = find_nearest_existing_ancestor(parent)?;
            let canonical_nearest =
                fs::canonicalize(&nearest).map_err(|e| map_io_error(e, &nearest, rel_path))?;
            if !canonical_nearest.starts_with(&self.canonical_root) {
                return Err(VfsError::PathTraversal(format!(
                    "path '{}' escaped mount root via ancestor symlink",
                    rel_path
                )));
            }

            fs::create_dir_all(parent).map_err(|e| map_io_error(e, parent, rel_path))?;

            // Post-dir-create containment check
            let canonical_parent =
                fs::canonicalize(parent).map_err(|e| map_io_error(e, parent, rel_path))?;
            if !canonical_parent.starts_with(&self.canonical_root) {
                return Err(VfsError::PathTraversal(format!(
                    "path '{}' parent directory escaped mount root",
                    rel_path
                )));
            }
        }

        fs::write(&path, data).map_err(|e| map_io_error(e, &path, rel_path))?;

        // Post-write verification
        let canonical_written =
            fs::canonicalize(&path).map_err(|e| map_io_error(e, &path, rel_path))?;
        if !canonical_written.starts_with(&self.canonical_root) {
            return Err(VfsError::PathTraversal(format!(
                "written path '{}' escaped mount root",
                rel_path
            )));
        }

        Ok(())
    }

    fn create_dir(&self, rel_path: &str) -> VfsResult<()> {
        if !self.policy.is_writable() {
            return Err(VfsError::ReadOnlyMount {
                mount_id: self.root.display().to_string(),
                path: VfsPath::new(rel_path).unwrap_or_else(|_| VfsPath::root()),
            });
        }

        let path = self.resolve_path(rel_path)?;

        // Pre-create nearest existing ancestor check
        let nearest = find_nearest_existing_ancestor(&path)?;
        let canonical_nearest =
            fs::canonicalize(&nearest).map_err(|e| map_io_error(e, &nearest, rel_path))?;
        if !canonical_nearest.starts_with(&self.canonical_root) {
            return Err(VfsError::PathTraversal(format!(
                "path '{}' escaped mount root via ancestor symlink",
                rel_path
            )));
        }

        fs::create_dir_all(&path).map_err(|e| map_io_error(e, &path, rel_path))?;

        // Post-create containment verification
        let canonical_created =
            fs::canonicalize(&path).map_err(|e| map_io_error(e, &path, rel_path))?;
        if !canonical_created.starts_with(&self.canonical_root) {
            return Err(VfsError::PathTraversal(format!(
                "created directory '{}' escaped mount root",
                rel_path
            )));
        }

        Ok(())
    }

    fn read_dir(&self, rel_path: &str) -> VfsResult<Vec<BackendDirEntry>> {
        let path = self.resolve_path(rel_path)?;
        let read = fs::read_dir(&path).map_err(|e| map_io_error(e, &path, rel_path))?;

        let mut entries = Vec::new();
        for item in read {
            let item = item.map_err(|e| map_io_error(e, &path, rel_path))?;
            let name = item.file_name().to_string_lossy().to_string();
            let ft = item
                .file_type()
                .map_err(|e| map_io_error(e, &item.path(), rel_path))?;
            let file_type = if ft.is_dir() {
                VfsFileType::Directory
            } else {
                VfsFileType::File
            };
            entries.push(BackendDirEntry { name, file_type });
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }
}
