//! Backend-agnostic 3D rendering foundation for FNF Universal.
//!
//! This crate owns CPU-side data (meshes, materials, textures, lights, cameras,
//! scene graph), visibility culling, render ordering and quality configuration.
//! It does NOT talk to a GPU: a `RenderBackend` implementation (for example a
//! future wgpu backend) consumes the `FramePacket` produced by `Renderer3D`.
//!
//! It does not depend on, replace, or modify any 2D renderer or engine runtime.

pub mod backend;
pub mod camera;
pub mod light;
pub mod material;
pub mod mesh;
pub mod quality;
pub mod queue;
pub mod renderer;
pub mod resources;
pub mod scene;
pub mod shader;
pub mod transform;

pub use glam;
use thiserror::Error;

/// Capability id an engine manifest can declare once a runtime wires this crate in.
pub const CAPABILITY_ID: &str = "render3d";

#[derive(Debug, Error, PartialEq)]
pub enum RenderError {
    #[error("invalid {what}: {reason}")]
    Invalid { what: &'static str, reason: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("backend error: {0}")]
    Backend(String),
}

impl RenderError {
    pub(crate) fn invalid(what: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            what,
            reason: reason.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, RenderError>;
