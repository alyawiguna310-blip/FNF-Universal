use std::collections::BTreeMap;
use std::sync::RwLock;

use crate::backend::{BackendDirEntry, VfsBackend};
use crate::error::{VfsError, VfsResult};
use crate::path::VfsPath;
use crate::types::{CaseSensitivity, VfsFileType, VfsMetadata};

#[derive(Debug, Clone)]
enum MemoryNode {
    File {
        content: Vec<u8>,
        modified_epoch_ms: Option<u64>,
    },
    Directory,
}

/// An in-memory filesystem backend.
///
/// Thread-safe and independent of the physical disk. Useful for tests,
/// dynamic/generated assets, and isolated mock environments.
pub struct MemoryMount {
    case_sensitivity: CaseSensitivity,
    // Maps normalized relative path segments to nodes
    // e.g. ["images", "bg.png"] -> MemoryNode::File
    nodes: RwLock<BTreeMap<Vec<String>, MemoryNode>>,
}

impl MemoryMount {
    /// Creates a new empty in-memory mount.
    pub fn new(case_sensitivity: CaseSensitivity) -> Self {
        let mut map = BTreeMap::new();
        // Root directory
        map.insert(Vec::new(), MemoryNode::Directory);
        Self {
            case_sensitivity,
            nodes: RwLock::new(map),
        }
    }

    /// Helper to insert a file directly into the memory mount.
    pub fn set_file(&self, rel_path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        let segments = self.parse_segments(rel_path)?;
        if segments.is_empty() {
            return Err(VfsError::InvalidPath("cannot overwrite mount root".into()));
        }

        let mut map = self
            .nodes
            .write()
            .map_err(|e| VfsError::Backend(e.to_string()))?;

        // 1. Resolve and validate all parent segments: none can be an existing file
        let mut resolved_parent = Vec::with_capacity(segments.len() - 1);
        let mut resolved_parents = Vec::with_capacity(segments.len() - 1);
        for seg in &segments[..segments.len() - 1] {
            let mut candidate = resolved_parent.clone();
            candidate.push(seg.clone());
            if let Some(existing) = self.find_key(&map, &candidate) {
                if matches!(map[existing], MemoryNode::File { .. }) {
                    return Err(VfsError::Io {
                        path: rel_path.to_string(),
                        message: "path segment conflicts with an existing file".into(),
                    });
                }
                resolved_parent = existing.clone();
            } else {
                resolved_parent = candidate;
            }
            resolved_parents.push(resolved_parent.clone());
        }

        // 2. Construct final key using resolved parent path plus requested filename
        let filename = segments.last().unwrap().clone();
        let mut final_key = resolved_parent;
        final_key.push(filename);

        // 3. Validate target: cannot overwrite an existing directory
        if let Some(existing_key) = self.find_key(&map, &final_key) {
            if matches!(map[existing_key], MemoryNode::Directory) {
                return Err(VfsError::Io {
                    path: rel_path.to_string(),
                    message: "cannot overwrite existing directory with a file".into(),
                });
            }
        }

        // 4. Ensure parent directories exist
        for parent in resolved_parents {
            map.entry(parent).or_insert(MemoryNode::Directory);
        }

        // 5. Insert file
        map.insert(
            final_key,
            MemoryNode::File {
                content: content.into(),
                modified_epoch_ms: None,
            },
        );
        Ok(())
    }

    fn parse_segments(&self, rel_path: &str) -> VfsResult<Vec<String>> {
        let path = VfsPath::new(rel_path)?;
        Ok(path.components().map(|s| s.to_string()).collect())
    }

    fn find_key<'a>(
        &self,
        map: &'a BTreeMap<Vec<String>, MemoryNode>,
        target: &[String],
    ) -> Option<&'a Vec<String>> {
        if self.case_sensitivity == CaseSensitivity::Sensitive {
            map.get_key_value(target).map(|(k, _)| k)
        } else {
            // Exact match priority first
            if let Some((k, _)) = map.get_key_value(target) {
                return Some(k);
            }
            // Deterministic case-insensitive search: BTreeMap keys are sorted lexicographically
            map.keys().find(|k| {
                if k.len() != target.len() {
                    return false;
                }
                k.iter()
                    .zip(target.iter())
                    .all(|(a, b)| a.eq_ignore_ascii_case(b))
            })
        }
    }
}

impl VfsBackend for MemoryMount {
    fn backend_type(&self) -> &'static str {
        "memory"
    }

    fn exists(&self, rel_path: &str) -> bool {
        let segments = match self.parse_segments(rel_path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let map = match self.nodes.read() {
            Ok(m) => m,
            Err(_) => return false,
        };
        self.find_key(&map, &segments).is_some()
    }

    fn metadata(&self, rel_path: &str) -> VfsResult<VfsMetadata> {
        let segments = self.parse_segments(rel_path)?;
        let map = self
            .nodes
            .read()
            .map_err(|e| VfsError::Backend(e.to_string()))?;
        let key = self.find_key(&map, &segments).ok_or_else(|| {
            VfsError::NotFound(VfsPath::new(rel_path).unwrap_or_else(|_| VfsPath::root()))
        })?;

        match &map[key] {
            MemoryNode::File {
                content,
                modified_epoch_ms,
            } => Ok(VfsMetadata {
                file_type: VfsFileType::File,
                size_bytes: content.len() as u64,
                read_only: false,
                modified_epoch_ms: *modified_epoch_ms,
            }),
            MemoryNode::Directory => Ok(VfsMetadata {
                file_type: VfsFileType::Directory,
                size_bytes: 0,
                read_only: false,
                modified_epoch_ms: None,
            }),
        }
    }

    fn read(&self, rel_path: &str) -> VfsResult<Vec<u8>> {
        self.read_with_canonical_path(rel_path)
            .map(|(bytes, _)| bytes)
    }

    fn read_with_canonical_path(&self, rel_path: &str) -> VfsResult<(Vec<u8>, String)> {
        let segments = self.parse_segments(rel_path)?;
        let map = self
            .nodes
            .read()
            .map_err(|e| VfsError::Backend(e.to_string()))?;
        let key = self.find_key(&map, &segments).ok_or_else(|| {
            VfsError::NotFound(VfsPath::new(rel_path).unwrap_or_else(|_| VfsPath::root()))
        })?;

        match &map[key] {
            MemoryNode::File { content, .. } => Ok((content.clone(), key.join("/"))),
            MemoryNode::Directory => Err(VfsError::Io {
                path: rel_path.to_string(),
                message: "cannot read directory as a file".into(),
            }),
        }
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> VfsResult<()> {
        let segments = self.parse_segments(rel_path)?;
        if segments.is_empty() {
            return Err(VfsError::InvalidPath("cannot overwrite mount root".into()));
        }
        let mut map = self
            .nodes
            .write()
            .map_err(|e| VfsError::Backend(e.to_string()))?;

        // 1. Resolve segments adopting existing casing and validate intermediate parent segments
        let mut resolved = Vec::with_capacity(segments.len());
        for (idx, seg) in segments.iter().enumerate() {
            let is_last = idx == segments.len() - 1;
            let mut candidate = resolved.clone();
            candidate.push(seg.clone());

            if let Some(existing) = self.find_key(&map, &candidate) {
                if !is_last && matches!(map[existing], MemoryNode::File { .. }) {
                    return Err(VfsError::Io {
                        path: rel_path.to_string(),
                        message: "path segment conflicts with an existing file".into(),
                    });
                }
                resolved = existing.clone();
            } else {
                resolved = candidate;
            }
        }

        // 2. Validate target: cannot overwrite an existing directory
        if let Some(node) = map.get(&resolved) {
            if matches!(node, MemoryNode::Directory) {
                return Err(VfsError::Io {
                    path: rel_path.to_string(),
                    message: "cannot overwrite existing directory with a file".into(),
                });
            }
        }

        // 3. Ensure parent directories exist
        for i in 1..resolved.len() {
            let parent = resolved[..i].to_vec();
            map.entry(parent).or_insert(MemoryNode::Directory);
        }

        // 4. Insert or update file at resolved path
        map.insert(
            resolved,
            MemoryNode::File {
                content: data.to_vec(),
                modified_epoch_ms: None,
            },
        );
        Ok(())
    }

    fn create_dir(&self, rel_path: &str) -> VfsResult<()> {
        let segments = self.parse_segments(rel_path)?;
        if segments.is_empty() {
            return Ok(());
        }
        let mut map = self
            .nodes
            .write()
            .map_err(|e| VfsError::Backend(e.to_string()))?;

        let mut resolved = Vec::with_capacity(segments.len());
        for seg in segments {
            let mut candidate = resolved.clone();
            candidate.push(seg);
            if let Some(existing) = self.find_key(&map, &candidate) {
                if let MemoryNode::File { .. } = map[existing] {
                    return Err(VfsError::Io {
                        path: rel_path.to_string(),
                        message: "path segment conflicts with an existing file".into(),
                    });
                }
                resolved = existing.clone();
            } else {
                map.insert(candidate.clone(), MemoryNode::Directory);
                resolved = candidate;
            }
        }
        Ok(())
    }

    fn read_dir(&self, rel_path: &str) -> VfsResult<Vec<BackendDirEntry>> {
        let segments = self.parse_segments(rel_path)?;
        let map = self
            .nodes
            .read()
            .map_err(|e| VfsError::Backend(e.to_string()))?;

        // Verify rel_path is a directory
        if !segments.is_empty() {
            let key = self.find_key(&map, &segments).ok_or_else(|| {
                VfsError::NotFound(VfsPath::new(rel_path).unwrap_or_else(|_| VfsPath::root()))
            })?;
            if !matches!(map[key], MemoryNode::Directory) {
                return Err(VfsError::Io {
                    path: rel_path.to_string(),
                    message: "not a directory".into(),
                });
            }
        }

        let prefix_len = segments.len();
        let mut entries = Vec::new();
        let mut seen_exact = std::collections::BTreeSet::new();
        let mut seen_ci = std::collections::BTreeSet::new();

        for (k, node) in map.iter() {
            if k.len() == prefix_len + 1 {
                let matches_prefix = if self.case_sensitivity == CaseSensitivity::Sensitive {
                    k[..prefix_len] == segments[..]
                } else {
                    k[..prefix_len]
                        .iter()
                        .zip(segments.iter())
                        .all(|(a, b)| a.eq_ignore_ascii_case(b))
                };

                if matches_prefix {
                    let name = k[prefix_len].clone();
                    let should_include = if self.case_sensitivity == CaseSensitivity::Sensitive {
                        seen_exact.insert(name.clone())
                    } else {
                        seen_ci.insert(name.to_ascii_lowercase())
                    };

                    if should_include {
                        let file_type = match node {
                            MemoryNode::File { .. } => VfsFileType::File,
                            MemoryNode::Directory => VfsFileType::Directory,
                        };
                        entries.push(BackendDirEntry { name, file_type });
                    }
                }
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_mount_read_write() {
        let mem = MemoryMount::new(CaseSensitivity::Sensitive);
        mem.write("hello.txt", b"world").unwrap();
        assert!(mem.exists("hello.txt"));
        assert_eq!(mem.read_to_string("hello.txt").unwrap(), "world");
        assert_eq!(mem.metadata("hello.txt").unwrap().size_bytes, 5);
    }

    #[test]
    fn memory_mount_case_insensitivity() {
        let mem = MemoryMount::new(CaseSensitivity::Insensitive);
        mem.write("Images/Characters/BF.png", b"bf_texture")
            .unwrap();

        assert!(mem.exists("images/characters/bf.png"));
        assert!(mem.exists("IMAGES/CHARACTERS/BF.PNG"));
        assert_eq!(mem.read("images/characters/bf.png").unwrap(), b"bf_texture");
    }

    #[test]
    fn memory_mount_read_dir() {
        let mem = MemoryMount::new(CaseSensitivity::Sensitive);
        mem.write("data/characters/bf.xml", b"<xml/>").unwrap();
        mem.write("data/characters/dad.xml", b"<xml/>").unwrap();
        mem.create_dir("data/stages").unwrap();

        let entries = mem.read_dir("data").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|e| e.name == "characters" && e.file_type == VfsFileType::Directory));
        assert!(entries
            .iter()
            .any(|e| e.name == "stages" && e.file_type == VfsFileType::Directory));
    }

    #[test]
    fn set_file_and_write_reject_root_path() {
        let mem = MemoryMount::new(CaseSensitivity::Sensitive);
        assert!(matches!(
            mem.set_file("", b"root_content"),
            Err(VfsError::InvalidPath(_))
        ));
        assert!(matches!(
            mem.set_file("/", b"root_content"),
            Err(VfsError::InvalidPath(_))
        ));
        assert!(matches!(
            mem.write("", b"root_content"),
            Err(VfsError::InvalidPath(_))
        ));
        assert!(matches!(
            mem.write("/", b"root_content"),
            Err(VfsError::InvalidPath(_))
        ));

        // Root should still be a directory
        assert_eq!(mem.metadata("/").unwrap().file_type, VfsFileType::Directory);
    }

    #[test]
    fn writing_and_set_file_beneath_existing_file_fails() {
        let mem = MemoryMount::new(CaseSensitivity::Sensitive);
        mem.set_file("songs/test.ogg", b"audio").unwrap();

        // set_file beneath existing file fails
        let set_err = mem.set_file("songs/test.ogg/nested.txt", b"fail");
        assert!(matches!(set_err, Err(VfsError::Io { .. })));

        // write beneath existing file fails
        let write_err = mem.write("songs/test.ogg/nested.txt", b"fail");
        assert!(matches!(write_err, Err(VfsError::Io { .. })));

        // create_dir beneath existing file fails
        let dir_err = mem.create_dir("songs/test.ogg/folder");
        assert!(matches!(dir_err, Err(VfsError::Io { .. })));
    }

    #[test]
    fn writing_and_set_file_over_existing_directory_fails() {
        let mem = MemoryMount::new(CaseSensitivity::Sensitive);
        mem.create_dir("data/stages").unwrap();

        // Cannot overwrite directory with a file via set_file
        let set_err = mem.set_file("data/stages", b"not_a_dir");
        assert!(matches!(set_err, Err(VfsError::Io { .. })));

        // Cannot overwrite directory with a file via write
        let write_err = mem.write("data/stages", b"not_a_dir");
        assert!(matches!(write_err, Err(VfsError::Io { .. })));

        // Case-insensitive variant
        let mem_ci = MemoryMount::new(CaseSensitivity::Insensitive);
        mem_ci.create_dir("Data/Stages").unwrap();
        assert!(matches!(
            mem_ci.write("data/stages", b"fail"),
            Err(VfsError::Io { .. })
        ));
        assert!(matches!(
            mem_ci.set_file("data/stages", b"fail"),
            Err(VfsError::Io { .. })
        ));
    }

    #[test]
    fn case_insensitive_operations_do_not_create_duplicate_logical_paths() {
        let mem = MemoryMount::new(CaseSensitivity::Insensitive);

        // 1. Write file with mixed casing
        mem.write("Images/Characters/BF.png", b"version1").unwrap();

        // 2. Overwrite file with different casing
        mem.write("images/characters/bf.png", b"version2").unwrap();

        // Verify only ONE file exists in directory and content was updated
        let char_entries = mem.read_dir("images/characters").unwrap();
        assert_eq!(char_entries.len(), 1);
        assert_eq!(char_entries[0].name, "BF.png");
        assert_eq!(
            mem.read_to_string("images/characters/bf.png").unwrap(),
            "version2"
        );
        assert_eq!(
            mem.read_to_string("IMAGES/CHARACTERS/BF.PNG").unwrap(),
            "version2"
        );

        // 3. Write second file under different directory casing
        mem.write("IMAGES/CHARACTERS/dad.png", b"dad").unwrap();

        // Verify directory tree did not duplicate intermediate directories
        let root_entries = mem.read_dir("").unwrap();
        assert_eq!(root_entries.len(), 1);
        assert_eq!(root_entries[0].name, "Images");

        let img_entries = mem.read_dir("images").unwrap();
        assert_eq!(img_entries.len(), 1);
        assert_eq!(img_entries[0].name, "Characters");

        let char_entries2 = mem.read_dir("images/characters").unwrap();
        assert_eq!(char_entries2.len(), 2);
    }

    #[test]
    fn set_file_with_differently_cased_parents_reuses_existing_directory_keys() {
        let mem = MemoryMount::new(CaseSensitivity::Insensitive);

        // Insert first file with mixed casing parent
        mem.set_file("Data/Stages/castle.xml", b"stage1").unwrap();

        // Insert second file with lowercase parent
        mem.set_file("data/stages/temple.xml", b"stage2").unwrap();

        // Verify root contains only "Data"
        let root_entries = mem.read_dir("").unwrap();
        assert_eq!(root_entries.len(), 1);
        assert_eq!(root_entries[0].name, "Data");

        // Verify "Data" contains only "Stages"
        let data_entries = mem.read_dir("data").unwrap();
        assert_eq!(data_entries.len(), 1);
        assert_eq!(data_entries[0].name, "Stages");

        // Verify "Stages" contains both files
        let stage_entries = mem.read_dir("data/stages").unwrap();
        assert_eq!(stage_entries.len(), 2);
        assert!(stage_entries.iter().any(|e| e.name == "castle.xml"));
        assert!(stage_entries.iter().any(|e| e.name == "temple.xml"));
    }

    #[test]
    fn intentional_final_component_case_collisions_remain_deterministic() {
        let mem = MemoryMount::new(CaseSensitivity::Insensitive);

        // Insert two entries that differ only in casing at the filename level
        mem.set_file("items/item.DAT", b"lower_item").unwrap();
        mem.set_file("items/ITEM.dat", b"upper_item").unwrap();

        // Exact match for "item.DAT" returns "lower_item"
        assert_eq!(mem.read_to_string("items/item.DAT").unwrap(), "lower_item");

        // Exact match for "ITEM.dat" returns "upper_item"
        assert_eq!(mem.read_to_string("items/ITEM.dat").unwrap(), "upper_item");

        // Non-exact match for "Item.Dat" deterministically chooses the first in sorted lexicographical order
        // In ASCII sort: 'I' comes before 'i', so "ITEM.dat" is chosen.
        let resolved = mem.read_to_string("items/Item.Dat").unwrap();
        assert_eq!(resolved, "upper_item");
    }

    #[test]
    fn read_dir_case_insensitive_deduplication() {
        let mem = MemoryMount::new(CaseSensitivity::Insensitive);

        // Insert two files differing only by case
        mem.set_file("assets/file.txt", b"lower").unwrap();
        mem.set_file("assets/FILE.TXT", b"upper").unwrap();

        let entries = mem.read_dir("assets").unwrap();
        // Should deduplicate to exactly 1 entry
        assert_eq!(entries.len(), 1);

        // Deterministic winner: "FILE.TXT" ('F' < 'f')
        assert_eq!(entries[0].name, "FILE.TXT");
        assert_eq!(entries[0].file_type, VfsFileType::File);
    }

    #[test]
    fn read_dir_case_sensitive_retains_both_names() {
        let mem = MemoryMount::new(CaseSensitivity::Sensitive);

        // Insert two files differing only by case
        mem.set_file("assets/file.txt", b"lower").unwrap();
        mem.set_file("assets/FILE.TXT", b"upper").unwrap();

        let entries = mem.read_dir("assets").unwrap();
        // Should retain both distinct names
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "FILE.TXT");
        assert_eq!(entries[0].file_type, VfsFileType::File);
        assert_eq!(entries[1].name, "file.txt");
        assert_eq!(entries[1].file_type, VfsFileType::File);
    }
}
