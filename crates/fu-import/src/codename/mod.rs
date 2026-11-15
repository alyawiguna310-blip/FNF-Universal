//! Codename Engine mod discovery, parsing and validation.
//!
//! The layer is strictly read-only: it inventories scripts and native binaries
//! but never reads, compiles, loads or executes them.

pub mod discovery;
pub mod model;
pub mod parse;
pub mod validate;

pub use discovery::{
    autoload_entries, discover_mods_root, open_mod_dir, scan_mod_tree, DiscoveredMod, ModFile,
    ModTree, ModsDiscovery, AUTOLOAD_FILE,
};
pub use model::{
    AnchorRole, AssetCategory, AssetEntry, AssetInventory, AssetKind, AssetLookup, Atlas,
    AtlasKind, Attribute, Character, CharacterAnimation, Chart, ChartEvent, ChartFormat,
    CodenameMod, ConfigSource, EventDefinition, EventParameter, ModpackConfig, Note,
    NoteTypeDefinition, RawElement, ScriptAsset, ScriptLanguage, Song, SongEvents, SongMetadata,
    Stage, StageAnchor, StageElement, StageSprite, StateRedirect, StrumLine, StrumPos,
    UnsupportedFile, UnsupportedReason,
};
pub use parse::{
    parse_mod, parse_mod_with, ParseOptions, ParsedMod, CHARACTERS_DIRECTORY, CONFIG_DIRECTORY,
    EVENTS_DIRECTORY, FLAGS_INI, MODPACK_INI, SONGS_DIRECTORY, STAGES_DIRECTORY,
};
pub use validate::{validate, validate_into};
