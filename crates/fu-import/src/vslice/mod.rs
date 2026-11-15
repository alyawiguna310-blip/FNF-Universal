//! V-Slice (Friday Night Funkin') mod import.
//!
//! The layer is read-only: scripts and native binaries are inventoried and are
//! never read, compiled, loaded or executed. Unknown JSON fields are retained
//! verbatim on every structure, and documents that cannot be decoded are kept as
//! raw text so a later conversion step can still inspect them.
//!
//! # Layout covered
//!
//! * `_polymod_meta.json` (or `manifest.json`) at the mod root.
//! * `songs/<id>/<id>-metadata.json` and `songs/<id>/<id>-chart.json`, including
//!   `-<variation>` files, the prefix-free `metadata.json`/`chart.json` names and
//!   a `charts/` folder fallback.
//! * `characters/` or `data/characters/*.json`.
//! * `stages/` or `data/stages/*.json`.
//! * `weeks/` or `data/weeks/*.json`.
//!
//! # What this is not
//!
//! The importer has been exercised against synthetic fixtures that follow the
//! public V-Slice data formats (song metadata `2.2.x`, chart data `2.0.x`). It
//! has not been validated against a complete real V-Slice mod, so compatibility
//! is reported per finding rather than claimed as a whole. Content a mod inherits
//! from the base game is reported as unverified, never as missing.

use std::path::Path;

use crate::discovery::DiscoveredMod;
use crate::report::{ImportReport, SupportLevel};
use crate::ImportError;

pub mod discovery;
pub mod model;
pub mod parse;
pub mod validate;

pub use discovery::{
    discover_vslice_mods, is_vslice_directory, metadata_kind, open_vslice_mod_dir, MetadataKind,
    VsliceDiscovery,
};
pub use model::{
    Character, CharacterAnimation, Chart, ChartEvent, Contributor, Note, PlayData, ProjectMetadata,
    Song, SongCharacters, SongMetadata, Stage, StageCharacter, StageProp, TimeChange,
    UnparsedDocument, VsliceMod, Week, VSLICE_MANIFEST_FILE, VSLICE_METADATA_FILE,
};
pub use parse::{
    parse_vslice_mod, parse_vslice_mod_with, ParsedVsliceMod, VsliceParseOptions,
    CHARACTERS_DIRECTORIES, SONGS_DIRECTORY, STAGES_DIRECTORIES, WEEKS_DIRECTORIES,
};
pub use validate::{
    validate, validate_into, SUPPORTED_CHART_VERSION_RULE, SUPPORTED_METADATA_VERSION_RULE,
};

/// A successfully imported V-Slice mod and its diagnostics.
#[derive(Debug, Clone)]
pub struct VsliceOutcome {
    /// The imported model, including unknown content.
    pub module: VsliceMod,
    /// Every finding from scanning, parsing and validation.
    pub report: ImportReport,
}

impl VsliceOutcome {
    /// True when nothing was reported at error severity.
    pub fn is_importable(&self) -> bool {
        !self.report.has_errors()
    }

    /// Overall support classification of this import.
    pub fn support(&self) -> SupportLevel {
        self.report.overall_support()
    }
}

/// Imports one V-Slice mod directory with default limits.
pub fn import_vslice_mod(directory: &Path) -> Result<VsliceOutcome, ImportError> {
    import_vslice_mod_with(directory, VsliceParseOptions::default())
}

/// Imports one V-Slice mod directory, then validates it.
pub fn import_vslice_mod_with(
    directory: &Path,
    options: VsliceParseOptions,
) -> Result<VsliceOutcome, ImportError> {
    let discovered = open_vslice_mod_dir(directory).map_err(|source| ImportError {
        path: directory.to_path_buf(),
        source,
    })?;
    Ok(import_discovered_vslice(&discovered, options))
}

/// Imports an already-resolved mod, which is what discovery returns.
pub fn import_discovered_vslice(
    discovered: &DiscoveredMod,
    options: VsliceParseOptions,
) -> VsliceOutcome {
    let ParsedVsliceMod { module, mut report } = parse_vslice_mod_with(discovered, options);
    validate_into(&module, &mut report);
    report.sort();
    VsliceOutcome { module, report }
}

/// Every V-Slice mod reachable from a `mods/` directory.
#[derive(Debug)]
pub struct VsliceModsRootOutcome {
    /// Discovery findings, including directories that are not V-Slice mods.
    pub discovery: VsliceDiscovery,
    /// One outcome per discovered mod.
    pub mods: Vec<VsliceOutcome>,
}

/// Discovers and imports every V-Slice mod in a `mods/` directory.
pub fn import_vslice_mods_root(mods_root: &Path) -> VsliceModsRootOutcome {
    import_vslice_mods_root_with(mods_root, VsliceParseOptions::default())
}

/// Discovers and imports every V-Slice mod in a `mods/` directory.
pub fn import_vslice_mods_root_with(
    mods_root: &Path,
    options: VsliceParseOptions,
) -> VsliceModsRootOutcome {
    let discovery = discover_vslice_mods(mods_root);
    let mods = discovery
        .mods
        .iter()
        .map(|discovered| import_discovered_vslice(discovered, options))
        .collect();
    VsliceModsRootOutcome { discovery, mods }
}
