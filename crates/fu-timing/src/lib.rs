//! Codename Engine timing, ported from Haxe to Rust.
//!
//! The reference is `source/funkin/backend/system/Conductor.hx` at Codename
//! commit `c2b73fa` (see `docs/CODENAME_RUST_MIGRATION.md`). This crate is a
//! behavioural port, not a syntax translation: the same formulas, the same
//! change-map construction from chart events, the same index searches and the
//! same step/beat/measure derivation.
//!
//! It deliberately does **not** own an audio device. The caller supplies the
//! song position in milliseconds (Codename reads it from the music stream);
//! [`Conductor::update`] turns that position into steps, beats and measures and
//! reports which signals fired.
//!
//! # Modules
//!
//! * [`bpm`] — [`bpm::BpmChange`], [`bpm::BpmMap`] and the time/step/beat/
//!   measure conversions.
//! * [`conductor`] — the stateful [`conductor::Conductor`].
//! * [`windows`] — the four judgement presets from `HitWindowData.hx`.
//!
//! # Integration
//!
//! [`bpm::BpmMap::from_codename_chart`] and
//! [`bpm::BpmMap::from_codename_song`] build the map from the existing
//! `fu-import::codename` model, so no chart is parsed twice.

pub mod bpm;
pub mod conductor;
pub mod windows;

pub use bpm::{
    BpmChange, BpmMap, RejectReason, RejectedEvent, TimingEvent, TimingEventKind,
    DEFAULT_BEATS_PER_MEASURE, DEFAULT_BPM, DEFAULT_STEPS_PER_BEAT,
};
pub use conductor::{BpmSignal, Conductor, Tick, TimeSignatureSignal};
pub use windows::{HitRating, HitWindows, WindowPreset, JUDGE_SCALES};
