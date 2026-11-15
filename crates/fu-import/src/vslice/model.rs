//! Typed, lossless representation of a V-Slice (Friday Night Funkin') mod.
//!
//! Every structure keeps two things that a strict deserializer would drop:
//!
//! * `raw` — the JSON value the structure was built from, unchanged.
//! * `unknown_fields` — every object key the importer does not interpret.
//!
//! That makes the model a faithful copy of the source data even when the
//! importer does not understand a field, and it lets a later conversion step
//! decide what to do with it instead of losing it during import.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codename::model::{AssetEntry, AssetInventory, ScriptAsset, UnsupportedFile};

/// Mod metadata file used by V-Slice mods.
pub const VSLICE_METADATA_FILE: &str = "_polymod_meta.json";

/// Alternative mod metadata file used by newer V-Slice builds.
pub const VSLICE_MANIFEST_FILE: &str = "manifest.json";

/// A complete V-Slice mod.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VsliceMod {
    /// Directory name of the mod.
    pub name: String,
    /// Absolute path the mod was read from.
    pub root: String,
    /// Mod-level metadata, when a recognized metadata file was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProjectMetadata>,
    /// Mod-relative path of the file `metadata` was read from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_source: Option<String>,
    /// Songs found under `songs/<id>/`.
    pub songs: Vec<Song>,
    /// Characters found under `characters/` or `data/characters/`.
    pub characters: Vec<Character>,
    /// Stages found under `stages/` or `data/stages/`.
    pub stages: Vec<Stage>,
    /// Story weeks found under `weeks/` or `data/weeks/`.
    pub weeks: Vec<Week>,
    /// Scripts, inventoried by path and size only.
    pub scripts: Vec<ScriptAsset>,
    /// Native binaries and archives, recorded but never loaded.
    pub unsupported_files: Vec<UnsupportedFile>,
    /// JSON documents that could not be decoded into a typed structure.
    pub unparsed: Vec<UnparsedDocument>,
    /// Every file in the mod, with sizes and detected kinds.
    pub assets: AssetInventory,
}

impl VsliceMod {
    /// Song by directory id, matched case-insensitively.
    pub fn song(&self, id: &str) -> Option<&Song> {
        self.songs
            .iter()
            .find(|song| song.id.eq_ignore_ascii_case(id))
    }

    /// Character by id, matched case-insensitively.
    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters
            .iter()
            .find(|character| character.id.eq_ignore_ascii_case(id))
    }

    /// Stage by id, matched case-insensitively.
    pub fn stage(&self, id: &str) -> Option<&Stage> {
        self.stages
            .iter()
            .find(|stage| stage.id.eq_ignore_ascii_case(id))
    }

    /// Week by id, matched case-insensitively.
    pub fn week(&self, id: &str) -> Option<&Week> {
        self.weeks
            .iter()
            .find(|week| week.id.eq_ignore_ascii_case(id))
    }

    /// Every chart in the mod, across songs and variations.
    pub fn charts(&self) -> impl Iterator<Item = &Chart> {
        self.songs.iter().flat_map(|song| song.charts.iter())
    }
}

/// Mod-level metadata from `_polymod_meta.json` or `manifest.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    /// Mod-relative path of the file this was read from.
    pub source: String,
    /// Stable mod id, when the metadata file declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Human readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Homepage or download URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Engine API version the mod was built for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
    /// Version of the mod itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_version: Option<String>,
    /// Declared license.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Icon path, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Author names, from `author` and `contributors`.
    pub authors: Vec<String>,
    /// Structured contributors.
    pub contributors: Vec<Contributor>,
    /// Mandatory mod dependencies, by id.
    pub dependencies: BTreeMap<String, String>,
    /// Optional mod dependencies, by id.
    pub optional_dependencies: BTreeMap<String, String>,
    /// The decoded document, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// One contributor entry in mod metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contributor {
    /// Display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Role, for example `Developer`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Contact address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Contact URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// One song directory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    /// Directory name, which is the song id.
    pub id: String,
    /// Mod-relative song directory.
    pub directory: String,
    /// Metadata for the default variation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SongMetadata>,
    /// Metadata for alternate variations, each carrying its `variation` id.
    pub variation_metadata: Vec<SongMetadata>,
    /// Charts for the default and alternate variations.
    pub charts: Vec<Chart>,
    /// Audio files inside the song directory.
    pub audio: Vec<AssetEntry>,
    /// Other files inside the song directory.
    pub other_files: Vec<String>,
}

impl Song {
    /// Metadata for `variation`, where `None` means the default variation.
    pub fn metadata_for(&self, variation: Option<&str>) -> Option<&SongMetadata> {
        match variation {
            None => self.metadata.as_ref(),
            Some(wanted) => self
                .variation_metadata
                .iter()
                .find(|metadata| metadata.variation.as_deref() == Some(wanted)),
        }
    }

    /// Chart for `variation`, where `None` means the default variation.
    pub fn chart_for(&self, variation: Option<&str>) -> Option<&Chart> {
        self.charts
            .iter()
            .find(|chart| chart.variation.as_deref() == variation)
    }

    /// Difficulty ids declared by the default metadata, in file order.
    pub fn difficulties(&self) -> Vec<String> {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.play_data.as_ref())
            .map(|play_data| play_data.difficulties.clone())
            .unwrap_or_default()
    }
}

/// Song metadata for one variation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongMetadata {
    /// Mod-relative path of the metadata file.
    pub source: String,
    /// Variation id, or `None` for the default variation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<String>,
    /// Metadata format version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Readable song name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_name: Option<String>,
    /// Song artist(s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// Charter credit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter: Option<String>,
    /// Time format. V-Slice only defines `ms`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_format: Option<String>,
    /// Whether the song loops.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looped: Option<bool>,
    /// Beat divisions, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divisions: Option<f64>,
    /// Audio offsets by track name.
    pub offsets: BTreeMap<String, Value>,
    /// BPM changes. V-Slice requires at least one at timestamp `0`.
    pub time_changes: Vec<TimeChange>,
    /// Menu and gameplay data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_data: Option<PlayData>,
    /// Writer of the file, for debugging only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    /// The decoded document, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// One BPM change from song metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeChange {
    /// Timestamp in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<f64>,
    /// Beats per minute from this timestamp on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Time signature numerator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<f64>,
    /// Time signature denominator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<f64>,
    /// Beat tuplets, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bt: Option<Value>,
    /// The decoded object, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// Menu and gameplay data for one variation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayData {
    /// Album id used by the freeplay menu.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Preview start, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_start: Option<f64>,
    /// Preview end, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_end: Option<f64>,
    /// Difficulty ratings by difficulty id.
    pub ratings: BTreeMap<String, f64>,
    /// Variations that have their own metadata and chart files.
    pub song_variations: Vec<String>,
    /// Playable difficulties.
    pub difficulties: Vec<String>,
    /// Characters used by this variation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub characters: Option<SongCharacters>,
    /// Stage id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Note style id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_style: Option<String>,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// Character ids used by one song variation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongCharacters {
    /// Player character id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    /// Girlfriend character id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub girlfriend: Option<String>,
    /// Opponent character id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opponent: Option<String>,
    /// Instrumental id, empty for `Inst.ogg`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumental: Option<String>,
    /// Alternative instrumental ids.
    pub alt_instrumentals: Vec<String>,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// A chart file for one variation, containing every difficulty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    /// Mod-relative path of the chart file.
    pub source: String,
    /// Variation id, or `None` for the default variation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<String>,
    /// Chart format version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Declared chart format, when a file states one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Writer of the file, for debugging only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    /// Scroll speed by difficulty id.
    pub scroll_speed: BTreeMap<String, f64>,
    /// Notes by difficulty id, sorted by id.
    pub notes: BTreeMap<String, Vec<Note>>,
    /// Song events, which apply to every difficulty in the variation.
    pub events: Vec<ChartEvent>,
    /// The decoded document, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

impl Chart {
    /// Difficulty ids that contain at least one note.
    pub fn difficulties(&self) -> Vec<&str> {
        self.notes
            .iter()
            .filter(|(_, notes)| !notes.is_empty())
            .map(|(difficulty, _)| difficulty.as_str())
            .collect()
    }

    /// Every note across all difficulties, with its difficulty id.
    pub fn all_notes(&self) -> impl Iterator<Item = (&str, &Note)> {
        self.notes.iter().flat_map(|(difficulty, notes)| {
            notes.iter().map(move |note| (difficulty.as_str(), note))
        })
    }
}

/// One note in a chart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Position of the note inside its difficulty array.
    pub index: usize,
    /// Timestamp in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<f64>,
    /// Strum direction: `0..=3` player, `4..=7` opponent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<f64>,
    /// Sustain length in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sustain_ms: Option<f64>,
    /// Note kind, preserved verbatim because charts use strings and numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Value>,
    /// Note parameters, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// The decoded object, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

impl Note {
    /// Lane inside its side of the strumline, when the direction is usable.
    pub fn lane(&self) -> Option<u8> {
        let direction = self.direction?;
        if !direction.is_finite() || direction < 0.0 {
            return None;
        }
        let direction = direction as u8;
        Some(direction % 4)
    }

    /// True when the note belongs to the player.
    pub fn is_player(&self) -> Option<bool> {
        let direction = self.direction?;
        if !direction.is_finite() || direction < 0.0 {
            return None;
        }
        Some(direction < 4.0)
    }
}

/// One song event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartEvent {
    /// Timestamp in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<f64>,
    /// Event name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Event value, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// The decoded object, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// A character definition from `characters/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    /// File stem, which is the character id.
    pub id: String,
    /// Mod-relative path of the character file.
    pub source: String,
    /// Readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Render type, for example `sparrow` or `animateatlas`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_type: Option<String>,
    /// Spritesheet path without extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_path: Option<String>,
    /// Sprite scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Whether anti-aliasing is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pixel: Option<bool>,
    /// Whether the sprite is flipped horizontally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flip_x: Option<bool>,
    /// Whether the sprite is flipped vertically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flip_y: Option<bool>,
    /// Global position offset, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offsets: Option<Value>,
    /// Camera offset while focusing the character.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_offsets: Option<Value>,
    /// Idle frequency in beats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dance_every: Option<f64>,
    /// Minimum sing duration in beats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sing_time: Option<f64>,
    /// First animation to play.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starting_animation: Option<String>,
    /// Health icon settings, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_icon: Option<Value>,
    /// Death animation settings, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death: Option<Value>,
    /// Texture atlas settings, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atlas_settings: Option<Value>,
    /// Animation definitions.
    pub animations: Vec<CharacterAnimation>,
    /// The decoded document, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            id: String::new(),
            source: String::new(),
            name: None,
            render_type: None,
            asset_path: None,
            scale: None,
            is_pixel: None,
            flip_x: None,
            flip_y: None,
            offsets: None,
            camera_offsets: None,
            dance_every: None,
            sing_time: None,
            starting_animation: None,
            health_icon: None,
            death: None,
            atlas_settings: None,
            animations: Vec::new(),
            raw: Value::Null,
            unknown_fields: BTreeMap::new(),
        }
    }
}

impl Character {
    /// Animation by name, matched case-insensitively.
    pub fn animation(&self, name: &str) -> Option<&CharacterAnimation> {
        self.animations.iter().find(|animation| {
            animation
                .name
                .as_deref()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        })
    }
}

/// One character animation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterAnimation {
    /// Animation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Frame label prefix in the atlas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Frames per second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<f64>,
    /// Explicit frame indices, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_indices: Option<Value>,
    /// Whether the animation loops.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looped: Option<bool>,
    /// Per-animation offset, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offsets: Option<Value>,
    /// Horizontal flip for this animation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flip_x: Option<bool>,
    /// Vertical flip for this animation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flip_y: Option<bool>,
    /// Animation type, for example `framelabel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anim_type: Option<String>,
    /// The decoded object, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// A stage definition from `stages/<id>.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    /// File stem, which is the stage id.
    pub id: String,
    /// Mod-relative path of the stage file.
    pub source: String,
    /// Readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Stage format version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Sprite directory used by the stage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    /// Camera zoom, from `cameraZoom` or `defaultZoom`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_zoom: Option<f64>,
    /// Whether the stage uses pixel art.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pixel_stage: Option<bool>,
    /// Stage props in file order.
    pub props: Vec<StageProp>,
    /// Older `layers` arrays, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layers: Option<Value>,
    /// Character placement by role, such as `bf`, `dad` and `gf`.
    pub characters: BTreeMap<String, StageCharacter>,
    /// The decoded document, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// One stage prop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageProp {
    /// Optional name scripts use to look the prop up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Spritesheet or color (`#rrggbb`) reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_path: Option<String>,
    /// Position, preserved verbatim because it may be `[x, y]` or an object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Value>,
    /// Stack order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_index: Option<f64>,
    /// Scale, either a number or `[w, h]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<Value>,
    /// Alpha.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    /// Parallax scroll factors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll: Option<Value>,
    /// Prop animations, preserved verbatim.
    pub animations: Vec<Value>,
    /// First animation to play, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starting_animation: Option<String>,
    /// Animation type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anim_type: Option<String>,
    /// The decoded object, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// Character placement inside a stage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageCharacter {
    /// Stack order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_index: Option<f64>,
    /// Position, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Value>,
    /// Scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Camera offsets, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_offsets: Option<Value>,
    /// Parallax scroll factors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll: Option<Value>,
    /// Alpha.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    /// The decoded object, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// A story week from `weeks/<id>.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Week {
    /// File stem, which is the week id.
    pub id: String,
    /// Mod-relative path of the week file.
    pub source: String,
    /// Readable story title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_name: Option<String>,
    /// Song ids, in order, when the file lists them as strings.
    pub songs: Vec<String>,
    /// The decoded document, unchanged.
    pub raw: Value,
    /// Keys the importer does not interpret.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// A JSON document that could not be decoded into a typed structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnparsedDocument {
    /// Mod-relative path.
    pub path: String,
    /// Why decoding failed.
    pub reason: String,
    /// The raw text, retained so nothing is silently dropped.
    pub contents: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notes_expose_lane_and_side() {
        let note = Note {
            direction: Some(5.0),
            ..Note::default()
        };
        assert_eq!(note.lane(), Some(1));
        assert_eq!(note.is_player(), Some(false));

        let player = Note {
            direction: Some(2.0),
            ..Note::default()
        };
        assert_eq!(player.lane(), Some(2));
        assert_eq!(player.is_player(), Some(true));

        let broken = Note {
            direction: Some(-1.0),
            ..Note::default()
        };
        assert_eq!(broken.lane(), None);
        assert_eq!(broken.is_player(), None);
        assert_eq!(Note::default().lane(), None);
    }

    #[test]
    fn chart_helpers_report_difficulties_and_all_notes() {
        let mut chart = Chart::default();
        chart.notes.insert(
            "hard".to_string(),
            vec![Note {
                index: 0,
                time_ms: Some(1000.0),
                ..Note::default()
            }],
        );
        chart.notes.insert("easy".to_string(), Vec::new());
        assert_eq!(chart.difficulties(), vec!["hard"]);
        assert_eq!(chart.all_notes().count(), 1);
    }

    #[test]
    fn song_helpers_find_variations() {
        let song = Song {
            id: "dadbattle".to_string(),
            metadata: Some(SongMetadata {
                variation: None,
                play_data: Some(PlayData {
                    difficulties: vec!["easy".to_string()],
                    ..PlayData::default()
                }),
                ..SongMetadata::default()
            }),
            variation_metadata: vec![SongMetadata {
                variation: Some("erect".to_string()),
                ..SongMetadata::default()
            }],
            charts: vec![
                Chart {
                    variation: None,
                    ..Chart::default()
                },
                Chart {
                    variation: Some("erect".to_string()),
                    ..Chart::default()
                },
            ],
            ..Song::default()
        };
        assert_eq!(song.difficulties(), vec!["easy"]);
        assert!(song.metadata_for(Some("erect")).is_some());
        assert!(song.chart_for(Some("erect")).is_some());
        assert!(song.chart_for(Some("nightmare")).is_none());
    }

    #[test]
    fn mod_helpers_lookup_case_insensitively() {
        let module = VsliceMod {
            songs: vec![Song {
                id: "DadBattle".to_string(),
                ..Song::default()
            }],
            characters: vec![Character {
                id: "BF".to_string(),
                ..Character::default()
            }],
            stages: vec![Stage {
                id: "MainStage".to_string(),
                ..Stage::default()
            }],
            weeks: vec![Week {
                id: "Week1".to_string(),
                ..Week::default()
            }],
            ..VsliceMod::default()
        };
        assert!(module.song("dadbattle").is_some());
        assert!(module.character("bf").is_some());
        assert!(module.stage("mainstage").is_some());
        assert!(module.week("week1").is_some());
    }

    #[test]
    fn model_round_trips_through_json() {
        let module = VsliceMod {
            name: "demo".to_string(),
            metadata: Some(ProjectMetadata {
                source: "_polymod_meta.json".to_string(),
                title: Some("Demo".to_string()),
                unknown_fields: BTreeMap::from([("custom".to_string(), json!(1))]),
                ..ProjectMetadata::default()
            }),
            songs: vec![Song {
                id: "song".to_string(),
                charts: vec![Chart {
                    source: "songs/song/song-chart.json".to_string(),
                    raw: json!({"version": "2.0.0"}),
                    ..Chart::default()
                }],
                ..Song::default()
            }],
            ..VsliceMod::default()
        };
        let json = serde_json::to_string(&module).expect("serialize");
        let parsed: VsliceMod = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, module);
        assert_eq!(parsed.charts().count(), 1);
    }
}
