//! Virtual File System (VFS) foundation for FNF Universal.
//!
//! Provides a platform-independent virtual file system supporting:
//! - Multi-mount content overlays with deterministic priority (base assets, mods, user data).
//! - Cascading fallback resolution.
//! - Union directory listing across active mounts.
//! - Security boundaries preventing path traversal and unauthorized writes to read-only content.
//! - Configurable case-sensitivity per mount for seamless cross-platform mod compatibility.

pub mod backend;
pub mod error;
pub mod memory;
pub mod mount;
pub mod path;
pub mod physical;
pub mod types;
pub mod vfs;

pub use backend::{BackendDirEntry, VfsBackend};
pub use error::{VfsError, VfsResult};
pub use memory::MemoryMount;
pub use mount::{Mount, MountInfo};
pub use path::VfsPath;
pub use physical::PhysicalMount;
pub use types::{
    CaseSensitivity, MountPolicy, VfsDirEntry, VfsFileType, VfsMetadata, VfsReadOutput,
};
pub use vfs::{Vfs, VfsWriteTarget};

#[cfg(test)]
mod tests;
