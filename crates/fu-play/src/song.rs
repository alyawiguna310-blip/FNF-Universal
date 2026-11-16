//! Loading an imported V-Slice song into the engine's playable chart.
//! This is the bridge between the read-only importer (`fu-import`) and the
//! gameplay core. It resolves one song of one difficulty into a [`Chart`] plus
//! its instrumental audio, and reports why it cannot when it cannot.
//! Only player notes are handed to gameplay.

use std::fmt;
use std::path::Path;

use fu_assets::load_bytes;
use fu_import::vslice;
use fu_vfs::{CaseSensitivity, MountPolicy, Vfs};

use crate::audio::decode_duration_ms;
use crate::game::Chart;

use fu_chart::RuntimeChart;

/// Time kept after the last note when the song length cannot be read from audio.
pub const TAIL_MS: u32 = 2_000;

/// Fallback tempo when a song declares no usable BPM change.
pub const DEFAULT_BPM: f32 = 120.0;

/// A song resolved into engine types and ready to play.
#[derive(Debug, Clone)]
pub struct PlayableSong {
    /// Chart handed to [`crate::game::Gameplay`].
    pub chart: Chart,
    /// Song id (its directory name).
    pub song_id: String,
    /// Human readable song name.
    pub title: String,
    /// Difficulty that was resolved.
    pub difficulty: String,
    /// Every difficulty the chart offers.
    pub available_difficulties: Vec<String>,
    /// Instrumental audio bytes, when the mod provides a readable track.
    pub audio: Option<Vec<u8>>,
    /// Mod-relative path the audio was read from.
    pub audio_path: Option<String>,
    /// Duration read from the audio, when it could be decoded.
    pub audio_duration_ms: Option<u32>,
    /// Number of import warnings that applied to the whole mod.
    pub import_warnings: usize,
}

impl PlayableSong {
    /// True when judging is driven by real audio rather than a clock.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Note count handed to gameplay.
    pub fn note_count(&self) -> usize {
        self.chart.notes.len()
    }
}

/// Why a song could not be prepared for play.
#[derive(Debug)]
pub enum SongError {
    /// The whole mod directory could not be imported.
    Import(fu_import::ImportError),
    /// No song with the requested id exists in the mod.
    SongNotFound {
        /// Requested id.
        id: String,
        /// Ids the mod does provide.
        available: Vec<String>,
    },
    /// The song has no readable chart.
    NoChart {
        /// Song id.
        song: String,
    },
    /// The requested difficulty does not exist.
    NoSuchDifficulty {
        /// Song id.
        song: String,
        /// Requested difficulty.
        requested: String,
        /// Difficulties the chart provides.
        available: Vec<String>,
    },
    /// The chart has no player notes that the engine can judge.
    NoPlayableNotes {
        /// Song id.
        song: String,
        /// Difficulty that was resolved.
        difficulty: String,
    },
    /// Audio or another file could not be read through the VFS.
    Unreadable(String),
}

impl fmt::Display for SongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Import(error) => write!(f, "could not import mod: {error}"),
            Self::SongNotFound { id, available } => write!(
                f,
                "song '{id}' is not in this mod; available songs: {}",
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                }
            ),
            Self::NoChart { song } => write!(f, "song '{song}' has no readable chart"),
            Self::NoSuchDifficulty {
                song,
                requested,
                available,
            } => write!(
                f,
                "song '{song}' has no difficulty '{requested}'; available: {}",
                available.join(", ")
            ),
            Self::NoPlayableNotes { song, difficulty } => write!(
                f,
                "song '{song}' difficulty '{difficulty}' has no player notes to judge"
            ),
            Self::Unreadable(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SongError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(error) => Some(error),
            _ => None,
        }
    }
}

/// Loads a V-Slice song from a mod directory.
/// `difficulty` is matched case-insensitively; when it is `None` the first
/// difficulty the metadata declares and the chart actually contains is used.
pub fn load_song(
    mod_root: &Path,
    song_id: &str,
    difficulty: Option<&str>,
) -> Result<PlayableSong, SongError> {
    let outcome = fu_import::vslice::import_vslice_mod(mod_root).map_err(SongError::Import)?;

    let module = &outcome.module;

    let song = module
        .song(song_id)
        .ok_or_else(|| SongError::SongNotFound {
            id: song_id.to_string(),
            available: module.songs.iter().map(|s| s.id.clone()).collect(),
        })?;

    let chart = song
        .chart_for(None)
        .or_else(|| song.charts.first())
        .ok_or_else(|| SongError::NoChart {
            song: song.id.clone(),
        })?;

    // Collect difficulties from chart notes keys
    let available_difficulties: Vec<String> = chart.notes.keys().cloned().collect();

    // Resolve the difficulty to use - normalize to lowercase for case-insensitive matching
    let resolved_difficulty = if let Some(d) = difficulty {
        d.to_lowercase()
    } else {
        available_difficulties.first().cloned().unwrap_or_else(|| {
            chart
                .notes
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "easy".to_string())
        })
    };

    // If a specific difficulty was requested but does not exist, report it immediately
    if difficulty.is_some() && !available_difficulties.contains(&resolved_difficulty) {
        return Err(SongError::NoSuchDifficulty {
            song: song.id.clone(),
            requested: difficulty.unwrap().to_string(),
            available: available_difficulties.clone(),
        });
    }

    // Resolve the runtime chart: it interprets the note times against the
    // metadata's BPM changes once, and gameplay reads exactly that map.
    let runtime =
        RuntimeChart::from_vslice_chart(chart, song.metadata.as_ref(), &resolved_difficulty)
            .ok_or_else(|| SongError::NoPlayableNotes {
                song: song.id.clone(),
                difficulty: resolved_difficulty.clone(),
            })?;

    if runtime.notes.is_empty() {
        return Err(SongError::NoPlayableNotes {
            song: song.id.clone(),
            difficulty: resolved_difficulty.clone(),
        });
    }

    // Read audio
    let audio = read_audio(mod_root, &song)?;

    let audio_duration_ms = audio
        .as_ref()
        .and_then(|(_, bytes)| decode_duration_ms(bytes));

    let last_note = runtime.last_note_ms();
    let length_ms = audio_duration_ms
        .map(|duration| duration.max(clamp_ms(last_note + 500.0)))
        .unwrap_or_else(|| clamp_ms(last_note + TAIL_MS as f64))
        .max(1);

    let title = song
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.song_name.clone())
        .unwrap_or_else(|| song.id.clone());

    let runtime = runtime.with_length_ms(length_ms as f64);

    Ok(PlayableSong {
        chart: Chart::from_runtime(&runtime, length_ms),
        song_id: song.id.clone(),
        title,
        difficulty: resolved_difficulty.clone(),
        available_difficulties,
        audio_path: audio.as_ref().map(|(path, _)| path.clone()),
        audio: audio.map(|(_, bytes)| bytes),
        audio_duration_ms,
        import_warnings: outcome.report.count(fu_import::Severity::Warning),
    })
}

fn clamp_ms(value: f64) -> u32 {
    value.max(0.0).min(u32::MAX as f64) as u32
}

fn read_audio(
    mod_root: &Path,
    song: &vslice::Song,
) -> Result<Option<(String, Vec<u8>)>, SongError> {
    let candidates: Vec<&String> = song.audio.iter().map(|entry| &entry.path).collect();
    let chosen = candidates
        .iter()
        .find(|path| path.ends_with("Inst.ogg"))
        .or_else(|| {
            candidates
                .iter()
                .find(|path| path.to_ascii_lowercase().ends_with(".ogg"))
        })
        .or_else(|| candidates.first());

    let Some(path) = chosen else {
        return Ok(None);
    };

    let mut vfs = Vfs::new();
    vfs.mount_physical(
        "mod",
        "/",
        mod_root,
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
    )
    .map_err(|error| SongError::Unreadable(format!("could not mount mod root: {error}")))?;

    let virtual_path = format!("/{path}");
    let bytes = load_bytes(&vfs, &virtual_path)
        .map_err(|error| SongError::Unreadable(format!("audio '{virtual_path}': {error}")))?;

    Ok(Some((path.to_string(), bytes)))
}
