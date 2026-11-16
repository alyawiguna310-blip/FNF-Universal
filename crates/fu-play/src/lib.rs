//! Minimal playable Friday Night Funkin' style prototype.
//!
//! The crate is split into a pure gameplay core ([`game`]), window/audio glue
//! ([`audio`]), rendering glue ([`visuals`]), session state ([`session`]) and the
//! importer bridge that turns an imported V-Slice song into a playable chart
//! ([`song`]). `game` and `session` are deliberately free of windowing and audio
//! dependencies so the timing rules can be tested headlessly.
//!
//! The `fu-play` binary is the windowed runner:
//! `fu-play --mod <mod dir> --song <id> [--difficulty <id>]`.

pub mod audio;
pub mod codename_content;
pub mod codename_song;
pub mod game;
pub mod opponent;
pub mod resources;
pub mod session;
pub mod song;
pub mod visuals;
