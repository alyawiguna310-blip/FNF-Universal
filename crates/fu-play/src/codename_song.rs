//! Loading a Codename Engine song into the engine's playable chart.
//!
//! This is the Codename counterpart of [`crate::song`]: it resolves one song
//! of one difficulty into a [`crate::song::PlayableSong`] plus its
//! instrumental audio, using the read-only `fu-import` Codename importer.
//! Only player notes (strum lines whose position is `boyfriend`) are handed
//! to gameplay, mirroring the V-Slice loader's player-only behaviour.

use std::path::Path;

use fu_assets::load_bytes;
use fu_import::codename;
use fu_vfs::{CaseSensitivity, MountPolicy, Vfs};

use crate::audio::decode_duration_ms;
use crate::game::Chart;
use crate::song::{PlayableSong, SongError, TAIL_MS};

use fu_chart::RuntimeChart;

/// Time kept after the last note when the song length cannot be read from audio.
/// Re-exported for convenience so callers do not need to know which loader they used.

/// Returns true when `mod_root` looks like a Codename Engine mod directory.
///
/// Detection requires a Codename-specific marker: either a `data/config/modpack.ini`
/// file or at least one `songs/<name>/meta.json` file. A directory that merely
/// contains recognizable files (for example an installation's `assets/` tree)
/// is not classified as a mod.
pub fn is_codename_mod(mod_root: &Path) -> bool {
    if !mod_root.is_dir() {
        return false;
    }
    // Marker 1: modpack.ini at the expected location.
    if mod_root
        .join("data")
        .join("config")
        .join("modpack.ini")
        .is_file()
    {
        return true;
    }
    // Marker 2: at least one song directory with a meta.json.
    let songs_dir = mod_root.join("songs");
    if songs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&songs_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if entry.path().join("meta.json").is_file() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Loads a Codename Engine song from a mod directory.
///
/// `difficulty` is matched case-insensitively against the chart file stems.
/// When it is `None` the first difficulty declared by the metadata (and present
/// in the charts) is used, falling back to the first chart.
pub fn load_codename_song(
    mod_root: &Path,
    song_id: &str,
    difficulty: Option<&str>,
) -> Result<PlayableSong, SongError> {
    let discovered = codename::open_mod_dir(mod_root).map_err(|source| {
        SongError::Import(fu_import::ImportError {
            path: mod_root.to_path_buf(),
            source,
        })
    })?;

    let parsed = codename::parse_mod(&discovered);
    let module = &parsed.module;

    let song = module
        .song(song_id)
        .ok_or_else(|| SongError::SongNotFound {
            id: song_id.to_string(),
            available: module.songs.iter().map(|s| s.name.clone()).collect(),
        })?;

    // Available difficulties: prefer metadata, fall back to chart stems.
    let available_difficulties: Vec<String> = if !song
        .metadata
        .as_ref()
        .map(|m| m.difficulties.is_empty())
        .unwrap_or(true)
    {
        song.metadata.as_ref().unwrap().difficulties.clone()
    } else {
        song.charts.iter().map(|c| c.difficulty.clone()).collect()
    };
    // Ensure every listed difficulty actually has a chart; keep only those that do.
    let available_difficulties: Vec<String> = available_difficulties
        .into_iter()
        .filter(|d| {
            song.charts
                .iter()
                .any(|c| c.difficulty.eq_ignore_ascii_case(d))
        })
        .collect();
    // If metadata listed nothing usable, fall back to chart stems.
    let available_difficulties = if available_difficulties.is_empty() {
        song.charts.iter().map(|c| c.difficulty.clone()).collect()
    } else {
        available_difficulties
    };

    // Resolve difficulty.
    let resolved_difficulty = if let Some(d) = difficulty {
        let lower = d.to_lowercase();
        // Find a chart whose difficulty matches case-insensitively.
        if let Some(chart) = song
            .charts
            .iter()
            .find(|c| c.difficulty.eq_ignore_ascii_case(d))
        {
            chart.difficulty.clone()
        } else {
            lower
        }
    } else {
        available_difficulties
            .first()
            .cloned()
            .unwrap_or_else(|| "easy".to_string())
    };

    if difficulty.is_some()
        && !available_difficulties
            .iter()
            .any(|d| d.eq_ignore_ascii_case(&resolved_difficulty))
    {
        return Err(SongError::NoSuchDifficulty {
            song: song.name.clone(),
            requested: difficulty.unwrap().to_string(),
            available: available_difficulties.clone(),
        });
    }

    // Find the chart for the resolved difficulty.
    let chart = song
        .chart(&resolved_difficulty)
        .ok_or_else(|| SongError::NoChart {
            song: song.name.clone(),
        })?;

    // The runtime chart interprets the chart's own timing events, including
    // the song-level events.json, so BPM and time-signature changes reach
    // gameplay instead of being flattened to the metadata tempo.
    let runtime =
        RuntimeChart::from_codename_chart(chart, song.events.as_ref(), song.metadata.as_ref());

    if runtime.notes.is_empty() {
        return Err(SongError::NoPlayableNotes {
            song: song.name.clone(),
            difficulty: resolved_difficulty.clone(),
        });
    }

    // Read audio through VFS.
    let audio = read_codename_audio(mod_root, song)?;

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
        .and_then(|m| m.display_name.clone())
        .unwrap_or_else(|| song.name.clone());

    let runtime = runtime.with_length_ms(length_ms as f64);

    Ok(PlayableSong {
        chart: Chart::from_runtime(&runtime, length_ms),
        song_id: song.name.clone(),
        title,
        difficulty: resolved_difficulty.clone(),
        available_difficulties,
        audio_path: audio.as_ref().map(|(path, _)| path.clone()),
        audio: audio.map(|(_, bytes)| bytes),
        audio_duration_ms,
        import_warnings: parsed.report.count(fu_import::Severity::Warning),
    })
}

fn clamp_ms(value: f64) -> u32 {
    value.max(0.0).min(u32::MAX as f64) as u32
}

/// Reads audio for a Codename song through the VFS.
///
/// Prefers `Inst.ogg` (case-insensitive) from the song's audio list, falling
/// back to any `.ogg` file, then to the first audio entry.
fn read_codename_audio(
    mod_root: &Path,
    song: &codename::Song,
) -> Result<Option<(String, Vec<u8>)>, SongError> {
    let candidates: Vec<&String> = song.audio.iter().map(|entry| &entry.path).collect();
    let chosen = candidates
        .iter()
        .find(|path| {
            path.rsplit('/')
                .next()
                .unwrap_or("")
                .eq_ignore_ascii_case("Inst.ogg")
        })
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

    let virtual_path = format!("/{}", path);
    let bytes = load_bytes(&vfs, &virtual_path)
        .map_err(|error| SongError::Unreadable(format!("audio '{virtual_path}': {error}")))?;

    Ok(Some((path.to_string(), bytes)))
}

/// Format-aware song loader.
///
/// Detects whether `mod_root` is a Codename Engine mod or a V-Slice mod and
/// dispatches to the appropriate loader. Falls back to trying V-Slice first
/// then Codename when neither marker is found.
pub fn load_song_auto(
    mod_root: &Path,
    song_id: &str,
    difficulty: Option<&str>,
) -> Result<PlayableSong, SongError> {
    use crate::song::load_song as load_vslice_song;

    if is_codename_mod(mod_root) {
        return load_codename_song(mod_root, song_id, difficulty);
    }
    if fu_import::vslice::is_vslice_directory(mod_root) {
        return load_vslice_song(mod_root, song_id, difficulty);
    }
    // Unknown format: try V-Slice first, then Codename.
    match load_vslice_song(mod_root, song_id, difficulty) {
        Ok(song) => Ok(song),
        Err(_) => load_codename_song(mod_root, song_id, difficulty),
    }
}
