use crate::path::VfsPath;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum VfsError {
    #[error("invalid virtual path: {0}")]
    InvalidPath(String),

    #[error("path traversal attempt detected: {0}")]
    PathTraversal(String),

    #[error("virtual path not found: '{0}'")]
    NotFound(VfsPath),

    #[error("cannot write to read-only mount '{mount_id}' at '{path}'")]
    ReadOnlyMount { mount_id: String, path: VfsPath },

    #[error("no writable mount available for virtual path '{0}'")]
    NoWritableMount(VfsPath),

    #[error("access denied: {0}")]
    AccessDenied(String),

    #[error("duplicate mount identifier: '{0}'")]
    DuplicateMount(String),

    #[error("mount not found: '{0}'")]
    MountNotFound(String),

    #[error("I/O error at '{path}': {message}")]
    Io { path: String, message: String },

    #[error("backend error: {0}")]
    Backend(String),
}

pub type VfsResult<T> = Result<T, VfsError>;
