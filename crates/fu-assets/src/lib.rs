//! Asset management and loading pipeline for FNF Universal.
//!
//! Provides VFS-driven asset indexing and basic content loading helpers.

mod animate;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fu_vfs::{Vfs, VfsFileType, VfsPath};
use serde::de::DeserializeOwned;
use walkdir::WalkDir;

pub mod asset;
pub mod atlas;
pub mod cache;
pub mod default_pack;
pub mod error;
pub mod manifest;
pub mod texture;

pub use asset::Asset;
pub use atlas::{Animation, AtlasFrame, AtlasRect, SparrowAtlas, UvRect};
pub use animate::{Affine, AnimateAtlas, AnimateLabel, AnimateQuad, AtlasError};
pub use cache::{AssetCache, CanonicalKey, RequestKey};
pub use default_pack::{
    install_default_pack, DefaultPack, DEFAULT_MANIFEST_JSON, DEFAULT_MANIFEST_PATH,
};
pub use error::{AssetError, AssetResult, PinInvalidation};
pub use manifest::{ResolvedResource, ResourceManifest, ResourceStack};
pub use texture::TextureAsset;

#[cfg(test)]
mod tests;

/// Maximum directory recursion depth during VFS asset indexing to prevent
/// stack exhaustion or cyclic traversal.
pub const MAX_INDEX_DEPTH: usize = 64;

/// Recursively discovers all file virtual paths within a VFS directory.
///
/// # Behavior
/// - Traverses the virtual directory hierarchy across all active mounts.
/// - Returns normalized [`VfsPath`] instances sorted deterministically.
/// - Propagates missing directory errors explicitly; never silently returns empty for a nonexistent root.
/// - Guards against cyclic traversal and excessive recursion depth.
pub fn index_vfs(vfs: &Vfs, virtual_root: impl AsRef<str>) -> AssetResult<Vec<VfsPath>> {
    let root = VfsPath::new(virtual_root.as_ref())?;
    let mut discovered = Vec::new();
    let mut visited = BTreeSet::new();

    fn traverse(
        vfs: &Vfs,
        dir: &VfsPath,
        depth: usize,
        visited: &mut BTreeSet<VfsPath>,
        discovered: &mut Vec<VfsPath>,
    ) -> AssetResult<()> {
        if depth > MAX_INDEX_DEPTH {
            return Err(AssetError::RecursionDepthExceeded(dir.clone()));
        }

        if !visited.insert(dir.clone()) {
            return Ok(());
        }

        let entries = vfs.read_dir(dir)?;
        for entry in entries {
            match entry.file_type {
                VfsFileType::File => {
                    discovered.push(entry.path);
                }
                VfsFileType::Directory => {
                    traverse(vfs, &entry.path, depth + 1, visited, discovered)?;
                }
            }
        }
        Ok(())
    }

    traverse(vfs, &root, 0, &mut visited, &mut discovered)?;
    discovered.sort();
    Ok(discovered)
}

/// Alias for [`index_vfs`]. Recursively indexes all files in a virtual directory.
#[inline]
pub fn index_vfs_directory(vfs: &Vfs, virtual_root: impl AsRef<str>) -> AssetResult<Vec<VfsPath>> {
    index_vfs(vfs, virtual_root)
}

/// Reads raw binary bytes for an asset from the VFS.
///
/// Dispatches across mounts according to VFS overlay priority and fallback rules.
pub fn load_bytes(vfs: &Vfs, path: impl AsRef<str>) -> AssetResult<Vec<u8>> {
    let vpath = VfsPath::new(path.as_ref())?;
    let data = vfs.read(&vpath)?;
    Ok(data)
}

/// Reads a UTF-8 text file from the VFS.
///
/// Dispatches across mounts according to VFS overlay priority and fallback rules.
pub fn load_text(vfs: &Vfs, path: impl AsRef<str>) -> AssetResult<String> {
    let vpath = VfsPath::new(path.as_ref())?;
    let text = vfs.read_to_string(&vpath)?;
    Ok(text)
}

/// Reads and deserializes a JSON document from the VFS.
///
/// Preserves the target virtual path in error diagnostics if deserialization fails.
pub fn load_json<T: DeserializeOwned>(vfs: &Vfs, path: impl AsRef<str>) -> AssetResult<T> {
    let vpath = VfsPath::new(path.as_ref())?;
    let bytes = vfs.read(&vpath)?;
    serde_json::from_slice(&bytes).map_err(|source| AssetError::Json {
        path: vpath,
        source,
    })
}

/// Indexes files on the physical host filesystem.
///
/// Retained for backwards compatibility with non-VFS physical path utilities.
/// Prefer [`index_vfs`] or [`index_vfs_directory`] for game asset resolution.
pub fn index_files(root: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}
pub mod migration;
pub use migration::run as migrate_codename;
