//! The unified runtime chart model.
//!
//! A [`RuntimeChart`] is what the engine plays: notes at full `f64`
//! millisecond precision, the [`BpmMap`] those times are measured against, the
//! strum lines they belong to, and the chart events in song order. Both
//! importers project into it (`fu_import::codename` and `fu_import::vslice`),
//! so timing is interpreted in exactly one place per format and every consumer
//! (gameplay, rendering, future vocals/stages) reads the same numbers.
//!
//! Note times are never rounded. `fu-timing` judges and converts with `f64`
//! throughout, and a note that was authored at `1234.5 ms` stays at
//! `1234.5 ms` here instead of becoming `1235 ms`.

use fu_import::{codename, vslice};
use fu_timing::{
    BpmMap, Conductor, HitWindows, TimingEvent, TimingEventKind, WindowPreset,
    DEFAULT_BEATS_PER_MEASURE, DEFAULT_BPM, DEFAULT_STEPS_PER_BEAT,
};
use serde_json::Value;

/// Which side of the stage a strum line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrumPosition {
    /// Notes the player hits (`boyfriend` / `player`).
    Player,
    /// Notes the opponent hits (`dad` / `opponent`).
    Opponent,
    /// Notes on the girlfriend line.
    Girlfriend,
    /// Any other line name a mod uses.
    Other,
}

impl StrumPosition {
    /// Classifies a Codename `position` string, case-insensitively.
    pub fn from_name(name: Option<&str>) -> Self {
        match name.map(str::trim) {
            Some(name)
                if name.eq_ignore_ascii_case("boyfriend")
                    || name.eq_ignore_ascii_case("player") =>
            {
                Self::Player
            }
            Some(name)
                if name.eq_ignore_ascii_case("dad") || name.eq_ignore_ascii_case("opponent") =>
            {
                Self::Opponent
            }
            Some(name)
                if name.eq_ignore_ascii_case("gf") || name.eq_ignore_ascii_case("girlfriend") =>
            {
                Self::Girlfriend
            }
            _ => Self::Other,
        }
    }

    /// True for the side the local player judges.
    pub fn is_player(self) -> bool {
        matches!(self, Self::Player)
    }
}

/// One note, at full precision and with its step resolved through the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuntimeNote {
    /// Hit time in milliseconds, exactly as authored.
    pub time_ms: f64,
    /// Lane inside the strum line.
    pub lane: u8,
    /// Sustain length in milliseconds (`sLen`/`l`).
    pub sustain_ms: f64,
    /// Fractional step index at [`Self::time_ms`], from the timing map.
    pub step: f64,
}

/// One strum line and its notes.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeStrumLine {
    /// Side of the stage this line plays on.
    pub position: StrumPosition,
    /// Lane count (`keyCount`), at least one.
    pub key_count: u8,
    /// Notes in time order.
    pub notes: Vec<RuntimeNote>,
}

impl RuntimeStrumLine {
    /// True when the player judges this line.
    pub fn is_player(&self) -> bool {
        self.position.is_player()
    }
}

/// One chart event, with its parameters left exactly as the chart wrote them.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeEvent {
    /// Event time in milliseconds.
    pub time_ms: f64,
    /// Event name, empty when the source omitted it.
    pub name: String,
    /// Parameter list, untouched.
    pub params: Vec<Value>,
}

/// A playable chart: notes, timing, strum lines and events.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeChart {
    /// Display name declared by the song, when it has one.
    pub song: String,
    /// Difficulty this chart was resolved for.
    pub difficulty: String,
    /// The single timing source for this chart.
    pub timing: BpmMap,
    /// Chart scroll speed, `1.0` when undeclared.
    pub scroll_speed: f64,
    /// Length of the chart in milliseconds. Audio length overrides it.
    pub length_ms: f64,
    /// Every strum line, player and opponent.
    pub strum_lines: Vec<RuntimeStrumLine>,
    /// Every chart event, sorted by time.
    pub events: Vec<RuntimeEvent>,
    /// Player notes flattened across player lines, sorted by `(time, lane)`.
    pub notes: Vec<RuntimeNote>,
    /// Lanes on the widest player line.
    pub lanes: u8,
}

impl RuntimeChart {
    /// Builds a chart from already-interpreted parts.
    ///
    /// Steps are resolved here, so a caller cannot supply a note whose `step`
    /// disagrees with [`Self::timing`]: that is the invariant this type exists
    /// to keep.
    pub fn new(
        song: impl Into<String>,
        difficulty: impl Into<String>,
        timing: BpmMap,
        mut strum_lines: Vec<RuntimeStrumLine>,
        mut events: Vec<RuntimeEvent>,
    ) -> Self {
        events.sort_by(|left, right| {
            left.time_ms
                .total_cmp(&right.time_ms)
                .then_with(|| left.name.cmp(&right.name))
        });

        for line in &mut strum_lines {
            line.notes.sort_by(|left, right| {
                left.time_ms
                    .total_cmp(&right.time_ms)
                    .then(left.lane.cmp(&right.lane))
            });
            for note in &mut line.notes {
                note.step = timing.steps_at_time(note.time_ms);
            }
        }

        let mut notes: Vec<RuntimeNote> = strum_lines
            .iter()
            .filter(|line| line.is_player())
            .flat_map(|line| line.notes.iter().copied())
            .collect();
        notes.sort_by(|left, right| {
            left.time_ms
                .total_cmp(&right.time_ms)
                .then(left.lane.cmp(&right.lane))
        });

        let lanes = strum_lines
            .iter()
            .filter(|line| line.is_player())
            .map(|line| line.key_count)
            .max()
            .unwrap_or(0);

        let length_ms = notes
            .last()
            .map(|note| note.time_ms.max(0.0))
            .unwrap_or(0.0);

        Self {
            song: song.into(),
            difficulty: difficulty.into(),
            timing,
            scroll_speed: 1.0,
            length_ms,
            strum_lines,
            events,
            notes,
            lanes,
        }
    }

    /// Overrides the chart length, normally with the decoded audio duration.
    pub fn with_length_ms(mut self, length_ms: f64) -> Self {
        if length_ms.is_finite() && length_ms > 0.0 {
            self.length_ms = length_ms;
        }
        self
    }

    /// Builds a chart from a parsed Codename chart file.
    pub fn from_codename_chart(
        chart: &codename::Chart,
        song_events: Option<&codename::SongEvents>,
        metadata: Option<&codename::SongMetadata>,
    ) -> Self {
        let timing = BpmMap::from_codename_song(chart, song_events, metadata);

        let mut events = Vec::new();
        let sources = chart.events.iter().chain(
            song_events
                .into_iter()
                .flat_map(|events| events.events.iter()),
        );
        for event in sources {
            let Some(time) = event.time.filter(|time| time.is_finite() && *time >= 0.0) else {
                continue;
            };
            events.push(RuntimeEvent {
                time_ms: time,
                name: event.name.clone().unwrap_or_default(),
                params: event.params.clone(),
            });
        }

        let mut strum_lines = Vec::with_capacity(chart.strum_lines.len());
        for line in &chart.strum_lines {
            let key_count = line.key_count.unwrap_or(4).clamp(1, 8) as u8;
            let mut notes = Vec::with_capacity(line.notes.len());
            for note in &line.notes {
                let Some(time) = note.time.filter(|time| time.is_finite() && *time >= 0.0) else {
                    continue;
                };
                let Some(lane) = note.id else {
                    continue;
                };
                if lane >= u32::from(key_count) {
                    continue;
                }
                notes.push(RuntimeNote {
                    time_ms: time,
                    lane: lane as u8,
                    sustain_ms: note.sustain_length.unwrap_or(0.0).max(0.0),
                    step: 0.0,
                });
            }
            strum_lines.push(RuntimeStrumLine {
                position: StrumPosition::from_name(line.position.as_deref()),
                key_count,
                notes,
            });
        }

        let song = metadata
            .and_then(|metadata| metadata.display_name.clone())
            .unwrap_or_else(|| chart.difficulty.clone());
        let scroll_speed = chart
            .scroll_speed
            .filter(|speed| speed.is_finite() && *speed > 0.0)
            .unwrap_or(1.0);

        let mut runtime = Self::new(song, chart.difficulty.clone(), timing, strum_lines, events);
        runtime.scroll_speed = scroll_speed;
        runtime
    }

    /// Builds a chart from one song of a parsed Codename mod.
    pub fn from_codename_song(song: &codename::Song, difficulty: &str) -> Option<Self> {
        let chart = song.chart(difficulty)?;
        Some(Self::from_codename_chart(
            chart,
            song.events.as_ref(),
            song.metadata.as_ref(),
        ))
    }

    /// Builds a chart from one difficulty of a parsed V-Slice chart.
    ///
    /// Returns `None` when the difficulty has no note array at all.
    pub fn from_vslice_chart(
        chart: &vslice::Chart,
        metadata: Option<&vslice::SongMetadata>,
        difficulty: &str,
    ) -> Option<Self> {
        let source_notes = chart.notes.get(difficulty)?;
        let timing = vslice_timing(metadata);

        let mut player = Vec::new();
        let mut opponent = Vec::new();
        for note in source_notes {
            let Some(time) = note.time_ms.filter(|time| time.is_finite() && *time >= 0.0) else {
                continue;
            };
            let Some(direction) = note
                .direction
                .filter(|direction| direction.is_finite() && *direction >= 0.0)
            else {
                continue;
            };
            let runtime_note = RuntimeNote {
                time_ms: time,
                lane: (direction as u8) % 4,
                sustain_ms: note.sustain_ms.unwrap_or(0.0).max(0.0),
                step: 0.0,
            };
            if note.is_player() == Some(true) {
                player.push(runtime_note);
            } else {
                opponent.push(runtime_note);
            }
        }

        let mut events = Vec::new();
        for event in &chart.events {
            let Some(time) = event
                .time_ms
                .filter(|time| time.is_finite() && *time >= 0.0)
            else {
                continue;
            };
            events.push(RuntimeEvent {
                time_ms: time,
                name: event.name.clone().unwrap_or_default(),
                params: event.value.clone().into_iter().collect(),
            });
        }

        let song = metadata
            .and_then(|metadata| metadata.song_name.clone())
            .unwrap_or_else(|| chart.source.clone());
        let scroll_speed = chart
            .scroll_speed
            .get(difficulty)
            .copied()
            .filter(|speed| speed.is_finite() && *speed > 0.0)
            .unwrap_or(1.0);

        let strum_lines = vec![
            RuntimeStrumLine {
                position: StrumPosition::Player,
                key_count: 4,
                notes: player,
            },
            RuntimeStrumLine {
                position: StrumPosition::Opponent,
                key_count: 4,
                notes: opponent,
            },
        ];

        let mut runtime = Self::new(song, difficulty.to_string(), timing, strum_lines, events);
        runtime.scroll_speed = scroll_speed;
        Some(runtime)
    }

    /// The strum line the local player judges.
    ///
    /// A Codename chart may declare several player lines (co-op); gameplay
    /// judges the first one, which is the solo-player line. Every line stays
    /// available through [`Self::strum_lines`].
    pub fn player_line(&self) -> Option<&RuntimeStrumLine> {
        self.strum_lines.iter().find(|line| line.is_player())
    }

    /// Every player strum line, in chart order.
    pub fn player_lines(&self) -> impl Iterator<Item = &RuntimeStrumLine> {
        self.strum_lines.iter().filter(|line| line.is_player())
    }

    /// Every non-player line (opponent and girlfriend), in chart order.
    pub fn opponent_lines(&self) -> impl Iterator<Item = &RuntimeStrumLine> {
        self.strum_lines.iter().filter(|line| !line.is_player())
    }

    /// Lane count of the player line gameplay judges.
    pub fn player_lanes(&self) -> usize {
        self.player_line()
            .map(|line| line.key_count as usize)
            .unwrap_or(0)
    }

    /// Tempo at `time_ms`, interpolating continuous BPM changes.
    pub fn bpm_at(&self, time_ms: f64) -> f64 {
        self.timing.bpm_at_time(time_ms)
    }

    /// Fractional step index at `time_ms`.
    pub fn step_at(&self, time_ms: f64) -> f64 {
        self.timing.steps_at_time(time_ms)
    }

    /// Fractional beat index at `time_ms`.
    pub fn beat_at(&self, time_ms: f64) -> f64 {
        self.timing.beats_at_time(time_ms)
    }

    /// Fractional measure index at `time_ms`.
    pub fn measure_at(&self, time_ms: f64) -> f64 {
        self.timing.measures_at_time(time_ms)
    }

    /// Joint from a step index to milliseconds.
    pub fn time_at_step(&self, step: f64) -> f64 {
        self.timing.time_at_steps(step)
    }

    /// Milliseconds per step at the start of the chart.
    pub fn step_crochet_ms(&self) -> f64 {
        15_000.0 / self.timing.changes()[0].bpm
    }

    /// Milliseconds per beat at the start of the chart.
    pub fn beat_crochet_ms(&self) -> f64 {
        15_000.0 * self.timing.changes()[0].steps_per_beat as f64 / self.timing.changes()[0].bpm
    }

    /// A conductor over this chart's timing, already at song position zero.
    pub fn conductor(&self) -> Conductor {
        Conductor::new(self.timing.clone())
    }

    /// Judgement windows for `preset`.
    pub fn windows(&self, preset: WindowPreset) -> HitWindows {
        preset.windows()
    }

    /// Number of player notes.
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// Time of the first player note, `0` for an empty chart.
    pub fn first_note_ms(&self) -> f64 {
        self.notes.first().map(|note| note.time_ms).unwrap_or(0.0)
    }

    /// Time of the last player note, `0` for an empty chart.
    pub fn last_note_ms(&self) -> f64 {
        self.notes.last().map(|note| note.time_ms).unwrap_or(0.0)
    }
}

/// Interprets V-Slice `timeChanges` as a Codename-compatible BPM map.
///
/// V-Slice stores tempo and time signature changes in one list, while Codename
/// keeps them as chart events. Each entry is projected onto the same two event
/// kinds `Conductor.mapEvent` understands, and the first entry supplies the
/// initial tempo and time signature.
pub fn vslice_timing(metadata: Option<&vslice::SongMetadata>) -> BpmMap {
    let mut changes: Vec<&vslice::TimeChange> = metadata
        .map(|metadata| metadata.time_changes.iter().collect())
        .unwrap_or_default();
    changes.sort_by(|left, right| left.t.unwrap_or(0.0).total_cmp(&right.t.unwrap_or(0.0)));

    let first = changes.first();
    let initial_bpm = first
        .and_then(|change| change.bpm)
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
        .unwrap_or(DEFAULT_BPM);
    let initial_beats = first
        .and_then(|change| change.n)
        .filter(|beats| beats.is_finite() && *beats > 0.0)
        .unwrap_or(DEFAULT_BEATS_PER_MEASURE);
    let initial_steps = first
        .and_then(|change| change.d)
        .filter(|denominator| denominator.is_finite() && *denominator > 0.0)
        .map(|denominator| (16.0 / denominator).floor() as i32)
        .filter(|steps| *steps >= 1)
        .unwrap_or(DEFAULT_STEPS_PER_BEAT);

    let mut events = Vec::new();
    let mut last_bpm = initial_bpm;
    for change in &changes {
        let time = change.t.unwrap_or(0.0).max(0.0);
        if time <= 0.0 {
            continue;
        }
        if let Some(bpm) = change.bpm.filter(|bpm| bpm.is_finite() && *bpm > 0.0) {
            if bpm != last_bpm {
                events.push(TimingEvent {
                    time,
                    kind: TimingEventKind::BpmChange { bpm },
                });
                last_bpm = bpm;
            }
        }
        if let (Some(numerator), Some(denominator)) = (
            change.n.filter(|value| value.is_finite() && *value > 0.0),
            change.d.filter(|value| value.is_finite() && *value > 0.0),
        ) {
            events.push(TimingEvent {
                time,
                kind: TimingEventKind::TimeSignatureChange {
                    numerator,
                    denominator,
                    denominator_is_steps_per_beat: false,
                },
            });
        }
    }

    BpmMap::from_timing_events(initial_bpm, initial_beats, initial_steps, &events)
}
