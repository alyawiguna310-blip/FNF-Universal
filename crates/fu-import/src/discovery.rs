//! Format-agnostic mod directory discovery shared by every importer.
//!
//! The scanning rules are identical for all supported engines: never follow a
//! symbolic link, never leave the selected directory, sort results
//! deterministically and report anything that had to be skipped. Engine
//! specific importers add their own content layout on top of this.

pub use crate::codename::discovery::{
    open_mod_dir, scan_mod_tree, DiscoveredMod, ModFile, ModTree, DEFAULT_MAX_DEPTH,
};
