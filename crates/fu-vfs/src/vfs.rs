use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::error::{VfsError, VfsResult};
use crate::memory::MemoryMount;
use crate::mount::{Mount, MountInfo};
use crate::path::VfsPath;
use crate::physical::PhysicalMount;
use crate::types::{
    CaseSensitivity, MountPolicy, VfsDirEntry, VfsFileType, VfsMetadata, VfsReadOutput,
};

/// Consistent snapshot of the mount a write would target.
///
/// Both fields are observed in the same immutable borrow of [`Vfs`], so they
/// describe the same topology state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsWriteTarget {
    /// Top write candidate, if any enabled mount covers the path.
    pub mount: Option<MountInfo>,
    /// Unique instance of the selected mount, if one covers the path.
    ///
    /// Mount ids may be reused after an unmount. This value changes when a new
    /// `Mount` is installed under the same id and stays stable across
    /// enable/disable.
    pub mount_instance: Option<u64>,
    /// Topology generation observed together with `mount`.
    pub topology_generation: u64,
}

/// Multi-mount Virtual File System coordinator.
///
/// Manages multiple content sources with deterministic priority, cascading fallback
/// resolution, union directory listing, and mod isolation.
#[derive(Default)]
pub struct Vfs {
    mounts: BTreeMap<String, Mount>,
    mount_instances: BTreeMap<String, u64>,
    next_mount_instance: u64,
    topology_generation: u64,
}

impl Vfs {
    /// Creates a new empty VFS.
    pub fn new() -> Self {
        Self {
            mounts: BTreeMap::new(),
            mount_instances: BTreeMap::new(),
            next_mount_instance: 0,
            topology_generation: 0,
        }
    }

    /// Monotonically increasing counter incremented whenever mount topology changes.
    #[inline]
    pub fn topology_generation(&self) -> u64 {
        self.topology_generation
    }

    /// Returns the unique instance id of a currently mounted id, if it exists.
    ///
    /// A mount id may be reused after `unmount`; a remounted id receives a new
    /// instance id. Enable/disable does not change the instance id.
    pub fn mount_instance_id(&self, id: &str) -> Option<u64> {
        self.mount_instances.get(id).copied()
    }

    /// Adds a mount. Returns an error if a mount with the same `id` already exists.
    pub fn mount(&mut self, mount: Mount) -> VfsResult<()> {
        if self.mounts.contains_key(&mount.id) {
            return Err(VfsError::DuplicateMount(mount.id));
        }
        self.next_mount_instance = self.next_mount_instance.checked_add(1).unwrap_or(1);
        let instance = self.next_mount_instance;
        self.mount_instances.insert(mount.id.clone(), instance);
        self.mounts.insert(mount.id.clone(), mount);
        self.topology_generation += 1;
        Ok(())
    }

    /// Convenience helper to mount a host directory.
    pub fn mount_physical(
        &mut self,
        id: impl Into<String>,
        mount_point: &str,
        root: impl AsRef<Path>,
        priority: i32,
        policy: MountPolicy,
        case: CaseSensitivity,
    ) -> VfsResult<()> {
        let vpath = VfsPath::new(mount_point)?;
        let backend = PhysicalMount::new(root, policy, case)?;
        let mount = Mount::new(id, vpath, priority, policy, case, Arc::new(backend));
        self.mount(mount)
    }

    /// Convenience helper to mount an in-memory backend.
    pub fn mount_memory(
        &mut self,
        id: impl Into<String>,
        mount_point: &str,
        priority: i32,
        policy: MountPolicy,
        case: CaseSensitivity,
    ) -> VfsResult<Arc<MemoryMount>> {
        let vpath = VfsPath::new(mount_point)?;
        let backend = Arc::new(MemoryMount::new(case));
        let mount = Mount::new(id, vpath, priority, policy, case, backend.clone());
        self.mount(mount)?;
        Ok(backend)
    }

    /// Removes a mount by its identifier.
    pub fn unmount(&mut self, id: &str) -> VfsResult<()> {
        let res = self
            .mounts
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| VfsError::MountNotFound(id.to_string()));
        if res.is_ok() {
            self.mount_instances.remove(id);
            self.topology_generation += 1;
        }
        res
    }

    /// Enables or disables a mount.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> VfsResult<()> {
        let mount = self
            .mounts
            .get_mut(id)
            .ok_or_else(|| VfsError::MountNotFound(id.to_string()))?;
        if mount.enabled != enabled {
            mount.enabled = enabled;
            self.topology_generation += 1;
        }
        Ok(())
    }

    /// Queries information about a mount.
    pub fn get_mount_info(&self, id: &str) -> Option<MountInfo> {
        self.mounts.get(id).map(Mount::info)
    }

    /// Lists all registered mounts.
    pub fn list_mounts(&self) -> Vec<MountInfo> {
        let mut list: Vec<_> = self.mounts.values().map(Mount::info).collect();
        list.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        list
    }

    /// Finds all matching enabled mounts for a given virtual path, sorted by:
    /// 1. Longest `mount_point` prefix first (most specific path match).
    /// 2. Highest `priority` first.
    /// 3. Stable tie-breaker: `id`.
    fn sorted_candidate_mounts<'a>(&'a self, path: &'a VfsPath) -> Vec<(&'a Mount, &'a str)> {
        let mut matches = Vec::new();
        for mount in self.mounts.values() {
            if !mount.enabled {
                continue;
            }
            if let Some(rel) = path.strip_prefix(&mount.mount_point) {
                matches.push((mount, rel));
            }
        }

        matches.sort_by(|(a, _), (b, _)| {
            b.mount_point
                .as_str()
                .len()
                .cmp(&a.mount_point.as_str().len())
                .then_with(|| b.priority.cmp(&a.priority))
                .then_with(|| a.id.cmp(&b.id))
        });

        matches
    }

    /// Checks if a file or directory exists across mounted sources.
    pub fn exists(&self, path: impl AsRef<str>) -> bool {
        let vpath = match VfsPath::new(path.as_ref()) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let candidates = self.sorted_candidate_mounts(&vpath);
        for (mount, rel) in candidates {
            if mount.backend.exists(rel) {
                return true;
            }
        }

        // Also check if any mount is rooted directly beneath this path
        for mount in self.mounts.values() {
            if mount.enabled && mount.mount_point.starts_with(&vpath) && mount.mount_point != vpath
            {
                return true;
            }
        }

        false
    }

    /// Queries metadata for a file or directory across mounted sources.
    pub fn metadata(&self, path: impl AsRef<str>) -> VfsResult<VfsMetadata> {
        let vpath = VfsPath::new(path.as_ref())?;
        let candidates = self.sorted_candidate_mounts(&vpath);

        for (mount, rel) in candidates {
            match mount.backend.metadata(rel) {
                Ok(meta) => return Ok(meta),
                Err(VfsError::NotFound(_)) => continue,
                Err(err) => return Err(err),
            }
        }

        // Check if this path is an implicit virtual directory holding sub-mounts
        for mount in self.mounts.values() {
            if mount.enabled && mount.mount_point.starts_with(&vpath) && mount.mount_point != vpath
            {
                return Ok(VfsMetadata::directory(true));
            }
        }

        Err(VfsError::NotFound(vpath))
    }

    /// Reads an entire file into memory with cascading fallback across mounts, returning read provenance.
    pub fn read_with_provenance(&self, path: impl AsRef<str>) -> VfsResult<VfsReadOutput> {
        let vpath = VfsPath::new(path.as_ref())?;
        let candidates = self.sorted_candidate_mounts(&vpath);

        for (mount, rel) in candidates {
            match mount.backend.read_with_canonical_path(rel) {
                Ok((bytes, canonical_rel)) => {
                    return Ok(VfsReadOutput {
                        bytes,
                        mount_id: mount.id.clone(),
                        mount_priority: mount.priority,
                        canonical_relative_path: canonical_rel,
                    });
                }
                Err(VfsError::NotFound(_)) => continue,
                Err(err) => return Err(err),
            }
        }

        Err(VfsError::NotFound(vpath))
    }

    /// Returns a consistent snapshot of the mount a write to `path` would target,
    /// together with the topology generation observed in the same borrow.
    ///
    /// This is intentionally not [`Vfs::resolve_mount`]: `resolve_mount` answers
    /// which mount currently serves an existing entry, while this answers which
    /// mount a write (or create) would dispatch to, including when the file does
    /// not exist yet. Returns `mount: None` when no enabled mount covers the path;
    /// the generation is still reported so callers can detect a topology change.
    pub fn write_target(&self, path: impl AsRef<str>) -> VfsResult<VfsWriteTarget> {
        let vpath = VfsPath::new(path.as_ref())?;
        let generation = self.topology_generation();
        let mount = self
            .sorted_candidate_mounts(&vpath)
            .first()
            .map(|(mount, _)| mount.info());
        let mount_instance = mount
            .as_ref()
            .and_then(|mount| self.mount_instance_id(&mount.id));
        Ok(VfsWriteTarget {
            mount,
            mount_instance,
            topology_generation: generation,
        })
    }

    /// Reads an entire file into memory with cascading fallback across mounts.
    pub fn read(&self, path: impl AsRef<str>) -> VfsResult<Vec<u8>> {
        self.read_with_provenance(path).map(|output| output.bytes)
    }

    /// Reads an entire file as a UTF-8 string with cascading fallback across mounts.
    pub fn read_to_string(&self, path: impl AsRef<str>) -> VfsResult<String> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes)
            .map_err(|e| VfsError::Backend(format!("invalid UTF-8 sequence: {e}")))
    }

    /// Writes data to a file. Dispatches strictly to the active (top priority) mount.
    ///
    /// # Errors
    /// Returns [`VfsError::ReadOnlyMount`] if the matching mount is read-only.
    /// Returns [`VfsError::NoWritableMount`] if no mount matches the path.
    pub fn write(&self, path: impl AsRef<str>, data: &[u8]) -> VfsResult<()> {
        let vpath = VfsPath::new(path.as_ref())?;
        let candidates = self.sorted_candidate_mounts(&vpath);
        if candidates.is_empty() {
            return Err(VfsError::NoWritableMount(vpath));
        }

        // The top candidate (highest prefix specificity, then highest priority) governs write permissions.
        // A higher-priority read-only mount explicitly blocks writes to prevent mutating shadowed layers.
        let (top_mount, top_rel) = &candidates[0];
        if !top_mount.policy.is_writable() {
            return Err(VfsError::ReadOnlyMount {
                mount_id: top_mount.id.clone(),
                path: vpath,
            });
        }

        top_mount.backend.write(top_rel, data)
    }

    /// Creates a directory on the active (top priority) writable mount.
    pub fn create_dir(&self, path: impl AsRef<str>) -> VfsResult<()> {
        let vpath = VfsPath::new(path.as_ref())?;
        let candidates = self.sorted_candidate_mounts(&vpath);
        if candidates.is_empty() {
            return Err(VfsError::NoWritableMount(vpath));
        }

        let (top_mount, top_rel) = &candidates[0];
        if !top_mount.policy.is_writable() {
            return Err(VfsError::ReadOnlyMount {
                mount_id: top_mount.id.clone(),
                path: vpath,
            });
        }

        top_mount.backend.create_dir(top_rel)
    }

    /// Performs union directory listing across all matching mounts.
    ///
    /// Entries are deduplicated with an authority-based collision policy:
    /// - Higher-priority mounts win exact-name collisions.
    /// - If the winning entry came from a case-insensitive mount, all case-insensitive
    ///   variations from lower-priority mounts are shadowed.
    /// - If the winning entry came from a case-sensitive mount, distinct case variations
    ///   from lower mounts that remain reachable are preserved.
    /// - Synthetic child mount directory entries reflect the exact spelling required to
    ///   access that mount point and do not leak child internal case-sensitivity to the parent.
    /// - Actual backend entries take precedence over implicit directories from nested submounts.
    /// - The winning entry retains its original spelling and file type.
    /// - Results are sorted alphabetically.
    pub fn read_dir(&self, path: impl AsRef<str>) -> VfsResult<Vec<VfsDirEntry>> {
        let vpath = VfsPath::new(path.as_ref())?;
        let candidates = self.sorted_candidate_mounts(&vpath);

        struct PotentialEntry {
            name: String,
            entry: VfsDirEntry,
            prefix_len: usize,
            priority: i32,
            mount_id: String,
            case_sensitivity: CaseSensitivity,
            is_implicit_submount: bool,
        }

        struct AcceptedEntry {
            name: String,
            entry: VfsDirEntry,
            case_sensitivity: CaseSensitivity,
        }

        fn is_shadowed(accepted: &[AcceptedEntry], candidate_name: &str) -> bool {
            for existing in accepted {
                // Exact match is always shadowed by a higher-authority entry
                if existing.name == candidate_name {
                    return true;
                }
                // If the higher-authority existing entry is from an insensitive mount,
                // it captures all case variations, shadowing this candidate.
                if existing.case_sensitivity == CaseSensitivity::Insensitive
                    && existing.name.eq_ignore_ascii_case(candidate_name)
                {
                    return true;
                }
            }
            false
        }

        let mut potential_entries: Vec<PotentialEntry> = Vec::new();
        let mut any_found = false;

        // 1. Collect child mount points directly or indirectly nested under this path.
        for mount in self.mounts.values() {
            if !mount.enabled {
                continue;
            }
            if let Some(rel) = mount.mount_point.strip_prefix(&vpath) {
                if !rel.is_empty() {
                    any_found = true;
                    let first_seg = rel.split('/').next().unwrap_or(rel).to_string();
                    let entry_path = vpath.join(&first_seg)?;
                    let is_direct = mount.mount_point == entry_path;
                    let prefix_len = if is_direct {
                        entry_path.as_str().len()
                    } else {
                        vpath.as_str().len()
                    };

                    potential_entries.push(PotentialEntry {
                        name: first_seg.clone(),
                        entry: VfsDirEntry {
                            name: first_seg,
                            path: entry_path,
                            file_type: VfsFileType::Directory,
                        },
                        prefix_len,
                        priority: mount.priority,
                        mount_id: mount.id.clone(),
                        case_sensitivity: CaseSensitivity::Sensitive, // Virtual mount point routing is exact
                        is_implicit_submount: !is_direct,
                    });
                }
            }
        }

        // 2. Query candidate mounts for this directory
        for (mount, rel) in candidates {
            match mount.backend.read_dir(rel) {
                Ok(backend_entries) => {
                    any_found = true;
                    for b_entry in backend_entries {
                        let entry_path = vpath.join(&b_entry.name)?;
                        potential_entries.push(PotentialEntry {
                            name: b_entry.name.clone(),
                            entry: VfsDirEntry {
                                name: b_entry.name,
                                path: entry_path,
                                file_type: b_entry.file_type,
                            },
                            prefix_len: mount.mount_point.as_str().len(),
                            priority: mount.priority,
                            mount_id: mount.id.clone(),
                            case_sensitivity: mount.case_sensitivity,
                            is_implicit_submount: false,
                        });
                    }
                }
                Err(VfsError::NotFound(_)) => continue,
                Err(err) => return Err(err),
            }
        }

        if !any_found {
            return Err(VfsError::NotFound(vpath));
        }

        // 3. Sort all potential entries by authority:
        //    a. Non-implicit entries (actual backend files/dirs or direct mounts) before implicit submounts
        //    b. Longest matching mount-point prefix first (prefix specificity)
        //    c. Highest priority first
        //    d. Stable mount ID ascending
        //    e. Stable entry name ascending
        potential_entries.sort_by(
            |a, b| match (a.is_implicit_submount, b.is_implicit_submount) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => b
                    .prefix_len
                    .cmp(&a.prefix_len)
                    .then_with(|| b.priority.cmp(&a.priority))
                    .then_with(|| a.mount_id.cmp(&b.mount_id))
                    .then_with(|| a.name.cmp(&b.name)),
            },
        );

        // 4. Deduplicate entries according to authority and case sensitivity
        let mut accepted: Vec<AcceptedEntry> = Vec::new();
        for candidate in potential_entries {
            if !is_shadowed(&accepted, &candidate.name) {
                accepted.push(AcceptedEntry {
                    name: candidate.name,
                    entry: candidate.entry,
                    case_sensitivity: candidate.case_sensitivity,
                });
            }
        }

        let mut entries: Vec<VfsDirEntry> = accepted.into_iter().map(|a| a.entry).collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Identifies which mount resolves a specific virtual path.
    ///
    /// Useful for asset attribution, mod inspection, and debugging.
    pub fn resolve_mount(&self, path: impl AsRef<str>) -> VfsResult<MountInfo> {
        let vpath = VfsPath::new(path.as_ref())?;
        let candidates = self.sorted_candidate_mounts(&vpath);

        for (mount, rel) in candidates {
            match mount.backend.metadata(rel) {
                Ok(_) => return Ok(mount.info()),
                Err(VfsError::NotFound(_)) => continue,
                Err(err) => return Err(err),
            }
        }

        Err(VfsError::NotFound(vpath))
    }
}
