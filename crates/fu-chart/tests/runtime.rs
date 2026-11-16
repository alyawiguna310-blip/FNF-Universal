//! Tests for the unified runtime chart model.
//!
//! The Codename side is exercised with the timing fixture mod that ships with
//! `fu-timing` (a chart with a BPM change and a time-signature change) and,
//! when the local Codename installation is present, with real shipped charts.
//! The V-Slice side is built in memory so no filesystem fixture is needed.

use std::collections::BTreeMap;
use std::path::Path;

use fu_chart::{vslice_timing, RuntimeChart, StrumPosition};
use fu_import::{codename, vslice};

const FIXTURE_MOD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fu-timing/tests/fixtures/mod"
);
const CODENAME_ASSETS: &str = "D:/idk/Codename/assets";

fn fixture_song() -> codename::Song {
    let discovered = codename::open_mod_dir(Path::new(FIXTURE_MOD)).expect("fixture mod opens");
    let parsed = codename::parse_mod(&discovered);
    parsed.module.song("timing").cloned().expect("timing song")
}

#[test]
fn codename_fixture_projects_its_timing_into_the_runtime_chart() {
    let song = fixture_song();
    let runtime = RuntimeChart::from_codename_song(&song, "hard").expect("hard chart");

    assert_eq!(runtime.song, "Timing Fixture");
    assert_eq!(runtime.difficulty, "hard");
    assert_eq!(runtime.scroll_speed, 1.6);
    assert_eq!(runtime.strum_lines.len(), 1);
    assert!(runtime.strum_lines[0].is_player());
    assert_eq!(runtime.lanes, 4);
    assert_eq!(runtime.note_count(), 2);
    assert_eq!(runtime.length_ms, 2000.0);

    // One map: initial tempo, the BPM change, then the time signature.
    let changes = runtime.timing.changes();
    assert_eq!(changes.len(), 3, "100 bpm, 200 bpm, 3/4: {changes:?}");
    assert_eq!(changes[0].bpm, 100.0);
    assert_eq!(changes[0].beats_per_measure, 4.0);
    assert_eq!(changes[1].song_time, 3000.0);
    assert_eq!(changes[1].bpm, 200.0);
    assert_eq!(changes[2].song_time, 6000.0);
    assert_eq!(changes[2].bpm, 200.0);
    assert_eq!(changes[2].beats_per_measure, 3.0);
    assert_eq!(changes[2].steps_per_beat, 4);

    // Conversions are the conductor's, not re-derived here.
    assert!((runtime.bpm_at(2999.0) - 100.0).abs() < 1e-9);
    assert!((runtime.bpm_at(3000.0) - 200.0).abs() < 1e-9);
    assert!((runtime.bpm_at(9000.0) - 200.0).abs() < 1e-9);
    assert!((runtime.step_at(4500.0) - 40.0).abs() < 1e-9);
    assert!((runtime.beat_at(4500.0) - 10.0).abs() < 1e-9);
    assert!((runtime.measure_at(6000.0) - 4.0).abs() < 1e-9);
    assert!((runtime.time_at_step(40.0) - 4500.0).abs() < 1e-9);

    // Note times and steps come from the same map.
    let first = runtime.notes[0];
    assert_eq!(first.time_ms, 1000.0);
    assert_eq!(first.lane, 0);
    assert_eq!(first.sustain_ms, 0.0);
    assert!((first.step - 1000.0 / 150.0).abs() < 1e-12);

    let second = runtime.notes[1];
    assert_eq!(second.time_ms, 2000.0);
    assert_eq!(second.lane, 3);
    assert_eq!(second.sustain_ms, 500.0);
    assert!((second.step - 2000.0 / 150.0).abs() < 1e-12);

    for note in &runtime.notes {
        assert_eq!(note.step, runtime.timing.steps_at_time(note.time_ms));
    }

    // Events stay in song order, parameters untouched.
    assert_eq!(runtime.events.len(), 2);
    assert_eq!(runtime.events[0].name, "BPM Change");
    assert_eq!(runtime.events[0].params[0].as_f64(), Some(200.0));
    assert_eq!(runtime.events[1].name, "Time Signature Change");

    assert!(runtime.timing.rejected().is_empty());
}

#[test]
fn a_runtime_chart_and_its_conductor_agree_on_steps_and_beats() {
    let song = fixture_song();
    let runtime = RuntimeChart::from_codename_song(&song, "hard").expect("hard chart");
    let mut conductor = runtime.conductor();

    for time in [0.0, 750.0, 3000.0, 4500.0, 6000.0, 6500.0, 8000.0] {
        conductor.update(time);
        assert!(
            (conductor.cur_step_float() - runtime.step_at(time)).abs() < 1e-9,
            "step at {time} ms"
        );
        assert!(
            (conductor.cur_beat_float() - runtime.beat_at(time)).abs() < 1e-9,
            "beat at {time} ms"
        );
        assert!(
            (conductor.cur_measure_float() - runtime.measure_at(time)).abs() < 1e-9,
            "measure at {time} ms"
        );
    }
}

#[test]
fn vslice_time_changes_become_one_tempo_map() {
    let metadata = vslice::SongMetadata {
        song_name: Some("Tempo".to_string()),
        time_changes: vec![
            vslice::TimeChange {
                t: Some(0.0),
                bpm: Some(120.0),
                ..Default::default()
            },
            vslice::TimeChange {
                t: Some(4000.0),
                bpm: Some(150.0),
                ..Default::default()
            },
            vslice::TimeChange {
                t: Some(8000.0),
                bpm: Some(150.0),
                n: Some(3.0),
                d: Some(4.0),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let timing = vslice_timing(Some(&metadata));
    let changes = timing.changes();
    assert_eq!(changes.len(), 3, "{changes:?}");
    assert_eq!(changes[0].bpm, 120.0);
    assert_eq!(changes[1].song_time, 4000.0);
    assert_eq!(changes[1].bpm, 150.0);
    assert_eq!(changes[2].song_time, 8000.0);
    assert_eq!(changes[2].beats_per_measure, 3.0);
    assert_eq!(changes[2].steps_per_beat, 4);
    assert!((timing.bpm_at_time(3999.0) - 120.0).abs() < 1e-9);
    assert!((timing.bpm_at_time(4000.0) - 150.0).abs() < 1e-9);
}

#[test]
fn vslice_notes_and_events_project_without_rounding() {
    let mut notes = BTreeMap::new();
    notes.insert(
        "hard".to_string(),
        vec![
            vslice::Note {
                index: 0,
                time_ms: Some(1000.5),
                direction: Some(0.0),
                sustain_ms: Some(250.0),
                ..Default::default()
            },
            vslice::Note {
                index: 1,
                time_ms: Some(1500.0),
                direction: Some(5.0),
                ..Default::default()
            },
        ],
    );
    let chart = vslice::Chart {
        source: "songs/x/x-chart.json".to_string(),
        notes,
        events: vec![vslice::ChartEvent {
            time_ms: Some(500.0),
            name: Some("Zoom Camera".to_string()),
            value: Some(serde_json::json!({"zoom": 1.1})),
            ..Default::default()
        }],
        ..Default::default()
    };
    let metadata = vslice::SongMetadata {
        song_name: Some("V-Slice Timing".to_string()),
        time_changes: vec![vslice::TimeChange {
            t: Some(0.0),
            bpm: Some(120.0),
            ..Default::default()
        }],
        ..Default::default()
    };

    let runtime =
        RuntimeChart::from_vslice_chart(&chart, Some(&metadata), "hard").expect("hard chart");

    assert_eq!(runtime.song, "V-Slice Timing");
    assert_eq!(runtime.note_count(), 1, "the opponent note is not player");
    assert_eq!(
        runtime.notes[0].time_ms, 1000.5,
        "sub-millisecond precision"
    );
    assert_eq!(runtime.notes[0].sustain_ms, 250.0);
    assert_eq!(runtime.notes[0].lane, 0);
    assert_eq!(runtime.strum_lines.len(), 2);
    assert_eq!(runtime.strum_lines[0].position, StrumPosition::Player);
    assert_eq!(runtime.strum_lines[1].position, StrumPosition::Opponent);
    assert_eq!(runtime.strum_lines[1].notes.len(), 1);
    assert_eq!(runtime.strum_lines[1].notes[0].lane, 1);
    assert_eq!(runtime.events.len(), 1);
    assert_eq!(runtime.events[0].name, "Zoom Camera");
    assert!((runtime.bpm_at(0.0) - 120.0).abs() < 1e-9);
    assert!((runtime.note_count() as f64) <= runtime.notes.len() as f64);
}

#[test]
fn real_codename_charts_keep_their_authored_note_times() {
    if !Path::new(CODENAME_ASSETS).is_dir() {
        eprintln!("skipping: no Codename installation at {CODENAME_ASSETS}");
        return;
    }
    let discovered = codename::open_mod_dir(Path::new(CODENAME_ASSETS)).expect("assets open");
    let parsed = codename::parse_mod(&discovered);

    for id in ["bopeebo", "dadbattle", "cocoa"] {
        let Some(song) = parsed.module.song(id) else {
            eprintln!("skipping {id}: not installed");
            continue;
        };
        let Some(chart) = song.charts.first() else {
            continue;
        };
        let runtime =
            RuntimeChart::from_codename_chart(chart, song.events.as_ref(), song.metadata.as_ref());

        let mut authored: Vec<(f64, u8)> = chart
            .strum_lines
            .iter()
            .filter(|line| {
                StrumPosition::from_name(line.position.as_deref()) == StrumPosition::Player
            })
            .flat_map(|line| {
                let key_count = line.key_count.unwrap_or(4);
                line.notes.iter().filter_map(move |note| {
                    let time = note.time?;
                    let lane = note.id?;
                    (lane < key_count).then_some((time, lane as u8))
                })
            })
            .collect();
        authored.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));

        assert_eq!(
            runtime.notes.len(),
            authored.len(),
            "{id}: every player note reaches the runtime chart"
        );
        for (note, (time, lane)) in runtime.notes.iter().zip(&authored) {
            assert_eq!(note.time_ms, *time, "{id}: note time is not rounded");
            assert_eq!(note.lane, *lane, "{id}: lane");
            assert_eq!(
                note.step,
                runtime.timing.steps_at_time(note.time_ms),
                "{id}: step follows the chart's map"
            );
        }
        assert!(
            !runtime.timing.changes().is_empty(),
            "{id}: a map always has an initial change"
        );
        println!(
            "{id}: {} notes, {} timing section(s), {:.2} bpm",
            runtime.notes.len(),
            runtime.timing.changes().len(),
            runtime.timing.changes()[0].bpm
        );
    }
}
