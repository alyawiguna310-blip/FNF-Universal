use std::sync::Arc;

use fu_vfs::{VfsError, VfsPath};
use thiserror::Error;

/// Errors that can occur during asset discovery and loading.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum AssetError {
    /// A virtual file system operation failed.
    #[error("VFS error: {0}")]
    Vfs(#[from] VfsError),

    /// Deserialization of a JSON asset failed.
    #[error("failed to deserialize JSON from '{path}': {source}")]
    Json {
        path: VfsPath,
        #[source]
        source: serde_json::Error,
    },

    /// Directory recursion exceeded the safety depth limit.
    #[error("recursion depth exceeded while indexing '{0}'")]
    RecursionDepthExceeded(VfsPath),

    /// Failed to decode an asset into its target type.
    #[error("failed to decode asset at '{path}': {reason}")]
    Decode { path: VfsPath, reason: String },

    /// Shared error from a concurrent loader thread.
    #[error("shared asset error: {0}")]
    Shared(Arc<AssetError>),

    /// Asset loading was aborted abnormally or panicking loader dropped.
    #[error("asset load aborted unexpectedly at '{0}")]
    LoadAborted(VfsPath),

    /// Asset was not found in cache.
    #[error("asset '{0}' not found in cache")]
    NotFound(VfsPath),

    /// The VFS topology changed while an eviction snapshot was being validated.
    /// No cache state was modified.
    #[error("VFS topology changed while preparing eviction of '{0}")]
    TopologyChanged(VfsPath),

    /// A generated or decoded texture could not be produced.
    #[error("texture error: {reason}")]
    Texture { reason: String },

    /// A resource manifest entry is malformed.
    #[error("invalid resource manifest: {reason}")]
    InvalidManifest { reason: String },

    /// A `pin` operation lost a race with invalidation and did not pin.
    #[error("pin invalidated for '{path}': {reason}")]
    PinInvalidated {
        path: VfsPath,
        reason: PinInvalidation,
    },

    /// An I/O operation failed (file copy, create, etc).
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: VfsPath,
        #[source]
        source: std::io::Error,
    },
}

/// Why a successful asset load could not be pinned.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinInvalidation {
    /// The request route was removed by eviction/clear or was never installed.
    RouteEvicted,
    /// The route generation no longer matches the VFS topology generation.
    TopologyChanged,
    /// The slot no longer contains the handle returned by the load.
    HandleSuperseded,
}

impl std::fmt::Display for PinInvalidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::RouteEvicted => "route was evicted before pinning",
            Self::TopologyChanged => "VFS topology changed before pinning",
            Self::HandleSuperseded => "cached handle was superseded before pinning",
        };
        f.write_str(text)
    }
}

impl From<crate::texture::TextureDecodeError> for AssetError {
    fn from(error: crate::texture::TextureDecodeError) -> Self {
        AssetError::Texture {
            reason: error.to_string(),
        }
    }
}

/// Convenience alias for results returned by `fu-assets`.
pub type AssetResult<T> = Result<T, AssetError>;
