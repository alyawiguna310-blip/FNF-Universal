//! Serializable 3D stage authoring model.
//!
//! This is data/validation foundation only. It does not load models or render a
//! scene; those responsibilities belong to the platform renderer.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StageDimension {
    #[default]
    TwoD,
    ThreeD,
    Hybrid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelFormat {
    #[default]
    Gltf,
    Glb,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    Low,
    #[default]
    Balanced,
    High,
    Ultra,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stage3DDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub dimension: StageDimension,
    #[serde(default)]
    pub models: Vec<ModelAsset>,
    #[serde(default)]
    pub nodes: Vec<SceneNode>,
    #[serde(default)]
    pub camera: Camera3D,
    #[serde(default)]
    pub lights: Vec<Light3D>,
    #[serde(default)]
    pub quality: QualitySettings,
}

impl Stage3DDefinition {
    /// Validate metadata and references without opening model files.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut issues = Vec::new();
        if self.id.trim().is_empty() {
            issues.push("id must not be empty".to_string());
        }
        if self.name.trim().is_empty() {
            issues.push("name must not be empty".to_string());
        }

        let mut model_ids = BTreeSet::new();
        for model in &self.models {
            if model.id.trim().is_empty() || !model_ids.insert(model.id.as_str()) {
                issues.push(format!("model id is empty or duplicated: '{}'", model.id));
            }
            if model.path.trim().is_empty() {
                issues.push(format!("model '{}' has an empty path", model.id));
            }
            let extension = model.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let expected = match model.format {
                ModelFormat::Gltf => "gltf",
                ModelFormat::Glb => "glb",
            };
            if extension != expected {
                issues.push(format!(
                    "model '{}' declares {:?} but path extension is '.{}'",
                    model.id, model.format, extension
                ));
            }
        }

        let mut node_ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.trim().is_empty() || !node_ids.insert(node.id.as_str()) {
                issues.push(format!("scene node id is empty or duplicated: '{}'", node.id));
            }
            if let Some(asset_id) = &node.model_asset_id {
                if !model_ids.contains(asset_id.as_str()) {
                    issues.push(format!(
                        "scene node '{}' references missing model '{}'",
                        node.id, asset_id
                    ));
                }
            }
            if !node.transform.is_finite() {
                issues.push(format!("scene node '{}' has a non-finite transform", node.id));
            }
        }

        if !self.camera.is_valid() {
            issues.push("camera values must be finite and clipping planes must be valid".into());
        }
        for (index, light) in self.lights.iter().enumerate() {
            if !light.is_valid() {
                issues.push(format!("light at index {index} has invalid values"));
            }
        }

        if self.quality.max_lights == 0 {
            issues.push("quality.maxLights must be at least one".into());
        }
        if self.quality.max_texture_dimension < 256 {
            issues.push("quality.maxTextureDimension must be at least 256".into());
        }

        if issues.is_empty() { Ok(()) } else { Err(issues) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelAsset {
    pub id: String,
    pub path: String,
    pub format: ModelFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneNode {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub model_asset_id: Option<String>,
    #[serde(default)]
    pub transform: Transform3D,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transform3D {
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_degrees: [f32; 3],
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
}

impl Default for Transform3D {
    fn default() -> Self {
        Self { position: [0.0; 3], rotation_degrees: [0.0; 3], scale: default_scale() }
    }
}

fn default_scale() -> [f32; 3] { [1.0; 3] }

impl Transform3D {
    fn is_finite(&self) -> bool {
        self.position.iter().chain(self.rotation_degrees.iter()).chain(self.scale.iter())
            .all(|value| value.is_finite())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Camera3D {
    #[serde(default = "default_fov")]
    pub field_of_view_degrees: f32,
    #[serde(default = "default_near")]
    pub near_clip: f32,
    #[serde(default = "default_far")]
    pub far_clip: f32,
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default)]
    pub target: [f32; 3],
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            field_of_view_degrees: default_fov(),
            near_clip: default_near(),
            far_clip: default_far(),
            position: [0.0, 2.0, 8.0],
            target: [0.0, 1.0, 0.0],
        }
    }
}

fn default_fov() -> f32 { 60.0 }
fn default_near() -> f32 { 0.1 }
fn default_far() -> f32 { 1000.0 }

impl Camera3D {
    fn is_valid(&self) -> bool {
        self.field_of_view_degrees.is_finite()
            && (1.0..179.0).contains(&self.field_of_view_degrees)
            && self.near_clip.is_finite()
            && self.far_clip.is_finite()
            && self.near_clip > 0.0
            && self.far_clip > self.near_clip
            && self.position.iter().chain(self.target.iter()).all(|v| v.is_finite())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    #[default]
    Directional,
    Point,
    Spot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Light3D {
    #[serde(default)]
    pub kind: LightKind,
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    #[serde(default = "default_color")]
    pub color_rgb: [f32; 3],
    #[serde(default)]
    pub position: [f32; 3],
}

impl Default for Light3D {
    fn default() -> Self {
        Self { kind: LightKind::Directional, intensity: default_intensity(), color_rgb: default_color(), position: [0.0; 3] }
    }
}

fn default_intensity() -> f32 { 1.0 }
fn default_color() -> [f32; 3] { [1.0; 3] }

impl Light3D {
    fn is_valid(&self) -> bool {
        self.intensity.is_finite()
            && self.intensity >= 0.0
            && self.color_rgb.iter().chain(self.position.iter())
                .all(|value| value.is_finite() && *value >= 0.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QualitySettings {
    #[serde(default)]
    pub preferred_tier: QualityTier,
    #[serde(default = "default_texture_dimension")]
    pub max_texture_dimension: u32,
    #[serde(default = "default_max_lights")]
    pub max_lights: u8,
    #[serde(default = "default_true")]
    pub shadows: bool,
    #[serde(default)]
    pub post_processing: bool,
    #[serde(default = "default_target_fps")]
    pub target_fps: u16,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            preferred_tier: QualityTier::Balanced,
            max_texture_dimension: default_texture_dimension(),
            max_lights: default_max_lights(),
            shadows: true,
            post_processing: false,
            target_fps: default_target_fps(),
        }
    }
}

fn default_texture_dimension() -> u32 { 2048 }
fn default_max_lights() -> u8 { 4 }
fn default_target_fps() -> u16 { 60 }

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Stage3DDefinition {
        Stage3DDefinition {
            id: "neon-rooftop".into(),
            name: "Neon Rooftop".into(),
            dimension: StageDimension::Hybrid,
            models: vec![ModelAsset { id: "city".into(), path: "models/city.glb".into(), format: ModelFormat::Glb }],
            nodes: vec![SceneNode { id: "city-node".into(), name: "City".into(), model_asset_id: Some("city".into()), transform: Transform3D::default(), visible: true }],
            camera: Camera3D::default(),
            lights: vec![Light3D::default()],
            quality: QualitySettings::default(),
        }
    }

    #[test]
    fn valid_3d_stage_passes_validation() { assert!(sample().validate().is_ok()); }

    #[test]
    fn missing_model_reference_is_rejected() {
        let mut stage = sample();
        stage.nodes[0].model_asset_id = Some("missing".into());
        assert!(stage.validate().unwrap_err().iter().any(|issue| issue.contains("missing model")));
    }

    #[test]
    fn invalid_camera_is_rejected() {
        let mut stage = sample();
        stage.camera.near_clip = 10.0;
        stage.camera.far_clip = 2.0;
        assert!(stage.validate().is_err());
    }

    #[test]
    fn stage_round_trips_json() {
        let stage = sample();
        let json = serde_json::to_string(&stage).unwrap();
        let decoded: Stage3DDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, stage);
    }
}
