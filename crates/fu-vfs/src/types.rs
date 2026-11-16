use crate::path::VfsPath;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VfsFileType {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VfsMetadata {
    pub file_type: VfsFileType,
    pub size_bytes: u64,
    pub read_only: bool,
    pub modified_epoch_ms: Option<u64>,
}

impl VfsMetadata {
    pub fn file(size_bytes: u64, read_only: bool) -> Self {
        Self {
            file_type: VfsFileType::File,
            size_bytes,
            read_only,
            modified_epoch_ms: None,
        }
    }

    pub fn directory(read_only: bool) -> Self {
        Self {
            file_type: VfsFileType::Directory,
            size_bytes: 0,
            read_only,
            modified_epoch_ms: None,
        }
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        self.file_type == VfsFileType::File
    }

    #[inline]
    pub fn is_dir(&self) -> bool {
        self.file_type == VfsFileType::Directory
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VfsDirEntry {
    pub name: String,
    pub path: VfsPath,
    pub file_type: VfsFileType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MountPolicy {
    #[default]
    ReadOnly,
    ReadWrite,
}

impl MountPolicy {
    #[inline]
    pub fn is_writable(&self) -> bool {
        matches!(self, MountPolicy::ReadWrite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CaseSensitivity {
    #[default]
    Sensitive,
    Insensitive,
}

/// Output returned by [`crate::Vfs::read_with_provenance`].
///
/// Contains the file bytes along with the provenance details of the
/// winning mount that satisfied the read request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VfsReadOutput {
    /// Raw file bytes returned by the winning backend.
    pub bytes: Vec<u8>,
    /// Unique identifier of the mount that supplied the data.
    pub mount_id: String,
    /// Priority of the winning mount at the time of resolution.
    pub mount_priority: i32,
    /// Canonical relative path within the winning mount, preserving backend entry casing.
    pub canonical_relative_path: String,
}
