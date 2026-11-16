use std::sync::Arc;

use crate::backend::VfsBackend;
use crate::path::VfsPath;
use crate::types::{CaseSensitivity, MountPolicy};
use serde::{Deserialize, Serialize};

/// Metadata description of a mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountInfo {
    pub id: String,
    pub mount_point: VfsPath,
    pub priority: i32,
    pub policy: MountPolicy,
    pub case_sensitivity: CaseSensitivity,
    pub enabled: bool,
    pub backend_type: String,
}

/// A mounted filesystem backend inside the VFS.
#[derive(Clone)]
pub struct Mount {
    pub id: String,
    pub mount_point: VfsPath,
    pub priority: i32,
    pub policy: MountPolicy,
    pub case_sensitivity: CaseSensitivity,
    pub enabled: bool,
    pub backend: Arc<dyn VfsBackend>,
}

impl Mount {
    pub fn new(
        id: impl Into<String>,
        mount_point: VfsPath,
        priority: i32,
        policy: MountPolicy,
        case_sensitivity: CaseSensitivity,
        backend: Arc<dyn VfsBackend>,
    ) -> Self {
        Self {
            id: id.into(),
            mount_point,
            priority,
            policy,
            case_sensitivity,
            enabled: true,
            backend,
        }
    }

    pub fn info(&self) -> MountInfo {
        MountInfo {
            id: self.id.clone(),
            mount_point: self.mount_point.clone(),
            priority: self.priority,
            policy: self.policy,
            case_sensitivity: self.case_sensitivity,
            enabled: self.enabled,
            backend_type: self.backend.backend_type().to_string(),
        }
    }
}
