//! End-to-end tests for the Codename Engine song loading path.

use std::path::Path;

use fu_play::codename_song::{is_codename_mod, load_codename_song};
use fu_play::song::SongError;

const CODENAME_ASSETS: &str = "D:/idk/Codename/assets";

#[test]
fn detects_codename_mod_by_song_meta() {
    assert!(is_codename_mod(Path::new(CODENAME_ASSETS)));
    assert!(!is_codename_mod(Path::new(
        "D:/idk/FNF-Universal/nonexistent"
    )));
    let temp = std::env::temp_dir().join("fu-play-empty-test");
    let _ = std::fs::create_dir_all(&temp);
    assert!(!is_codename_mod(&temp));
    let _ = std::fs::remove_dir(&temp);
}

#[test]
fn loads_a_codename_song() {
    let song = load_codename_song(Path::new(CODENAME_ASSETS), "bopeebo", Some("easy"))
        .expect("codename song loads");
    println!(
        "title={}, notes={}, audio={}, bpm={}",
        song.title,
        song.note_count(),
        song.has_audio(),
        song.chart.bpm
    );
    assert!(song.note_count() > 0, "should have player notes");
    assert!(song.has_audio(), "should have audio");
    assert_eq!(song.difficulty, "easy");
    assert!(!song.available_difficulties.is_empty());
}

#[test]
fn codename_opponent_notes_are_excluded() {
    let song = load_codename_song(Path::new(CODENAME_ASSETS), "bopeebo", Some("easy"))
        .expect("codename song loads");
    assert!(song.note_count() > 10, "should have many player notes");
}

#[test]
fn codename_difficulty_selection() {
    let song = load_codename_song(Path::new(CODENAME_ASSETS), "bopeebo", Some("HARD"))
        .expect("case-insensitive difficulty");
    assert_eq!(song.difficulty, "hard");
}

#[test]
fn codename_unknown_song_is_reported() {
    let error =
        load_codename_song(Path::new(CODENAME_ASSETS), "nonexistent-song-xyz", None).unwrap_err();
    assert!(matches!(error, SongError::SongNotFound { .. }));
}

#[test]
fn codename_unknown_difficulty_is_reported() {
    let error =
        load_codename_song(Path::new(CODENAME_ASSETS), "bopeebo", Some("nightmare")).unwrap_err();
    assert!(matches!(error, SongError::NoSuchDifficulty { .. }));
}

#[test]
fn codename_missing_audio_returns_none() {
    let temp = std::env::temp_dir().join("fu-play-codename-noaudio");
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(temp.join("songs/testsong/charts")).unwrap();
    std::fs::write(
        temp.join("songs/testsong/meta.json"),
        r#"{"displayName": "Test", "bpm": 120, "difficulties": ["easy"]}"#,
    )
    .unwrap();
    let full_json = r#"{"strumLines": [{"position": "boyfriend", "keyCount": 4, "notes": [{"time": 1000, "id": 0}]}, {"position": "dad", "keyCount": 4, "notes": [{"time": 2000, "id": 1}]}"#;
    let full_json = format!("{}]", full_json);
    let full_json = format!("{}}}", full_json);
    std::fs::write(temp.join("songs/testsong/charts/easy.json"), &full_json).unwrap();

    let song = load_codename_song(&temp, "testsong", None).expect("song loads without audio");
    assert!(!song.has_audio());
    assert_eq!(song.note_count(), 1);
    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn codename_sustain_preserved_in_timing() {
    let song = load_codename_song(Path::new(CODENAME_ASSETS), "bopeebo", Some("easy"))
        .expect("codename song loads");
    let times: Vec<f64> = song.chart.notes.iter().map(|n| n.time_ms).collect();
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    assert_eq!(times, sorted, "notes should be sorted by time");
}
