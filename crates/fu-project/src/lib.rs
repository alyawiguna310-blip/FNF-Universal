//! Shared, serializable project model for the FNF Universal editor and engine.
//!
//! This crate defines data ownership and validation. It does not render notes,
//! execute scripts, or provide an audio/gameplay runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROJECT_FORMAT: &str = "fnf-universal-project";
pub const PROJECT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocument {
    pub format: String,
    pub format_version: u32,
    pub project: ProjectMetadata,
    #[serde(default)]
    pub songs: Vec<Song>,
    #[serde(default)]
    pub characters: Vec<CharacterDefinition>,
    #[serde(default)]
    pub stages: Vec<StageDefinition>,
    #[serde(default)]
    pub assets: Vec<AssetReference>,
    #[serde(default)]
    pub mod_settings: Vec<ModSettingDefinition>,
}

impl ProjectDocument {
    pub fn new(project: ProjectMetadata) -> Self {
        Self {
            format: PROJECT_FORMAT.into(),
            format_version: PROJECT_FORMAT_VERSION,
            project,
            songs: Vec::new(),
            characters: Vec::new(),
            stages: Vec::new(),
            assets: Vec::new(),
            mod_settings: Vec::new(),
        }
    }

    /// Validate structural and semantic constraints without loading or executing
    /// any mod-provided scripts or assets.
    pub fn validate(&self) -> Result<(), Vec<ValidationIssue>> {
        let mut issues = Vec::new();

        if self.format != PROJECT_FORMAT {
            issues.push(ValidationIssue::new(
                "format",
                format!("expected '{PROJECT_FORMAT}'"),
            ));
        }
        if self.format_version != PROJECT_FORMAT_VERSION {
            issues.push(ValidationIssue::new(
                "formatVersion",
                format!("unsupported version {}; expected {PROJECT_FORMAT_VERSION}", self.format_version),
            ));
        }

        validate_id(&self.project.id, "project.id", &mut issues);
        if self.project.name.trim().is_empty() {
            issues.push(ValidationIssue::new("project.name", "must not be empty"));
        }

        let mut song_ids = BTreeSet::new();
        for (song_index, song) in self.songs.iter().enumerate() {
            let base = format!("songs[{song_index}]");
            validate_unique_id(&song.id, &format!("{base}.id"), &mut song_ids, &mut issues);
            if song.title.trim().is_empty() {
                issues.push(ValidationIssue::new(format!("{base}.title"), "must not be empty"));
            }
            if !song.bpm.is_finite() || song.bpm <= 0.0 {
                issues.push(ValidationIssue::new(format!("{base}.bpm"), "must be a finite number greater than zero"));
            }
            if song.lane_count == 0 {
                issues.push(ValidationIssue::new(format!("{base}.laneCount"), "must be at least one"));
            }

            let mut note_ids = BTreeSet::new();
            for (note_index, note) in song.notes.iter().enumerate() {
                let note_path = format!("{base}.notes[{note_index}]");
                validate_unique_id(&note.id, &format!("{note_path}.id"), &mut note_ids, &mut issues);
                if note.gameplay.lane >= song.lane_count {
                    issues.push(ValidationIssue::new(
                        format!("{note_path}.gameplay.lane"),
                        format!("lane {} is outside this song's {} lanes", note.gameplay.lane, song.lane_count),
                    ));
                }
            }
        }

        let mut character_ids = BTreeSet::new();
        for (index, character) in self.characters.iter().enumerate() {
            let path = format!("characters[{index}].id");
            validate_unique_id(&character.id, &path, &mut character_ids, &mut issues);
            if character.name.trim().is_empty() {
                issues.push(ValidationIssue::new(format!("characters[{index}].name"), "must not be empty"));
            }
        }

        let mut stage_ids = BTreeSet::new();
        for (index, stage) in self.stages.iter().enumerate() {
            let path = format!("stages[{index}].id");
            validate_unique_id(&stage.id, &path, &mut stage_ids, &mut issues);
            if stage.name.trim().is_empty() {
                issues.push(ValidationIssue::new(format!("stages[{index}].name"), "must not be empty"));
            }
        }

        let mut asset_ids = BTreeSet::new();
        for (index, asset) in self.assets.iter().enumerate() {
            let base = format!("assets[{index}]");
            validate_unique_id(&asset.id, &format!("{base}.id"), &mut asset_ids, &mut issues);
            if asset.path.trim().is_empty() {
                issues.push(ValidationIssue::new(format!("{base}.path"), "must not be empty"));
            }
        }

        let mut setting_ids = BTreeSet::new();
        for (index, setting) in self.mod_settings.iter().enumerate() {
            let base = format!("modSettings[{index}]");
            validate_unique_id(&setting.id, &format!("{base}.id"), &mut setting_ids, &mut issues);
            if setting.label.trim().is_empty() {
                issues.push(ValidationIssue::new(format!("{base}.label"), "must not be empty"));
            }
        }

        if issues.is_empty() { Ok(()) } else { Err(issues) }
    }

    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(source: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(source)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub target_engine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    pub bpm: f64,
    pub lane_count: u8,
    #[serde(default)]
    pub notes: Vec<ChartNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChartNote {
    pub id: String,
    /// Gameplay timing and lane identity are independent of sprite placement.
    pub gameplay: NoteGameplay,
    /// Optional renderer-only styling/path binding. It cannot change judgment.
    #[serde(default)]
    pub visual: Option<NoteVisualBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteGameplay {
    pub lane: u8,
    pub strum_time_ms: u64,
    #[serde(default)]
    pub sustain_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteVisualBinding {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub movement_path_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub animations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub objects: Vec<StageObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageObject {
    pub id: String,
    pub visible: bool,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssetReference {
    pub id: String,
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModSettingDefinition {
    pub id: String,
    pub label: String,
    /// JSON value for the creator-defined default. The editor can add a
    /// richer typed setting schema without changing the project envelope.
    pub default_value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

impl ValidationIssue {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { path: path.into(), message: message.into() }
    }
}

fn validate_id(id: &str, path: &str, issues: &mut Vec<ValidationIssue>) {
    if id.trim().is_empty() {
        issues.push(ValidationIssue::new(path, "must not be empty"));
    }
}

fn validate_unique_id(
    id: &str,
    path: &str,
    seen: &mut BTreeSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_id(id, path, issues);
    if !id.trim().is_empty() && !seen.insert(id.to_owned()) {
        issues.push(ValidationIssue::new(path, format!("duplicate id '{id}'")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project() -> ProjectDocument {
        let mut project = ProjectDocument::new(ProjectMetadata {
            id: "demo".into(),
            name: "Demo Mod".into(),
            target_engine: None,
        });
        project.songs.push(Song {
            id: "tutorial".into(),
            title: "Tutorial".into(),
            bpm: 120.0,
            lane_count: 4,
            notes: vec![ChartNote {
                id: "note-1".into(),
                gameplay: NoteGameplay {
                    lane: 2,
                    strum_time_ms: 500,
                    sustain_ms: 0,
                },
                visual: None,
            }],
        });
        project
    }

    #[test]
    fn valid_project_passes_validation() {
        assert!(sample_project().validate().is_ok());
    }

    #[test]
    fn rejects_invalid_bpm_and_lane() {
        let mut project = sample_project();
        project.songs[0].bpm = 0.0;
        project.songs[0].notes[0].gameplay.lane = 4;
        let issues = project.validate().unwrap_err();
        assert!(issues.iter().any(|issue| issue.path == "songs[0].bpm"));
        assert!(issues.iter().any(|issue| issue.path == "songs[0].notes[0].gameplay.lane"));
    }

    #[test]
    fn visual_binding_does_not_mutate_gameplay_state() {
        let mut note = sample_project().songs.remove(0).notes.remove(0);
        let gameplay_before = note.gameplay.clone();
        note.visual = Some(NoteVisualBinding {
            profile_id: Some("neon-blue".into()),
            movement_path_id: Some("orbit-left".into()),
        });
        assert_eq!(note.gameplay, gameplay_before);
    }

    #[test]
    fn project_json_round_trips() {
        let project = sample_project();
        let json = project.to_pretty_json().unwrap();
        let decoded = ProjectDocument::from_json(&json).unwrap();
        assert_eq!(decoded, project);
    }
}
