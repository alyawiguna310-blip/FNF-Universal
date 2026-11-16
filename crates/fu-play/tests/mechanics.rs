//! Integration tests for the P3 gameplay mechanics on Codename charts.
//!
//! The synthetic fixture is a six-lane chart with sustains, two opponent lines,
//! a BPM change, a time-signature change and a non-timing event — the shapes
//! Codename supports but the shipped songs do not all exercise. The real-asset
//! test checks the same path against the installed game.

use std::path::{Path, PathBuf};

use fu_import::test_support::TempDir;
use fu_play::codename_song::load_codename_song;
use fu_play::game::{Gameplay, HoldState, StrumSlot};
use fu_play::session::simulate;
use fu_timing::{HitRating, WindowPreset};

const CODENAME_ASSETS: &str = "D:/idk/Codename/assets";

fn write_mechanics_mod(temp: &TempDir) -> PathBuf {
    temp.write(
        "mod/data/config/modpack.ini",
        "[modpack]\nname=Mechanics Fixture\n",
    )
    .unwrap();
    temp.write(
        "mod/songs/mechanics/meta.json",
        r#"{
  "displayName": "Mechanics",
  "bpm": 100.0,
  "beatsPerMeasure": 4,
  "stepsPerBeat": 4,
  "difficulties": ["hard"]
}"#,
    )
    .unwrap();
    temp.write(
        "mod/songs/mechanics/charts/hard.json",
        r#"{
  "scrollSpeed": 1.5,
  "strumLines": [
    {
      "keyCount": 6,
      "position": "boyfriend",
      "characters": ["bf"],
      "notes": [
        { "time": 1000, "id": 5, "sLen": 500 },
        { "time": 2000, "id": 1, "sLen": 3000 },
        { "time": 7000, "id": 2, "sLen": 0 }
      ]
    },
    {
      "keyCount": 4,
      "position": "dad",
      "characters": ["dad"],
      "notes": [
        { "time": 500, "id": 0 },
        { "time": 1500, "id": 3 }
      ]
    },
    {
      "keyCount": 4,
      "position": "gf",
      "characters": ["gf"],
      "notes": [{ "time": 900, "id": 2 }]
    }
  ],
  "events": [
    { "time": 3000, "name": "BPM Change", "params": [200.0] },
    { "time": 3500, "name": "Zoom Camera", "params": [1.1] },
    { "time": 6000, "name": "Time Signature Change", "params": [3.0, 4.0, false] }
  ]
}"#,
    )
    .unwrap();
    temp.join("mod")
}

fn load_mechanics() -> (TempDir, fu_play::song::PlayableSong) {
    let temp = TempDir::new("fu-play-mechanics").unwrap();
    let song = load_codename_song(&write_mechanics_mod(&temp), "mechanics", None)
        .expect("mechanics song loads");
    (temp, song)
}

#[test]
fn a_six_lane_chart_reaches_gameplay_without_four_lane_assumptions() {
    let (_temp, song) = load_mechanics();
    assert_eq!(song.chart.lanes, 6);
    assert_eq!(song.chart.notes.len(), 3);
    assert_eq!(
        song.chart
            .notes
            .iter()
            .filter(|note| note.sustain_ms > 0.0)
            .count(),
        2
    );

    let mut game = Gameplay::new(song.chart.clone());
    assert_eq!(game.lanes(), 6);
    game.update(1_000.0);
    let hit = game.press(5, 1_000.0).expect("lane 5 is playable");
    assert_eq!(hit.rating, HitRating::Sick);
    assert_eq!(hit.sustain_ms, 500.0);
    assert!(game.press(6, 1_000.0).is_none(), "lane 6 does not exist");
}

#[test]
fn a_codename_sustain_spans_a_bpm_change_and_keeps_its_end() {
    let (_temp, song) = load_mechanics();
    let mut game = Gameplay::new(song.chart.clone());
    game.update(2_000.0);
    let hit = game.press(1, 2_000.0).expect("head hit");
    assert_eq!(hit.sustain_ms, 3_000.0);

    game.update(5_000.0);
    let hold = game.holds()[0];
    assert_eq!(
        hold.end_ms, 5_000.0,
        "end time is chart time, not step time"
    );
    assert_eq!(hold.state, HoldState::Completed);
    assert!(hold.held_steps > 20, "held {} steps", hold.held_steps);
    assert!((game.bpm() - 200.0).abs() < 1e-9);

    // The sustain crossed the 3000 ms tempo change; its steps came from the
    // same conductor the judgement used.
    let expected = {
        let timing = game.timing();
        let mut count = 0;
        for step in 1..=timing.steps_at_time(5_000.0) as i64 {
            let time = timing.time_at_steps(step as f64);
            if time > 2_000.0 && time <= 5_000.0 {
                count += 1;
            }
        }
        count
    };
    assert_eq!(hold.held_steps as i64, expected);
}

#[test]
fn codename_chart_events_are_dispatched_on_their_step() {
    let (_temp, song) = load_mechanics();
    let chart = &song.chart;

    let bpm_event = chart
        .events
        .iter()
        .find(|event| event.name == "BPM Change")
        .expect("BPM change event");
    assert_eq!(bpm_event.step, 20, "200 bpm starts at step 20");

    let zoom = chart
        .events
        .iter()
        .find(|event| event.name == "Zoom Camera")
        .expect("non-timing event");
    assert_eq!(zoom.step, 26, "3500 ms is 26 steps at 200 bpm");

    let mut game = Gameplay::new(chart.clone());
    game.update(3_500.0);
    let fired: Vec<&str> = game
        .fired_events()
        .iter()
        .map(|index| chart.events[*index].name.as_str())
        .collect();
    assert_eq!(fired, vec!["BPM Change", "Zoom Camera"]);
    assert!(game.tick().steps.contains(&20));
    assert!(!game.tick().beats.is_empty() || !game.tick().steps.is_empty());
}

#[test]
fn opponent_and_girlfriend_lines_are_simulated_separately() {
    let (_temp, song) = load_mechanics();
    assert_eq!(song.chart.strum_lines.len(), 2);

    let mut game = Gameplay::new(song.chart.clone());
    game.update(2_000.0);
    let hits = game.opponent_hits();
    assert_eq!(hits.len(), 3);
    assert!(hits.iter().any(|hit| hit.slot == StrumSlot::Opponent));
    assert!(hits.iter().any(|hit| hit.slot == StrumSlot::Girlfriend));
    assert_eq!(game.hits(), 0, "opponent notes are never player hits");
    assert_eq!(game.score(), 0);
}

#[test]
fn the_loaded_chart_names_its_preset_explicitly() {
    let (_temp, song) = load_mechanics();
    assert_eq!(
        song.chart.preset,
        WindowPreset::Default,
        "Codename RatingManager's default preset"
    );
    assert_eq!(song.chart.windows().sick, 37.8);
    assert_eq!(song.chart.windows().shit, 180.0);

    let cne = song.chart.clone().with_preset(WindowPreset::CneClassic);
    assert_eq!(cne.windows().sick, 50.0);
    assert_eq!(cne.windows().shit, 250.0);

    // The choice is visible in gameplay, not silently baked in.
    let mut game = Gameplay::new(song.chart.clone());
    game.update(1_000.0);
    assert_eq!(game.windows().shit, 180.0);
    game.press(5, 1_000.0).unwrap();
    let mut cne_game = Gameplay::new(cne);
    cne_game.update(1_000.0);
    assert_eq!(cne_game.press(5, 1_240.0).unwrap().rating, HitRating::Shit);
}

#[test]
fn a_real_codename_chart_uses_opponent_lines_and_plays_headlessly() {
    if !Path::new(CODENAME_ASSETS).is_dir() {
        eprintln!("skipping: no Codename installation at {CODENAME_ASSETS}");
        return;
    }
    let song = load_codename_song(Path::new(CODENAME_ASSETS), "bopeebo", Some("hard"))
        .expect("bopeebo loads");

    assert_eq!(song.chart.lanes, 4, "bopeebo is a four-key chart");
    assert!(
        !song.chart.strum_lines.is_empty(),
        "the opponent lines are kept for simulation"
    );
    let opponent_notes: usize = song
        .chart
        .strum_lines
        .iter()
        .map(|line| line.notes.len())
        .sum();
    assert!(opponent_notes > 10);

    let mut game = Gameplay::new(song.chart.clone());
    game.update(2_000.0);
    assert!(game.opponent().total_hits() > 0);
    assert!(game.opponent().note_count() > 10);
    assert_eq!(game.hits(), 0);

    let result = simulate(&song.chart, true);
    assert!(result.completed, "bopeebo runs to its end");
    assert_eq!(result.hits as usize, song.note_count());
    assert_eq!(result.misses, 0);
}
