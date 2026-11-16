use crate::error::{VfsError, VfsResult};
use crate::types::{VfsFileType, VfsMetadata};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDirEntry {
    pub name: String,
    pub file_type: VfsFileType,
}

/// Abstract backend interface for a mounted VFS storage location.
///
/// Backends receive normalized relative paths with forward slashes (e.g. `"images/bg.png"`),
/// with `""` representing the root of the mount itself.
pub trait VfsBackend: Send + Sync {
    /// Friendly name describing the backend type (e.g. `"physical"`, `"memory"`, `"archive"`).
    fn backend_type(&self) -> &'static str;

    /// Checks if a file or directory exists at the relative path.
    fn exists(&self, rel_path: &str) -> bool;

    /// Queries metadata for a file or directory.
    fn metadata(&self, rel_path: &str) -> VfsResult<VfsMetadata>;

    /// Reads an entire file into memory.
    fn read(&self, rel_path: &str) -> VfsResult<Vec<u8>>;

    /// Reads an entire file into memory along with its canonical relative path (matching backend casing).
    fn read_with_canonical_path(&self, rel_path: &str) -> VfsResult<(Vec<u8>, String)> {
        let bytes = self.read(rel_path)?;
        Ok((bytes, rel_path.to_string()))
    }

    /// Reads an entire file as a UTF-8 string.
    fn read_to_string(&self, rel_path: &str) -> VfsResult<String> {
        let bytes = self.read(rel_path)?;
        String::from_utf8(bytes)
            .map_err(|e| VfsError::Backend(format!("invalid UTF-8 sequence: {e}")))
    }

    /// Writes data to a file, creating parent directories if supported.
    fn write(&self, rel_path: &str, data: &[u8]) -> VfsResult<()>;

    /// Creates a directory at the relative path.
    fn create_dir(&self, rel_path: &str) -> VfsResult<()>;

    /// Lists entries in a directory.
    fn read_dir(&self, rel_path: &str) -> VfsResult<Vec<BackendDirEntry>>;
}
