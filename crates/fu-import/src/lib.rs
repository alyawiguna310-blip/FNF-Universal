//! Read-only, non-executing importers for third-party FNF mod content.
//!
//! # Scope
//!
//! The crate reads a mod directory and produces a complete, serializable model
//! plus a diagnostic report. It is a foundation, not a compatibility claim: any
//! behaviour that cannot be verified from the files alone is reported as
//! unverified rather than assumed.
//!
//! # Supported formats
//!
//! * [`codename`] — Codename Engine mods (`data/config/modpack.ini`, XML
//!   characters and stages, Codename chart JSON).
//! * [`vslice`] — V-Slice (Friday Night Funkin') mods (`_polymod_meta.json`,
//!   song `metadata.json`/`chart.json` pairs, character and stage JSON).
//!
//! The two importers share [`report`], [`path`] and [`discovery`], and each one
//! keeps its parsing isolated so a change in one format cannot affect the other.
//!
//! # Safety model
//!
//! * Every path taken from imported content is parsed into a
//!   [`path::SafeRelativePath`], which rejects absolute, drive-qualified,
//!   traversing, NUL-containing and alternate-data-stream paths.
//! * Every file is opened through [`path::read_text_within`], which
//!   canonicalizes the target and proves it is still inside the mod root, so a
//!   symlink cannot redirect a read outside the selected directory.
//! * Directory scanning never follows symbolic links.
//! * Scripts are inventoried by path and size only: they are never read,
//!   compiled or executed. Native libraries and executables are recorded and
//!   never loaded.
//! * XML parsing never resolves external entities or DTDs, so a hostile
//!   document cannot cause file or network access.
//! * The crate contains no `unsafe` code.
//!
//! # Example
//!
//! ```no_run
//! use fu_import::import_mod;
//!
//! let outcome = import_mod(std::path::Path::new("mods/Vs Var Overhaul [Demo]"))?;
//! println!("{}", outcome.report.to_text());
//! println!("{} characters, {} songs", outcome.module.characters.len(), outcome.module.songs.len());
//! # Ok::<(), fu_import::ImportError>(())
//! ```

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use thiserror::Error;

pub mod codename;
pub mod discovery;
pub mod ini;
pub mod path;
pub mod report;
pub mod vslice;
pub mod xml;

#[doc(hidden)]
pub mod test_support;

pub use codename::{
    discover_mods_root, open_mod_dir, parse_mod, parse_mod_with, validate, validate_into,
    CodenameMod, DiscoveredMod, ModsDiscovery, ParseOptions, ParsedMod,
};
pub use report::{Diagnostic, DiagnosticKind, ImportReport, ImportSummary, Severity, SupportLevel};
pub use vslice::{
    import_vslice_mod, import_vslice_mod_with, import_vslice_mods_root, is_vslice_directory,
    parse_vslice_mod, parse_vslice_mod_with, validate as validate_vslice, MetadataKind, VsliceMod,
    VsliceOutcome, VsliceParseOptions, VSLICE_MANIFEST_FILE, VSLICE_METADATA_FILE,
};

/// Failure that prevents a directory from being imported at all.
///
/// Problems inside an otherwise importable mod are reported as diagnostics
/// instead, so this error is limited to the selected root being unusable.
#[derive(Debug, Error)]
#[error("{path}: {source}")]
pub struct ImportError {
    /// The directory that could not be opened.
    pub path: PathBuf,
    /// Why it could not be opened.
    #[source]
    pub source: path::PathError,
}

/// A successfully imported mod and its diagnostics.
#[derive(Debug, Clone)]
pub struct ImportOutcome {
    /// The imported model, including unknown content.
    pub module: CodenameMod,
    /// Every finding from scanning, parsing and validation.
    pub report: ImportReport,
}

impl ImportOutcome {
    /// True when nothing was reported at error severity.
    ///
    /// Warnings still allow a usable import; check
    /// [`ImportReport::overall_support`] for a finer classification.
    pub fn is_importable(&self) -> bool {
        !self.report.has_errors()
    }

    /// Overall support classification of this import.
    pub fn support(&self) -> SupportLevel {
        self.report.overall_support()
    }
}

/// Imports one mod directory with default limits.
pub fn import_mod(directory: &Path) -> Result<ImportOutcome, ImportError> {
    import_mod_with(directory, ParseOptions::default())
}

/// Imports one mod directory, then validates it.
pub fn import_mod_with(
    directory: &Path,
    options: ParseOptions,
) -> Result<ImportOutcome, ImportError> {
    let discovered = open_mod_dir(directory).map_err(|source| ImportError {
        path: directory.to_path_buf(),
        source,
    })?;
    Ok(import_discovered(&discovered, options))
}

/// Imports an already-resolved mod, which is what discovery returns.
pub fn import_discovered(discovered: &DiscoveredMod, options: ParseOptions) -> ImportOutcome {
    let ParsedMod { module, mut report } = parse_mod_with(discovered, options);
    validate_into(&module, &mut report);
    report.sort();
    ImportOutcome { module, report }
}

/// Every mod reachable from a `mods/` directory, with discovery findings.
#[derive(Debug)]
pub struct ModsRootOutcome {
    /// Autoload parsing and scanning findings, including mods that failed to open.
    pub discovery: ModsDiscovery,
    /// One outcome per mod that could be opened, in autoload order.
    pub mods: Vec<ImportOutcome>,
}

/// Discovers and imports every mod listed by a `mods/` directory.
pub fn import_mods_root(mods_root: &Path) -> ModsRootOutcome {
    import_mods_root_with(mods_root, ParseOptions::default())
}

/// Discovers and imports every mod listed by a `mods/` directory.
pub fn import_mods_root_with(mods_root: &Path, options: ParseOptions) -> ModsRootOutcome {
    let discovery = discover_mods_root(mods_root);
    let mods = discovery
        .mods
        .iter()
        .map(|discovered| import_discovered(discovered, options))
        .collect();
    ModsRootOutcome { discovery, mods }
}

/// True when `path` is a directory that could be imported.
pub fn is_importable_directory(path: &Path) -> bool {
    open_mod_dir(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importing_a_missing_directory_is_an_error() {
        let temp = test_support::TempDir::new("lib-missing").unwrap();
        let error = import_mod(&temp.join("nope")).unwrap_err();
        assert!(error.to_string().contains("nope"));
        assert!(!is_importable_directory(&temp.join("nope")));
        assert!(is_importable_directory(temp.path()));
    }

    #[test]
    fn importing_an_empty_directory_is_importable_with_warnings() {
        let temp = test_support::TempDir::new("lib-empty").unwrap();
        let outcome = import_mod(temp.path()).unwrap();
        assert!(outcome.is_importable());
        assert!(outcome.report.contains_code("missing-metadata"));
        assert_eq!(outcome.module.songs.len(), 0);
    }
}
