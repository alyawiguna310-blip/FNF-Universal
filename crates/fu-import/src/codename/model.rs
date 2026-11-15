//! Serializable representation of a Codename Engine mod.
//!
//! The model is intentionally lossless: every element, attribute, INI section
//! and JSON property that the importer does not interpret is still stored, so a
//! round trip through [`CodenameMod`] keeps unknown content available to later
//! tools. Nothing here executes, decodes or renders mod content.
//!
//! This model is deliberately separate from `ProjectDocument`: forcing Codename
//! data into the editor project schema would drop information such as per-strum
//! line layout, unknown chart properties or arbitrary INI sections.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ini::{IniDocument, IniEntry};

/// An XML attribute, kept in source order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    /// Attribute name as written.
    pub name: String,
    /// Attribute value as written (after entity decoding).
    pub value: String,
}

/// An element the importer recognizes structurally but does not interpret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawElement {
    /// Element name.
    pub name: String,
    /// All attributes, in source order.
    pub attributes: Vec<Attribute>,
    /// Trimmed text content.
    pub text: String,
}

/// Looks up `name` in an ordered attribute list.
pub fn attribute<'a>(attributes: &'a [Attribute], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .map(|attribute| attribute.value.as_str())
}

/// Looks up `name` case-insensitively in an ordered attribute list.
pub fn attribute_ci<'a>(attributes: &'a [Attribute], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
        .map(|attribute| attribute.value.as_str())
}

/// Parses an attribute value as `f64`.
pub fn attribute_f64(attributes: &[Attribute], name: &str) -> Option<f64> {
    attribute_ci(attributes, name)?.trim().parse().ok()
}

/// A fully imported Codename Engine mod.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodenameMod {
    /// Mod directory name.
    pub name: String,
    /// Mod root as resolved on disk, for display only.
    pub root: String,
    /// `data/config` metadata.
    pub config: ModpackConfig,
    /// Characters from `data/characters/*.xml`.
    pub characters: Vec<Character>,
    /// Stages from `data/stages/*.xml`.
    pub stages: Vec<Stage>,
    /// Songs from `songs/*`.
    pub songs: Vec<Song>,
    /// Event definitions from `data/events/*.json`.
    pub events: Vec<EventDefinition>,
    /// Every script that was inventoried but never executed.
    pub scripts: Vec<ScriptAsset>,
    /// Native libraries, executables and archives that were never loaded.
    pub unsupported_files: Vec<UnsupportedFile>,
    /// Complete file inventory with atlas recognition.
    pub assets: AssetInventory,
}

impl CodenameMod {
    /// Character by id (file stem), matched case-insensitively.
    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters
            .iter()
            .find(|character| character.id.eq_ignore_ascii_case(id))
    }

    /// Stage by id (file stem), matched case-insensitively.
    pub fn stage(&self, id: &str) -> Option<&Stage> {
        self.stages
            .iter()
            .find(|stage| stage.id.eq_ignore_ascii_case(id))
    }

    /// Song by directory name, matched case-insensitively.
    pub fn song(&self, name: &str) -> Option<&Song> {
        self.songs
            .iter()
            .find(|song| song.name.eq_ignore_ascii_case(name))
    }

    /// Event definition by name, matched case-insensitively.
    pub fn event_definition(&self, name: &str) -> Option<&EventDefinition> {
        self.events
            .iter()
            .find(|event| event.name.eq_ignore_ascii_case(name))
    }

    /// True when the mod defines `name` as an event, either through a JSON
    /// definition or an accompanying `.hx` script.
    pub fn defines_event(&self, name: &str) -> bool {
        self.event_definition(name).is_some()
            || self
                .scripts
                .iter()
                .any(|script| script.in_event_directory && script.stem().eq_ignore_ascii_case(name))
    }
}

/// Configuration assembled from `modpack.ini` and `flags.ini`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackConfig {
    /// Every configuration file that was read, with its full parsed contents.
    pub sources: Vec<ConfigSource>,
    /// `[Common] API_VERSION`, kept as text because mods are inconsistent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
    /// Merged `[Flags]` entries. Later files win on duplicate keys.
    pub flags: BTreeMap<String, String>,
    /// Merged `[Discord]` entries.
    pub discord: BTreeMap<String, String>,
    /// `[StateRedirects]` and `[StateRedirects.force]` entries in load order.
    pub state_redirects: Vec<StateRedirect>,
    /// Section names that the importer did not interpret, for diagnostics.
    pub unknown_sections: Vec<String>,
}

impl ModpackConfig {
    /// True when no configuration content was assembled at all.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
            && self.state_redirects.is_empty()
            && self.flags.is_empty()
            && self.discord.is_empty()
            && self.api_version.is_none()
    }

    /// All entries of sections named `name` across every source, in load order.
    pub fn entries<'a>(&'a self, name: &'a str) -> Vec<&'a IniEntry> {
        self.sources
            .iter()
            .flat_map(|source| source.document.merged_entries(name))
            .collect()
    }
}

/// One parsed configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSource {
    /// Mod-relative path.
    pub path: String,
    /// Entire parsed document, preserving unknown sections and keys.
    pub document: IniDocument,
}

/// A state redirect declared by the mod.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateRedirect {
    /// Source state name, for example `TitleState`.
    pub state: String,
    /// Target state or script, for example `newTitle`.
    pub target: String,
    /// True when declared under `[StateRedirects.force]`.
    pub force: bool,
    /// Configuration file that declared it.
    pub source: String,
}

/// A character definition from `data/characters`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    /// File stem, which is how charts and other content reference the character.
    pub id: String,
    /// Mod-relative source path.
    pub source: String,
    /// All `<character>` attributes, including unknown ones.
    pub attributes: Vec<Attribute>,
    /// `sprite` reference (an image, Sparrow atlas or Animate atlas folder).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
    /// Health-bar icon reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// `<anim>` children in source order.
    pub animations: Vec<CharacterAnimation>,
    /// Child elements that are neither `<anim>` nor recognized.
    pub extra_elements: Vec<RawElement>,
}

impl Character {
    /// Animation by name, matched case-insensitively.
    pub fn animation(&self, name: &str) -> Option<&CharacterAnimation> {
        self.animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case(name))
    }
}

/// A character `<anim>` entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterAnimation {
    /// Chart-facing animation name, for example `singLEFT`.
    pub name: String,
    /// Atlas animation identifier (`anim` attribute).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
    /// `label` attribute; Codename treats some values as booleans and some as
    /// frame labels, so it is preserved as text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Animation x offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Animation y offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// All attributes, including unknown ones.
    pub attributes: Vec<Attribute>,
}

/// A stage definition from `data/stages`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    /// File stem, which is how charts reference the stage.
    pub id: String,
    /// Mod-relative source path.
    pub source: String,
    /// All `<stage>` attributes, including unknown ones.
    pub attributes: Vec<Attribute>,
    /// `name` attribute, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `folder` attribute; image directory relative to `images/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Sprites and anchors in source order, which is also draw order.
    pub elements: Vec<StageElement>,
    /// Child elements that are not recognized.
    pub extra_elements: Vec<RawElement>,
}

impl Stage {
    /// Stage sprites in source order.
    pub fn sprites(&self) -> impl Iterator<Item = &StageSprite> {
        self.elements.iter().filter_map(|element| match element {
            StageElement::Sprite(sprite) => Some(sprite),
            StageElement::Anchor(_) => None,
        })
    }

    /// Character anchors in source order.
    pub fn anchors(&self) -> impl Iterator<Item = &StageAnchor> {
        self.elements.iter().filter_map(|element| match element {
            StageElement::Anchor(anchor) => Some(anchor),
            StageElement::Sprite(_) => None,
        })
    }
}

/// A child of `<stage>`, in draw order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "element")]
pub enum StageElement {
    /// A backdrop sprite.
    Sprite(StageSprite),
    /// A character anchor.
    Anchor(StageAnchor),
}

/// A stage `<sprite>` definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageSprite {
    /// Logical name used by scripts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Image name inside the stage folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
    /// Horizontal offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Vertical offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Uniform scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Horizontal parallax. `scroll` is the legacy single-axis form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_x: Option<f64>,
    /// Vertical parallax. `scroll` is the legacy single-axis form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_y: Option<f64>,
    /// Zoom factor applied before stage zoom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom_factor: Option<f64>,
    /// Opacity multiplier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    /// All attributes, including unknown ones.
    pub attributes: Vec<Attribute>,
}

/// A stage character anchor (`dad`, `boyfriend`, `gf` or `girlfriend`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageAnchor {
    /// Which gameplay slot this anchor positions.
    pub role: AnchorRole,
    /// Element name as written, so `gf` and `girlfriend` both survive.
    pub tag: String,
    /// Horizontal offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Vertical offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Horizontal parallax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_x: Option<f64>,
    /// Vertical parallax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_y: Option<f64>,
    /// Anchor scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// All attributes, including unknown ones.
    pub attributes: Vec<Attribute>,
}

/// Gameplay slot addressed by a stage anchor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorRole {
    /// Opponent slot (`dad`).
    #[default]
    Opponent,
    /// Player slot (`boyfriend`).
    Player,
    /// Girlfriend slot (`gf` or `girlfriend`).
    Girlfriend,
}

/// A song directory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    /// Directory name, which is how the engine finds the song.
    pub name: String,
    /// Mod-relative directory path.
    pub directory: String,
    /// `meta.json`, when present and parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SongMetadata>,
    /// `events.json`, when present and parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<SongEvents>,
    /// Charts in `charts/*.json`.
    pub charts: Vec<Chart>,
    /// Audio files under `song/`.
    pub audio: Vec<AssetEntry>,
    /// Scripts under `scripts/`.
    pub scripts: Vec<ScriptAsset>,
    /// Any other file in the song directory.
    pub other_files: Vec<AssetEntry>,
}

impl Song {
    /// Chart by difficulty file stem, matched case-insensitively.
    pub fn chart(&self, difficulty: &str) -> Option<&Chart> {
        self.charts
            .iter()
            .find(|chart| chart.difficulty.eq_ignore_ascii_case(difficulty))
    }
}

/// Parsed `meta.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongMetadata {
    /// `displayName`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Health-bar icon name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Freeplay colour string, kept verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Beats per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Whether a voices track is expected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_voices: Option<bool>,
    /// Difficulty names declared by the song.
    pub difficulties: Vec<String>,
    /// Chart steps per beat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps_per_beat: Option<f64>,
    /// Time-signature numerator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beats_per_measure: Option<f64>,
    /// Whether opponent mode is allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opponent_mode_allowed: Option<bool>,
    /// Whether co-op is allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coop_allowed: Option<bool>,
    /// Game-over block, preserved as-is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gameover: Option<Value>,
    /// `customValues`, preserved as-is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_values: Option<Value>,
    /// Properties that are not interpreted here.
    pub unknown_fields: BTreeMap<String, Value>,
    /// The complete document, so nothing is lost.
    pub raw: Value,
}

/// Parsed song-level `events.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongEvents {
    /// Events in file order.
    pub events: Vec<ChartEvent>,
    /// Top-level properties that are not `events`.
    pub unknown_fields: BTreeMap<String, Value>,
    /// The complete document.
    pub raw: Value,
}

/// Chart layout that was detected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartFormat {
    /// Codename `strumLines` layout.
    #[default]
    Codename,
    /// Legacy `{"song": {"notes": [...]}}` layout.
    LegacyNotes,
    /// Layout that could not be identified.
    Unknown,
}

/// A parsed chart file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    /// Mod-relative source path.
    pub source: String,
    /// Difficulty, taken from the chart file stem.
    pub difficulty: String,
    /// Detected layout.
    pub format: ChartFormat,
    /// `chartVersion`, which mods write as either a string or a number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_version: Option<String>,
    /// `codenameChart` flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codename_chart: Option<bool>,
    /// `scrollSpeed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_speed: Option<f64>,
    /// Default stage for this chart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// `noteTypes`, preserving string and object entries.
    pub note_types: Vec<NoteTypeDefinition>,
    /// `strumLines` in file order.
    pub strum_lines: Vec<StrumLine>,
    /// Events embedded in the chart.
    pub events: Vec<ChartEvent>,
    /// `players`, when present, preserved as-is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub players: Option<Value>,
    /// Top-level properties that are not interpreted here.
    pub unknown_fields: BTreeMap<String, Value>,
    /// The complete document.
    pub raw: Value,
}

impl Chart {
    /// Every note across all strum lines.
    pub fn notes(&self) -> impl Iterator<Item = &Note> {
        self.strum_lines.iter().flat_map(|line| line.notes.iter())
    }

    /// Total note count.
    pub fn note_count(&self) -> usize {
        self.strum_lines.iter().map(|line| line.notes.len()).sum()
    }
}

/// A `noteTypes` entry. Codename accepts strings and structured objects.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteTypeDefinition {
    /// Name, or an empty string when the entry has no `name`.
    pub name: String,
    /// The entry exactly as written.
    pub raw: Value,
}

/// One entry of `strumLines`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrumLine {
    /// Number of lanes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_count: Option<u32>,
    /// Slot the line belongs to, for example `dad`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    /// Characters bound to this line.
    pub characters: Vec<String>,
    /// `type`, preserved because mods use both numbers and names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_type: Option<Value>,
    /// Whether the receptors are drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Receptor scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strum_scale: Option<f64>,
    /// Receptor spacing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strum_spacing: Option<f64>,
    /// Receptor position offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strum_pos: Option<StrumPos>,
    /// Line position on screen, as a fraction of the screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strum_line_pos: Option<f64>,
    /// Vocal track suffix for this line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocals_suffix: Option<String>,
    /// Notes in file order.
    pub notes: Vec<Note>,
    /// Properties that are not interpreted here.
    pub unknown_fields: BTreeMap<String, Value>,
    /// The line exactly as written.
    pub raw: Value,
}

/// Two-dimensional receptor offset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrumPos {
    /// Horizontal offset.
    pub x: f64,
    /// Vertical offset.
    pub y: f64,
}

/// A single note.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Hit time in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    /// Lane index (`id` in the source).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    /// Sustain length in milliseconds (`sLen` in the source).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sustain_length: Option<f64>,
    /// Note type index or name (`type` in the source), preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_type: Option<Value>,
    /// Any other property, preserved.
    pub extra: BTreeMap<String, Value>,
}

/// An event occurrence inside a chart or `events.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartEvent {
    /// Time in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    /// Event name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Heterogeneous parameter list, preserved exactly.
    pub params: Vec<Value>,
    /// Any other property, preserved.
    pub extra: BTreeMap<String, Value>,
}

/// A JSON event definition from `data/events`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDefinition {
    /// Event name, taken from the file stem.
    pub name: String,
    /// Mod-relative source path.
    pub source: String,
    /// Declared parameters in file order.
    pub params: Vec<EventParameter>,
    /// Properties that are not interpreted here.
    pub unknown_fields: BTreeMap<String, Value>,
    /// The complete document.
    pub raw: Value,
}

/// One declared event parameter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventParameter {
    /// Parameter name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Declared type expression, for example `Float(0.001, 10, 0.01, 2)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
    /// Default value, kept as JSON because mods mix strings and numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    /// Properties that are not interpreted here.
    pub unknown_fields: BTreeMap<String, Value>,
}

/// A script that was inventoried but never executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAsset {
    /// Mod-relative path.
    pub path: String,
    /// Script language.
    pub language: ScriptLanguage,
    /// File size in bytes.
    pub size: u64,
    /// True when the script lives under `data/events`.
    pub in_event_directory: bool,
}

impl ScriptAsset {
    /// File name without extension.
    pub fn stem(&self) -> &str {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        match name.rfind('.') {
            Some(index) if index > 0 => &name[..index],
            _ => name,
        }
    }
}

/// Recognized script languages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptLanguage {
    /// Haxe source, the native Codename scripting language.
    Haxe,
    /// Lua source, recognized so it can be reported.
    Lua,
    /// Any other script extension.
    Other(String),
}

impl ScriptLanguage {
    /// Lowercase identifier.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Haxe => "haxe",
            Self::Lua => "lua",
            Self::Other(name) => name,
        }
    }
}

/// A file that will never be loaded by the importer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedFile {
    /// Mod-relative path.
    pub path: String,
    /// Why it is unsupported.
    pub reason: UnsupportedReason,
    /// File size in bytes.
    pub size: u64,
}

/// Why a file is recorded but never used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnsupportedReason {
    /// `.dll`, `.ndll`, `.so`, `.dylib` or similar.
    NativeLibrary,
    /// `.exe`, `.msi`, `.bat` or similar.
    Executable,
    /// `.zip`, `.rar`, `.7z` and other archives.
    Archive,
    /// A file type the importer does not model.
    Unknown,
}

/// Complete, sorted file inventory plus atlas recognition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInventory {
    /// Regular files, sorted by path.
    pub entries: Vec<AssetEntry>,
    /// Recognized atlas candidates; contents are not decoded.
    pub atlases: Vec<Atlas>,
    /// Sum of all file sizes.
    pub total_bytes: u64,
}

impl AssetInventory {
    /// Builds an inventory from `(path, size)` pairs and detects atlases.
    pub fn build<E>(files: E) -> Self
    where
        E: IntoIterator<Item = (String, u64)>,
    {
        let mut entries: Vec<AssetEntry> = files
            .into_iter()
            .map(|(path, size)| AssetEntry {
                category: AssetCategory::classify(&path),
                kind: AssetKind::classify(&path),
                path,
                size,
            })
            .collect();
        entries.sort_by(|left, right| left.path.cmp(&right.path));

        let total_bytes = entries.iter().map(|entry| entry.size).sum();
        let atlases = detect_atlases(&entries);

        Self {
            entries,
            atlases,
            total_bytes,
        }
    }

    /// True when nothing was inventoried.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Exact lookup by mod-relative path.
    pub fn get(&self, path: &str) -> Option<&AssetEntry> {
        self.entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Case-insensitive lookup. Used to distinguish "missing" from "wrong case".
    pub fn get_case_insensitive(&self, path: &str) -> Option<&AssetEntry> {
        self.entries
            .iter()
            .find(|entry| entry.path.eq_ignore_ascii_case(path))
    }

    /// Exact lookup first, then a case-insensitive fallback.
    ///
    /// Returns [`AssetLookup::Missing`] when no candidate exists at all.
    pub fn lookup<'a>(&'a self, path: &str) -> AssetLookup<'a> {
        if let Some(entry) = self.get(path) {
            return AssetLookup::Found(entry);
        }
        match self.get_case_insensitive(path) {
            Some(entry) => AssetLookup::CaseMismatch { actual: entry },
            None => AssetLookup::Missing,
        }
    }

    /// Looks up the first candidate that exists exactly; if none does, reports
    /// the first case-insensitive match, otherwise [`AssetLookup::Missing`].
    pub fn lookup_any<'a>(&'a self, candidates: &[String]) -> AssetLookup<'a> {
        for candidate in candidates {
            if let Some(entry) = self.get(candidate) {
                return AssetLookup::Found(entry);
            }
        }
        for candidate in candidates {
            if let Some(entry) = self.get_case_insensitive(candidate) {
                return AssetLookup::CaseMismatch { actual: entry };
            }
        }
        AssetLookup::Missing
    }

    /// True when any file lives under `prefix`.
    pub fn has_directory(&self, prefix: &str) -> bool {
        let mut prefix = prefix.trim_end_matches('/').to_string();
        prefix.push('/');
        self.entries
            .iter()
            .any(|entry| entry.path.starts_with(&prefix))
    }

    /// Files under `prefix`, sorted.
    pub fn files_under<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a AssetEntry> + 'a {
        let mut prefix = prefix.trim_end_matches('/').to_string();
        prefix.push('/');
        self.entries
            .iter()
            .filter(move |entry| entry.path.starts_with(&prefix))
    }

    /// True when at least one file has one of `extensions` under `prefix`.
    pub fn has_extension_under(&self, prefix: &str, extensions: &[&str]) -> bool {
        self.files_under(prefix).any(|entry| {
            entry.path.rsplit_once('.').is_some_and(|(_, extension)| {
                extensions.contains(&extension.to_ascii_lowercase().as_str())
            })
        })
    }
}

/// Outcome of an inventory lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetLookup<'a> {
    /// The path exists exactly.
    Found(&'a AssetEntry),
    /// The path exists with different casing.
    CaseMismatch {
        /// The entry that matched case-insensitively.
        actual: &'a AssetEntry,
    },
    /// No such path.
    Missing,
}

/// One inventoried file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEntry {
    /// Mod-relative path with forward slashes.
    pub path: String,
    /// Top-level content category.
    pub category: AssetCategory,
    /// Detected file kind.
    pub kind: AssetKind,
    /// File size in bytes.
    pub size: u64,
}

/// Top-level directory a file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetCategory {
    /// `images/`
    Images,
    /// `songs/`
    Songs,
    /// `music/`
    Music,
    /// `sounds/`
    Sounds,
    /// `shaders/`
    Shaders,
    /// `fonts/`
    Fonts,
    /// `videos/`
    Videos,
    /// `data/`
    Data,
    /// Anything else, including files at the mod root.
    Other,
}

impl AssetCategory {
    /// Classifies a mod-relative path by its first segment.
    pub fn classify(path: &str) -> Self {
        match path.split('/').next().unwrap_or(path) {
            "images" => Self::Images,
            "songs" => Self::Songs,
            "music" => Self::Music,
            "sounds" => Self::Sounds,
            "shaders" => Self::Shaders,
            "fonts" => Self::Fonts,
            "videos" => Self::Videos,
            "data" => Self::Data,
            _ => Self::Other,
        }
    }
}

/// Detected file kind, based on extension only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    /// Raster or vector image.
    Image,
    /// XML document.
    Xml,
    /// JSON document.
    Json,
    /// Plain text.
    Text,
    /// Audio track.
    Audio,
    /// Video file.
    Video,
    /// Font file.
    Font,
    /// Shader source.
    Shader,
    /// Script source.
    Script(ScriptLanguage),
    /// Native library, executable or other opaque binary.
    Binary,
    /// Compressed archive.
    Archive,
    /// Unrecognized extension.
    Other,
}

impl AssetKind {
    /// Classifies a path by extension.
    pub fn classify(path: &str) -> Self {
        let extension = path
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => Self::Image,
            "xml" => Self::Xml,
            "json" => Self::Json,
            "txt" | "md" | "csv" | "ini" => Self::Text,
            "ogg" | "mp3" | "wav" | "flac" => Self::Audio,
            "mp4" | "webm" | "ogv" | "avi" | "mkv" => Self::Video,
            "ttf" | "otf" | "woff" | "woff2" => Self::Font,
            "frag" | "vert" | "glsl" | "wgsl" => Self::Shader,
            "hx" => Self::Script(ScriptLanguage::Haxe),
            "lua" => Self::Script(ScriptLanguage::Lua),
            "dll" | "ndll" | "so" | "dylib" | "exe" | "msi" | "bat" | "cmd" | "sh" | "py"
            | "js" | "jar" => Self::Binary,
            "zip" | "rar" | "7z" | "gz" | "tar" | "bz2" | "xz" => Self::Archive,
            _ => Self::Other,
        }
    }
}

/// A recognized atlas candidate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Atlas {
    /// Recognized atlas layout.
    pub kind: AtlasKind,
    /// Directory containing the atlas, when the layout is folder based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Spritesheet image path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Data files that describe the atlas.
    pub data_files: Vec<String>,
}

/// Recognized atlas layouts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AtlasKind {
    /// `name.png` plus `name.xml`, the Flixel Sparrow atlas layout.
    #[default]
    Sparrow,
    /// A folder containing `Animation.json`, the Adobe Animate texture-atlas
    /// layout.
    AnimateFolder,
}

/// Detects Sparrow pairs and Animate atlas folders.
///
/// Detection is filename based on purpose: the importer records that an atlas
/// exists without parsing or decoding frames.
fn detect_atlases(entries: &[AssetEntry]) -> Vec<Atlas> {
    let mut atlases = Vec::new();

    for entry in entries {
        if let Some((stem, extension)) = entry.path.rsplit_once('.') {
            if extension.eq_ignore_ascii_case("png") {
                let xml = format!("{stem}.xml");
                if entries.iter().any(|other| other.path == xml) {
                    atlases.push(Atlas {
                        kind: AtlasKind::Sparrow,
                        folder: stem.rsplit_once('/').map(|(folder, _)| folder.to_string()),
                        image: Some(entry.path.clone()),
                        data_files: vec![xml],
                    });
                }
            }
        }
    }

    let mut folders: BTreeMap<String, Vec<&AssetEntry>> = BTreeMap::new();
    for entry in entries {
        if entry.path.ends_with("/Animation.json") {
            if let Some((folder, _)) = entry.path.rsplit_once('/') {
                folders.entry(folder.to_string()).or_default().push(entry);
            }
        }
    }
    for (folder, animation_files) in folders {
        let image = entries
            .iter()
            .find(|entry| {
                entry.path.starts_with(&format!("{folder}/")) && entry.kind == AssetKind::Image
            })
            .map(|entry| entry.path.clone());
        let data_files = entries
            .iter()
            .filter(|entry| {
                entry.path.starts_with(&format!("{folder}/")) && entry.kind == AssetKind::Json
            })
            .map(|entry| entry.path.clone())
            .collect();
        atlases.push(Atlas {
            kind: AtlasKind::AnimateFolder,
            folder: Some(folder.clone()),
            image,
            data_files: animation_files
                .first()
                .map(|_| data_files)
                .unwrap_or_default(),
        });
    }

    atlases.sort_by(|left, right| left.folder.cmp(&right.folder));
    atlases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_classifies_and_sorts() {
        let inventory = AssetInventory::build(vec![
            ("songs/black-humor/song/Inst.ogg".to_string(), 10),
            ("images/characters/Var.png".to_string(), 20),
            ("data/characters/var.xml".to_string(), 5),
            ("data/config/modpack.ini".to_string(), 1),
        ]);
        assert_eq!(inventory.total_bytes, 36);
        assert_eq!(inventory.entries[0].path, "data/characters/var.xml");
        assert_eq!(inventory.entries[0].category, AssetCategory::Data);
        assert_eq!(inventory.entries[1].kind, AssetKind::Text);
        let sound = inventory.get("songs/black-humor/song/Inst.ogg").unwrap();
        assert_eq!(sound.category, AssetCategory::Songs);
        assert_eq!(sound.kind, AssetKind::Audio);
    }

    #[test]
    fn lookup_reports_case_mismatches() {
        let inventory = AssetInventory::build(vec![
            ("images/characters/Var.png".to_string(), 1),
            ("images/characters/var.xml".to_string(), 1),
        ]);
        assert!(matches!(
            inventory.lookup("images/characters/Var.png"),
            AssetLookup::Found(_)
        ));
        assert!(matches!(
            inventory.lookup("images/characters/VAR.png"),
            AssetLookup::CaseMismatch { .. }
        ));
        assert!(matches!(
            inventory.lookup("images/characters/Var.xml"),
            AssetLookup::CaseMismatch { .. }
        ));
        assert!(matches!(
            inventory.lookup("images/characters/Missing.png"),
            AssetLookup::Missing
        ));
    }

    #[test]
    fn lookup_any_prefers_exact_then_case_insensitive() {
        let inventory = AssetInventory::build(vec![
            ("songs/x/song/Inst.ogg".to_string(), 1),
            ("songs/x/song/Voices.ogg".to_string(), 1),
        ]);
        let exact = vec![
            "songs/x/song/inst.ogg".to_string(),
            "songs/x/song/Inst.ogg".to_string(),
        ];
        assert!(matches!(
            inventory.lookup_any(&exact),
            AssetLookup::Found(_)
        ));
        let wrong_case = vec!["songs/x/song/INST.OGG".to_string()];
        assert!(matches!(
            inventory.lookup_any(&wrong_case),
            AssetLookup::CaseMismatch { .. }
        ));
    }

    #[test]
    fn detects_sparrow_and_animate_atlases() {
        let inventory = AssetInventory::build(vec![
            ("images/characters/Var.png".to_string(), 1),
            ("images/characters/Var.xml".to_string(), 1),
            ("images/characters/newBF/Animation.json".to_string(), 1),
            ("images/characters/newBF/spritemap1.png".to_string(), 1),
            ("images/characters/newBF/spritemap1.json".to_string(), 1),
        ]);
        assert_eq!(inventory.atlases.len(), 2);
        let sparrow = inventory
            .atlases
            .iter()
            .find(|atlas| atlas.kind == AtlasKind::Sparrow)
            .unwrap();
        assert_eq!(sparrow.image.as_deref(), Some("images/characters/Var.png"));
        assert_eq!(sparrow.folder.as_deref(), Some("images/characters"));
        let animate = inventory
            .atlases
            .iter()
            .find(|atlas| atlas.kind == AtlasKind::AnimateFolder)
            .unwrap();
        assert_eq!(animate.folder.as_deref(), Some("images/characters/newBF"));
        assert_eq!(
            animate.image.as_deref(),
            Some("images/characters/newBF/spritemap1.png")
        );
        assert_eq!(animate.data_files.len(), 2);
    }

    #[test]
    fn directory_and_extension_helpers() {
        let inventory = AssetInventory::build(vec![
            ("images/stages/castle/floor.png".to_string(), 1),
            ("images/stages/castle/back.png".to_string(), 1),
            ("images/icons/var.png".to_string(), 1),
        ]);
        assert!(inventory.has_directory("images/stages/castle"));
        assert!(inventory.has_directory("images/stages/castle/"));
        assert!(!inventory.has_directory("images/stages/dark-castle"));
        assert_eq!(inventory.files_under("images/stages/castle").count(), 2);
        assert!(inventory.has_extension_under("images/stages/castle", &["png"]));
        assert!(!inventory.has_extension_under("images/stages/castle", &["xml"]));
    }

    #[test]
    fn script_assets_expose_stems() {
        let script = ScriptAsset {
            path: "data/events/Camera Bump.hx".to_string(),
            language: ScriptLanguage::Haxe,
            size: 1,
            in_event_directory: true,
        };
        assert_eq!(script.stem(), "Camera Bump");
    }

    #[test]
    fn mod_helpers_find_content_case_insensitively() {
        let mut module = CodenameMod::default();
        module.characters.push(Character {
            id: "Var".to_string(),
            ..Character::default()
        });
        module.stages.push(Stage {
            id: "Castle".to_string(),
            ..Stage::default()
        });
        module.events.push(EventDefinition {
            name: "Camera Bump".to_string(),
            ..EventDefinition::default()
        });
        assert!(module.character("var").is_some());
        assert!(module.stage("castle").is_some());
        assert!(module.defines_event("camera bump"));
        assert!(!module.defines_event("Lyrics"));
    }

    #[test]
    fn model_round_trips_through_json() {
        let mut module = CodenameMod {
            name: "demo".to_string(),
            ..CodenameMod::default()
        };
        module.characters.push(Character {
            id: "var".to_string(),
            sprite: Some("Var".to_string()),
            attributes: vec![Attribute {
                name: "mystery".to_string(),
                value: "42".to_string(),
            }],
            ..Character::default()
        });
        module.stages.push(Stage {
            id: "castle".to_string(),
            elements: vec![
                StageElement::Sprite(StageSprite {
                    name: Some("floor".to_string()),
                    ..StageSprite::default()
                }),
                StageElement::Anchor(StageAnchor {
                    role: AnchorRole::Girlfriend,
                    tag: "gf".to_string(),
                    ..StageAnchor::default()
                }),
            ],
            ..Stage::default()
        });
        let json = serde_json::to_string(&module).expect("serialize");
        let parsed: CodenameMod = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, module);
        assert_eq!(
            parsed.stages[0].anchors().next().unwrap().tag,
            "gf",
            "anchor order survives"
        );
        assert_eq!(parsed.stages[0].sprites().count(), 1);
    }
}
