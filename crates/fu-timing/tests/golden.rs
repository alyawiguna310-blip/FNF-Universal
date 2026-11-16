//! Golden tests for the timing port.
//!
//! Expected numbers here are derived by hand from the Haxe formulas (closed
//! forms for constant tempo and single changes) and, where a formula is not
//! closed, from invariants the formula must satisfy. The independent
//! JavaScript transcription in `tests/reference/` covers the wider space; see
//! `tests/parity.rs`.

use std::path::Path;

use fu_import::codename::{open_mod_dir, parse_mod};
use fu_import::test_support::TempDir;

use fu_timing::bpm::{timing_events_from_chart_events, TimingEvent, TimingEventKind};
use fu_timing::windows::{HitRating, HitWindows, WindowPreset, JUDGE_SCALES};
use fu_timing::{BpmMap, Conductor};

const FIXTURE_MOD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mod");
const CODENAME_ASSETS: &str = "D:/idk/Codename/assets";

fn close(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= 1e-9 * scale
}

fn assert_close(label: &str, actual: f64, expected: f64) {
    assert!(
        close(actual, expected),
        "{label}: {actual} != {expected} (delta {})",
        (actual - expected).abs()
    );
}

fn bpm_event(time: f64, bpm: f64) -> TimingEvent {
    TimingEvent {
        time,
        kind: TimingEventKind::BpmChange { bpm },
    }
}

fn timesig_event(time: f64, numerator: f64, denominator: f64, steps: bool) -> TimingEvent {
    TimingEvent {
        time,
        kind: TimingEventKind::TimeSignatureChange {
            numerator,
            denominator,
            denominator_is_steps_per_beat: steps,
        },
    }
}

fn continuous_event(time: f64, bpm: f64, steps: f64) -> TimingEvent {
    TimingEvent {
        time,
        kind: TimingEventKind::ContinuousBpmChange { bpm, steps },
    }
}

// --- constant tempo ---------------------------------------------------------

#[test]
fn constant_tempo_uses_the_codename_crochets() {
    let map = BpmMap::new(100.0, 4.0, 4);
    let mut conductor = Conductor::new(map.clone());
    conductor.update(0.0);

    // 15000 / bpm = ms per step; 15000 * stepsPerBeat / bpm = ms per beat.
    assert_close("step crochet", conductor.step_crochet(), 150.0);
    assert_close("crochet", conductor.crochet(), 600.0);
    assert_close("measure length", conductor.measure_length(), 16.0);

    assert_close("steps at 1500 ms", map.steps_at_time(1500.0), 10.0);
    assert_close("beats at 1500 ms", map.beats_at_time(1500.0), 2.5);
    assert_close("time at step 10", map.time_at_steps(10.0), 1500.0);
    assert_close("time at beat 4", map.time_at_beats(4.0), 2400.0);
    assert_close("time at measure 1", map.time_at_measures(1.0), 2400.0);
}

#[test]
fn conductor_dispatches_each_crossed_step_and_beat_in_order() {
    let mut conductor = Conductor::new(BpmMap::new(100.0, 4.0, 4));

    let tick = conductor.update(0.0);
    assert!(tick.is_empty(), "t=0 fires nothing");

    let tick = conductor.update(600.0);
    assert_eq!(tick.steps, vec![1, 2, 3, 4], "four steps in the first beat");
    assert_eq!(tick.beats, vec![1]);
    assert!(tick.measures.is_empty());

    let tick = conductor.update(2400.0);
    assert_eq!(tick.steps, (5..=16).collect::<Vec<_>>());
    assert_eq!(tick.beats, vec![2, 3, 4]);
    assert_eq!(tick.measures, vec![1]);

    // A backwards seek reports the single index it landed on, like Haxe.
    let tick = conductor.update(0.0);
    assert_eq!(tick.steps, vec![0]);
    assert_eq!(tick.beats, vec![0]);
    assert_eq!(tick.measures, vec![0]);
}

// --- BPM changes ------------------------------------------------------------

#[test]
fn a_bpm_change_advances_the_change_map_exactly() {
    let map = BpmMap::from_timing_events(100.0, 4.0, 4, &[bpm_event(3000.0, 200.0)]);
    assert_eq!(map.changes().len(), 2);

    let change = &map.changes()[1];
    assert_close("song time", change.song_time, 3000.0);
    assert_eq!(change.bpm, 200.0);
    // 3000 ms at 100 bpm = 3000 / 150 = 20 steps; 20/4 = 5 beats; 5/4 = 1.25 measures.
    assert_close("step time", change.step_time, 20.0);
    assert_close("beat time", change.beat_time, 5.0);
    assert_close("measure time", change.measure_time, 1.25);

    // 1500 ms after the change at 200 bpm = 20 more steps.
    assert_close("steps at 4500 ms", map.steps_at_time(4500.0), 40.0);
    assert_close("beats at 4500 ms", map.beats_at_time(4500.0), 10.0);
    assert_close("measures at 4500 ms", map.measures_at_time(4500.0), 2.5);
    assert_close("time at step 40", map.time_at_steps(40.0), 4500.0);
    assert_close("bpm at 4500 ms", map.bpm_at_time(4500.0), 200.0);
    assert_close("bpm before change", map.bpm_at_time(2999.0), 100.0);
}

#[test]
fn multiple_bpm_changes_keep_the_map_ordered() {
    let map = BpmMap::from_timing_events(
        100.0,
        4.0,
        4,
        &[bpm_event(3000.0, 200.0), bpm_event(9000.0, 150.0)],
    );
    assert_eq!(map.changes().len(), 3);
    assert_eq!(map.changes()[2].bpm, 150.0);
    // 6000 ms at 200 bpm = 80 steps, on top of the 20 already elapsed.
    assert_close("step time", map.changes()[2].step_time, 100.0);
    assert_close("beat time", map.changes()[2].beat_time, 25.0);
    assert_close("steps at 9750 ms", map.steps_at_time(9750.0), 107.5);
}

// --- time signatures --------------------------------------------------------

#[test]
fn a_time_signature_change_rebases_and_floors_the_counters() {
    let map = BpmMap::from_timing_events(100.0, 4.0, 4, &[timesig_event(6000.0, 3.0, 4.0, false)]);
    assert_eq!(map.changes().len(), 2);
    let change = &map.changes()[1];

    assert_eq!(change.beats_per_measure, 3.0);
    assert_eq!(change.steps_per_beat, 4, "16 / denominator = 4");
    // 6000 ms at 100 bpm = 40 steps, 10 beats, 2.5 measures, then the Haxe
    // rounding adds 0.99998 and floors: 40, 10, 3.
    assert_close("step time", change.step_time, 40.0);
    assert_close("beat time", change.beat_time, 10.0);
    assert_close("measure time", change.measure_time, 3.0);
    assert_close("measure length", map.measure_length(), 12.0);
}

#[test]
fn time_signature_derives_steps_from_the_denominator_or_takes_steps_directly() {
    let eighth =
        BpmMap::from_timing_events(120.0, 4.0, 4, &[timesig_event(2000.0, 7.0, 8.0, false)]);
    assert_eq!(eighth.changes()[1].steps_per_beat, 2, "16 / 8");
    assert_eq!(eighth.changes()[1].beats_per_measure, 7.0);

    let direct =
        BpmMap::from_timing_events(120.0, 4.0, 4, &[timesig_event(2000.0, 5.0, 6.0, true)]);
    assert_eq!(
        direct.changes()[1].steps_per_beat,
        6,
        "denominator is steps"
    );

    // 3/4 signature: floor(16/3) = 5, matching `Math.floor(16 / params[1])`.
    let third =
        BpmMap::from_timing_events(120.0, 4.0, 4, &[timesig_event(2000.0, 4.0, 3.0, false)]);
    assert_eq!(third.changes()[1].steps_per_beat, 5);
}

// --- continuous BPM ---------------------------------------------------------

#[test]
fn a_continuous_change_uses_codenames_integral_formula() {
    let map = BpmMap::from_timing_events(100.0, 4.0, 4, &[continuous_event(0.0, 200.0, 8.0)]);
    let change = &map.changes()[1];
    assert!(change.continuous);

    // endSongTime = t + steps/(bpm1-bpm0) * ln(bpm1/bpm0) * 15000
    let expected_end = 8.0 / (200.0 - 100.0) * (200.0f64 / 100.0).ln() * 15_000.0;
    assert_close("end song time", change.end_song_time, expected_end);
    assert_close("end step time", change.end_step_time, 8.0);

    // The tempo interpolates geometrically, so the midpoint is sqrt(100*200).
    let midpoint = expected_end / 2.0;
    assert_close(
        "midpoint bpm",
        map.bpm_at_time(midpoint),
        20_000.0f64.sqrt(),
    );

    // At the end of the ramp the step counter reaches exactly endStepTime.
    assert_close("steps at ramp end", map.steps_at_time(expected_end), 8.0);
    assert_close("bpm at ramp end", map.bpm_at_time(expected_end), 200.0);
    // Past the ramp the new tempo is flat.
    assert_close(
        "bpm past ramp",
        map.bpm_at_time(expected_end + 1000.0),
        200.0,
    );
}

#[test]
fn a_continuous_change_absorbs_the_remaining_map_time() {
    // A change after a completed ramp must start counting from endStepTime and
    // endSongTime, not from the ramp's start.
    let map = BpmMap::from_timing_events(
        100.0,
        4.0,
        4,
        &[continuous_event(0.0, 200.0, 8.0), bpm_event(5000.0, 120.0)],
    );
    let ramp = &map.changes()[1];
    let after = &map.changes()[2];

    let expected_step = 8.0 + (5000.0 - ramp.end_song_time) / (15_000.0 / 200.0);
    assert_close("step time after ramp", after.step_time, expected_step);
    assert_close("steps at 5000 ms", map.steps_at_time(5000.0), expected_step);
}

#[test]
fn events_inside_a_continuous_ramp_are_rejected() {
    let map = BpmMap::from_timing_events(
        120.0,
        4.0,
        4,
        &[
            continuous_event(1000.0, 240.0, 8.0),
            bpm_event(1100.0, 90.0),
        ],
    );
    assert_eq!(map.rejected().len(), 1);
    assert_eq!(
        map.rejected()[0].reason,
        fu_timing::RejectReason::IntersectsContinuousChange
    );
    assert_eq!(map.changes().len(), 2, "the rejected change is not mapped");

    let same = BpmMap::from_timing_events(120.0, 4.0, 4, &[continuous_event(1000.0, 120.0, 8.0)]);
    assert_eq!(
        same.rejected()[0].reason,
        fu_timing::RejectReason::ContinuousChangeToSameBpm
    );
}

#[test]
fn repeated_bpm_values_and_unsorted_events_are_handled() {
    let map = BpmMap::from_timing_events(
        100.0,
        4.0,
        4,
        &[
            bpm_event(4000.0, 150.0),
            bpm_event(1000.0, 120.0),
            bpm_event(1000.0, 120.0), // duplicate tempo: not a new change
        ],
    );
    assert_eq!(map.changes().len(), 3, "duplicate tempo is skipped");
    assert_eq!(map.changes()[1].bpm, 120.0);
    assert_eq!(map.changes()[2].bpm, 150.0);
    assert!(map.changes()[1].song_time < map.changes()[2].song_time);
}

// --- fu-import integration --------------------------------------------------

#[test]
fn builds_the_map_from_the_existing_importer_model() {
    let temp = TempDir::new("fu-timing-import").unwrap();
    let fixture_chart =
        std::fs::read_to_string(format!("{FIXTURE_MOD}/songs/timing/charts/hard.json"))
            .expect("fixture chart");
    let fixture_meta = std::fs::read_to_string(format!("{FIXTURE_MOD}/songs/timing/meta.json"))
        .expect("fixture meta");
    temp.write("mod/songs/timing/charts/hard.json", &fixture_chart)
        .unwrap();
    temp.write("mod/songs/timing/meta.json", &fixture_meta)
        .unwrap();
    temp.write("mod/data/config/modpack.ini", "[Common]\nAPI_VERSION=3\n")
        .unwrap();

    let discovered = open_mod_dir(&temp.join("mod")).unwrap();
    let parsed = parse_mod(&discovered);
    let song = parsed.module.song("timing").expect("fixture song");
    let chart = song.chart("hard").expect("fixture chart");
    assert_eq!(
        chart.strum_lines[0].notes.len(),
        2,
        "the importer parsed the chart"
    );

    let direct = BpmMap::from_timing_events(
        100.0,
        4.0,
        4,
        &[
            bpm_event(3000.0, 200.0),
            timesig_event(6000.0, 3.0, 4.0, false),
        ],
    );
    let imported = BpmMap::from_codename_chart(chart, song.metadata.as_ref());
    assert_eq!(
        imported, direct,
        "importer-driven map equals the direct map"
    );

    // The imported events are the three interpreted ones, in order.
    let interpreted = timing_events_from_chart_events(&chart.events);
    assert_eq!(interpreted.len(), 2);
    assert_close("first event time", interpreted[0].time, 3000.0);
}

// --- judgement windows ------------------------------------------------------

#[test]
fn judgement_presets_match_hit_window_data() {
    let cne = WindowPreset::CneClassic.windows();
    assert_eq!(
        (cne.sick, cne.good, cne.bad, cne.shit),
        (50.0, 187.5, 225.0, 250.0)
    );
    let classic = WindowPreset::FnfClassic.windows();
    assert_eq!(
        (classic.sick, classic.good, classic.bad, classic.shit),
        (33.334, 125.0025, 150.003, 166.67)
    );
    let vslice = WindowPreset::FnFVSlice.windows();
    assert_eq!(
        (vslice.sick, vslice.good, vslice.bad, vslice.shit),
        (45.0, 90.0, 135.4, 180.0)
    );
    let default = WindowPreset::Default.windows();
    assert_eq!(
        (default.sick, default.good, default.bad, default.shit),
        (37.8, 75.6, 113.4, 180.0)
    );

    assert_eq!(
        JUDGE_SCALES,
        [1.5, 1.33, 1.16, 1.0, 0.84, 0.66, 0.5, 0.33, 0.2]
    );
}

#[test]
fn ratings_follow_the_window_boundaries() {
    let windows = HitWindows::default();
    assert_eq!(windows.rating_for(0.0), HitRating::Sick);
    assert_eq!(windows.rating_for(37.8), HitRating::Sick);
    assert_eq!(windows.rating_for(-37.8), HitRating::Sick);
    assert_eq!(windows.rating_for(37.81), HitRating::Good);
    assert_eq!(windows.rating_for(75.6), HitRating::Good);
    assert_eq!(windows.rating_for(113.4), HitRating::Bad);
    assert_eq!(windows.rating_for(180.0), HitRating::Shit);
    assert_eq!(windows.rating_for(180.01), HitRating::Miss);

    let scaled = windows.scaled(0.5);
    assert_close("scaled sick", scaled.sick, 18.9);
    let offset = windows.offset(10.0);
    assert_close("offset sick", offset.sick, 47.8);
    assert_eq!(
        WindowPreset::from_name("FNF_VSlice"),
        Some(WindowPreset::FnFVSlice)
    );
}

// --- real Codename assets (skipped when the installation is absent) ---------

#[test]
fn real_codename_charts_keep_constant_tempo_timing() {
    let root = Path::new(CODENAME_ASSETS);
    if !root.is_dir() {
        eprintln!("skipping: {CODENAME_ASSETS} is not present on this machine");
        return;
    }

    for song_id in ["bopeebo", "dadbattle", "cocoa"] {
        let meta_path = root.join("songs").join(song_id).join("meta.json");
        let chart_path = root
            .join("songs")
            .join(song_id)
            .join("charts")
            .join("easy.json");
        if !meta_path.is_file() || !chart_path.is_file() {
            eprintln!("skipping {song_id}: chart shape not present");
            continue;
        }
        let meta_text = std::fs::read_to_string(&meta_path).unwrap();
        let chart_text = std::fs::read_to_string(&chart_path).unwrap();

        let temp = TempDir::new(&format!("fu-timing-real-{song_id}")).unwrap();
        temp.write(&format!("mod/songs/{song_id}/meta.json"), &meta_text)
            .unwrap();
        temp.write(
            &format!("mod/songs/{song_id}/charts/easy.json"),
            &chart_text,
        )
        .unwrap();
        temp.write("mod/data/config/modpack.ini", "[Common]\nAPI_VERSION=3\n")
            .unwrap();

        let discovered = open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_mod(&discovered);
        let Some(song) = parsed.module.song(song_id) else {
            eprintln!("skipping {song_id}: importer did not produce the song");
            continue;
        };
        let Some(chart) = song.chart("easy") else {
            eprintln!("skipping {song_id}: importer did not produce the chart");
            continue;
        };

        let map = BpmMap::from_codename_song(chart, song.events.as_ref(), song.metadata.as_ref());
        let meta_bpm = song
            .metadata
            .as_ref()
            .and_then(|meta| meta.bpm)
            .expect("meta.json declares bpm");

        if map.changes().len() != 1 {
            eprintln!(
                "skipping {song_id}: chart contains {} timing change(s), not a constant-tempo case",
                map.changes().len() - 1
            );
            continue;
        }

        assert_eq!(map.changes()[0].bpm, meta_bpm, "{song_id}: initial tempo");
        for time in [0.0, 1000.0, 5000.0, 30_000.0, 60_000.0] {
            let expected_steps = time * meta_bpm / 15_000.0;
            assert_close(
                &format!("{song_id} steps at {time} ms"),
                map.steps_at_time(time),
                expected_steps,
            );
            let round_trip = map.time_at_steps(expected_steps);
            assert_close(
                &format!("{song_id} round trip at {time} ms"),
                round_trip,
                time,
            );
        }
    }
}
