//! Integration tests for chart runtime timing.
//!
//! These drive the real loader path (parsed Codename mod → `RuntimeChart` →
//! `Chart` → `Gameplay`) and check that gameplay judges against the chart's
//! tempo map rather than a single metadata BPM. Fixtures are written to a
//! temporary directory, so no installation is needed for the synthetic cases;
//! the real-asset test skips itself when Codename is not installed.

use std::path::PathBuf;

use fu_import::test_support::TempDir;
use fu_play::codename_song::load_codename_song;
use fu_play::game::{Gameplay, HitRating};
use fu_play::session::{simulate, Session};
use fu_play::song::load_song;

const CODENAME_ASSETS: &str = "D:/idk/Codename/assets";

fn write_codename_mod(temp: &TempDir) -> PathBuf {
    temp.write(
        "mod/data/config/modpack.ini",
        "[modpack]\nname=Timing Fixture\n",
    )
    .unwrap();
    temp.write(
        "mod/songs/timingsong/meta.json",
        r#"{
  "displayName": "Timing Song",
  "bpm": 100.0,
  "beatsPerMeasure": 4,
  "stepsPerBeat": 4,
  "difficulties": ["hard"]
}"#,
    )
    .unwrap();
    temp.write(
        "mod/songs/timingsong/charts/hard.json",
        r#"{
  "scrollSpeed": 1.4,
  "strumLines": [
    {
      "keyCount": 4,
      "position": "boyfriend",
      "characters": ["bf"],
      "notes": [
        { "time": 1000, "id": 0 },
        { "time": 4000, "id": 1 },
        { "time": 7000, "id": 2 }
      ]
    },
    {
      "keyCount": 4,
      "position": "dad",
      "characters": ["dad"],
      "notes": [{ "time": 1500, "id": 3 }]
    }
  ],
  "events": [
    { "time": 3000, "name": "BPM Change", "params": [200.0] },
    { "time": 6000, "name": "Time Signature Change", "params": [3.0, 4.0, false] }
  ]
}"#,
    )
    .unwrap();
    temp.join("mod")
}

#[test]
fn codename_timing_events_reach_the_gameplay_chart() {
    let temp = TempDir::new("timing-codename").unwrap();
    let song = load_codename_song(&write_codename_mod(&temp), "timingsong", None)
        .expect("timing song loads");

    assert_eq!(song.chart.bpm, 100.0, "starting tempo");
    assert_eq!(song.chart.notes.len(), 3, "opponent notes are excluded");
    assert_eq!(song.chart.notes[0].time_ms, 1000.0);
    assert_eq!(song.chart.notes[1].time_ms, 4000.0);
    assert_eq!(song.chart.notes[2].time_ms, 7000.0);

    let changes = song.chart.timing().changes();
    assert_eq!(changes.len(), 3, "{changes:?}");
    assert_eq!(changes[1].song_time, 3000.0);
    assert_eq!(changes[1].bpm, 200.0);
    assert_eq!(changes[2].beats_per_measure, 3.0);
}

#[test]
fn gameplay_follows_the_conductor_across_a_bpm_change() {
    let temp = TempDir::new("timing-conductor").unwrap();
    let song = load_codename_song(&write_codename_mod(&temp), "timingsong", None)
        .expect("timing song loads");
    let mut game = Gameplay::new(song.chart.clone());

    game.update(1000.0);
    let hit = game.press(0, 1000.0).expect("note at 1000 ms");
    assert_eq!(hit.rating, HitRating::Sick);

    game.update(2999.0);
    assert!((game.bpm() - 100.0).abs() < 1e-9, "before the change");
    assert_eq!(game.step(), 19);

    game.update(3000.0);
    assert!((game.bpm() - 200.0).abs() < 1e-9, "from the change on");
    assert_eq!(game.tick().bpm_change.map(|signal| signal.bpm), Some(200.0));
    assert_eq!(game.step(), 20, "200 bpm starts at step 20");

    game.update(4000.0);
    assert_eq!(game.step(), 33);
    assert_eq!(game.beat(), 8);

    game.update(6000.0);
    assert_eq!(
        game.tick()
            .time_signature_change
            .map(|signal| signal.beats_per_measure),
        Some(3.0)
    );
    assert_eq!(game.conductor().beats_per_measure(), 3.0);
    assert_eq!(game.conductor().measure_length(), 12.0);

    game.update(7000.0);
    assert_eq!(game.step(), 73);
    assert_eq!(game.beat(), 18);
    assert_eq!(game.measure(), 5);

    // Every step index the conductor reported equals the chart's own map.
    for time in [0.0, 1000.0, 3000.0, 4000.0, 6000.0, 7000.0] {
        game.update(time);
        assert_eq!(
            game.conductor().cur_step_float(),
            game.timing().steps_at_time(time as f64),
            "step at {time} ms"
        );
    }
}

#[test]
fn a_session_reports_the_same_timing_as_the_gameplay_core() {
    let temp = TempDir::new("timing-session").unwrap();
    let song = load_codename_song(&write_codename_mod(&temp), "timingsong", None)
        .expect("timing song loads");
    let mut session = Session::new(song.chart.clone());
    session.start();

    session.advance(3000.0);
    assert!((session.bpm() - 200.0).abs() < 1e-9);
    assert_eq!(session.step(), 20);
    assert_eq!(session.measure(), 1);
    assert!(session.tick().bpm_change.is_some());

    session.advance(6500.0);
    assert_eq!(session.measure(), 4);
    assert_eq!(session.timing().changes().len(), 3);
}

#[test]
fn a_multi_tempo_codename_song_plays_to_completion() {
    let temp = TempDir::new("timing-autoplay").unwrap();
    let song = load_codename_song(&write_codename_mod(&temp), "timingsong", None)
        .expect("timing song loads");

    let result = simulate(&song.chart, true);
    assert!(result.completed, "song runs to its end");
    assert_eq!(result.hits as usize, song.note_count());
    assert_eq!(result.misses, 0);
    assert_eq!(result.max_combo as usize, song.note_count());
}

#[test]
fn vslice_time_changes_reach_gameplay() {
    let temp = TempDir::new("timing-vslice").unwrap();
    temp.write(
        "mod/_polymod_meta.json",
        r#"{"title": "Tempo Fixture", "api_version": "0.5.0"}"#,
    )
    .unwrap();
    temp.write(
        "mod/songs/tempo/tempo-metadata.json",
        r#"{
  "version": "2.2.3",
  "songName": "Tempo",
  "timeChanges": [
    {"t": 0, "bpm": 120},
    {"t": 2000, "bpm": 240}
  ],
  "playData": {"difficulties": ["hard"]}
}"#,
    )
    .unwrap();
    temp.write(
        "mod/songs/tempo/tempo-chart.json",
        r#"{
  "version": "2.0.0",
  "notes": {"hard": [{"t": 500, "d": 0}, {"t": 2500, "d": 1}]}
}"#,
    )
    .unwrap();

    let song = load_song(&temp.join("mod"), "tempo", None).expect("song loads");
    assert_eq!(song.chart.bpm, 120.0);
    assert_eq!(song.chart.notes[0].time_ms, 500.0);
    assert_eq!(song.chart.notes[1].time_ms, 2500.0);
    let changes = song.chart.timing().changes();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes[1].song_time, 2000.0);
    assert_eq!(changes[1].bpm, 240.0);

    let mut game = Gameplay::new(song.chart.clone());
    game.update(1999.0);
    assert!((game.bpm() - 120.0).abs() < 1e-9);
    game.update(2500.0);
    assert!((game.bpm() - 240.0).abs() < 1e-9);
    assert_eq!(game.step(), 24, "16 steps at 120 bpm, then 8 at 240");

    let result = simulate(&song.chart, true);
    assert!(result.completed);
    assert_eq!(result.misses, 0);
}

#[test]
fn a_real_codename_song_keeps_its_tempo_and_plays_headlessly() {
    if !std::path::Path::new(CODENAME_ASSETS).is_dir() {
        eprintln!("skipping: no Codename installation at {CODENAME_ASSETS}");
        return;
    }
    let song = load_codename_song(
        std::path::Path::new(CODENAME_ASSETS),
        "bopeebo",
        Some("hard"),
    )
    .expect("bopeebo loads");

    let changes = song.chart.timing().changes();
    assert_eq!(changes.len(), 1, "shipped songs declare a single tempo");
    assert_eq!(changes[0].song_time, 0.0);
    assert_eq!(song.chart.bpm, changes[0].bpm as f32);
    assert!(song.note_count() > 10);

    let mut game = Gameplay::new(song.chart.clone());
    game.update(2000.0);
    assert_eq!(
        game.conductor().cur_step_float(),
        song.chart.timing().steps_at_time(2000.0)
    );

    let result = simulate(&song.chart, true);
    assert!(result.completed, "bopeebo runs to its end");
    assert_eq!(result.hits as usize, song.note_count());
    assert_eq!(result.misses, 0);
    println!(
        "bopeebo: {} notes, {:.2} bpm, {} steps at 2000 ms",
        song.note_count(),
        song.chart.bpm,
        game.step()
    );
}
