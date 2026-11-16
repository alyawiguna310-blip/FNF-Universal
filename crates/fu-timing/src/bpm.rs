//! BPM change map and the time/step/beat/measure conversions.
//!
//! Ported from `Conductor.hx` (`BPMChangeEvent`, `mapBPMChanges`, `mapEvent`,
//! `mapBPMChange`, `getTimeInChangeIndex`, `getTimeWithIndexInBPM`,
//! `getTimeWithBPMInSteps`, `getStepsWithBPMInTime`, `getStepsWithIndexInBPM`,
//! `getTimeInSteps`, `getStepsInTime`, `getTimeInBeats`, `getBeatsInTime`,
//! `getMeasuresInTime`).
//!
//! A `BpmMap` always contains at least the initial change at song time `0`.

use serde::{Deserialize, Serialize};

use fu_import::codename::{Chart, ChartEvent, SongEvents, SongMetadata};

/// Haxe `Flags.DEFAULT_BPM`.
pub const DEFAULT_BPM: f64 = 100.0;
/// Haxe `Flags.DEFAULT_BEATS_PER_MEASURE`.
pub const DEFAULT_BEATS_PER_MEASURE: f64 = 4.0;
/// Haxe `Flags.DEFAULT_STEPS_PER_BEAT`.
pub const DEFAULT_STEPS_PER_BEAT: i32 = 4;

/// `60000 / 4`: Codename measures steps with `15000 / bpm` milliseconds each.
pub const MS_PER_STEP_MINUTE: f64 = 15_000.0;

/// `MathUtil` margin used by `equal`/`lessThan`/`greaterThan`.
const FLOAT_MARGIN: f64 = 0.000_000_1;

/// Haxe `CoolUtil.floorInt`: truncate toward zero, then step down if negative
/// and not integral (i.e. mathematical floor).
pub fn floor_int(value: f64) -> i64 {
    let truncated = value.trunc();
    if value < 0.0 && truncated != value {
        truncated as i64 - 1
    } else {
        truncated as i64
    }
}

/// Haxe `CoolUtil.boundInt`.
pub fn bound_int(value: i64, min: i64, max: i64) -> i64 {
    value.clamp(min, max)
}

/// `MathUtil.equal`.
pub fn approx_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= FLOAT_MARGIN
}

/// `MathUtil.lessThan`.
pub fn approx_less_than(a: f64, b: f64) -> bool {
    a < b - FLOAT_MARGIN
}

/// `FlxMath.remapToRange`.
fn remap_to_range(value: f64, start1: f64, stop1: f64, start2: f64, stop2: f64) -> f64 {
    start2 + (stop2 - start2) * ((value - start1) / (stop1 - start1))
}

/// One entry of the BPM change map (`Conductor.BPMChangeEvent`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BpmChange {
    /// Time in the song, in milliseconds, where this change starts.
    pub song_time: f64,
    /// Tempo from this change on.
    pub bpm: f64,
    /// Time-signature numerator.
    pub beats_per_measure: f64,
    /// Steps per beat.
    pub steps_per_beat: i32,
    /// End of a continuous BPM change, in milliseconds (`0` otherwise).
    pub end_song_time: f64,
    /// End of a continuous BPM change, in steps (`0` otherwise).
    pub end_step_time: f64,
    /// True when this change interpolates until `end_song_time`.
    pub continuous: bool,
    /// Absolute step index at `song_time`.
    pub step_time: f64,
    /// Absolute beat index at `song_time`.
    pub beat_time: f64,
    /// Absolute measure index at `song_time`.
    pub measure_time: f64,
}

impl BpmChange {
    /// The first map entry, at song time `0`.
    pub fn initial(bpm: f64, beats_per_measure: f64, steps_per_beat: i32) -> Self {
        Self {
            song_time: 0.0,
            bpm,
            beats_per_measure,
            steps_per_beat,
            end_song_time: 0.0,
            end_step_time: 0.0,
            continuous: false,
            step_time: 0.0,
            beat_time: 0.0,
            measure_time: 0.0,
        }
    }
}

/// The timing event kinds the conductor interprets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TimingEventKind {
    /// `BPM Change`.
    BpmChange {
        /// Target tempo.
        bpm: f64,
    },
    /// `Time Signature Change`.
    TimeSignatureChange {
        /// Numerator (beats per measure).
        numerator: f64,
        /// Denominator, or the step count when `denominator_is_steps_per_beat`.
        denominator: f64,
        /// `params[2]`: the second parameter is already steps per beat.
        denominator_is_steps_per_beat: bool,
    },
    /// `Continuous BPM Change`.
    ContinuousBpmChange {
        /// Target tempo at the end of the ramp.
        bpm: f64,
        /// Ramp length in steps.
        steps: f64,
    },
}

impl TimingEventKind {
    /// Event name as written in chart JSON.
    pub fn name(&self) -> &'static str {
        match self {
            Self::BpmChange { .. } => "BPM Change",
            Self::TimeSignatureChange { .. } => "Time Signature Change",
            Self::ContinuousBpmChange { .. } => "Continuous BPM Change",
        }
    }

    fn is_time_signature(&self) -> bool {
        matches!(self, Self::TimeSignatureChange { .. })
    }
}

/// One timing event, already validated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimingEvent {
    /// Event time in milliseconds.
    pub time: f64,
    /// Interpreted payload.
    pub kind: TimingEventKind,
}

/// Why the conductor refused an event (`Conductor.invalidEvents`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RejectReason {
    /// The event falls inside a continuous change.
    IntersectsContinuousChange,
    /// A continuous change whose target equals the current tempo.
    ContinuousChangeToSameBpm,
}

/// An event the conductor rejected, with its position in the source list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectedEvent {
    /// Index in the caller-supplied event slice.
    pub index: usize,
    /// Event time.
    pub time: f64,
    /// Event name.
    pub name: String,
    /// Why it was rejected.
    pub reason: RejectReason,
}

/// The BPM change map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BpmMap {
    changes: Vec<BpmChange>,
    rejected: Vec<RejectedEvent>,
}

impl BpmMap {
    /// A map with a single change and the Codename defaults.
    pub fn new(bpm: f64, beats_per_measure: f64, steps_per_beat: i32) -> Self {
        Self {
            changes: vec![BpmChange::initial(
                bpm,
                beats_per_measure,
                steps_per_beat.max(1),
            )],
            rejected: Vec::new(),
        }
    }

    /// A map at `Flags.DEFAULT_BPM`.
    pub fn default_map() -> Self {
        Self::new(
            DEFAULT_BPM,
            DEFAULT_BEATS_PER_MEASURE,
            DEFAULT_STEPS_PER_BEAT,
        )
    }

    /// Every change, in song order. Always non-empty.
    pub fn changes(&self) -> &[BpmChange] {
        &self.changes
    }

    /// Events the conductor rejected, in discovery order.
    pub fn rejected(&self) -> &[RejectedEvent] {
        &self.rejected
    }

    /// Builds the map exactly like `Conductor.mapBPMChanges`.
    ///
    /// Events are sorted by `(time, time-signature-first)`, matching
    /// `events.sort((a, b) -> equal(a.time, b.time) ? (a.name == "Time
    /// Signature Change" ? -1 : 1) : Std.int(a.time - b.time))`.
    pub fn from_timing_events(
        initial_bpm: f64,
        beats_per_measure: f64,
        steps_per_beat: i32,
        events: &[TimingEvent],
    ) -> Self {
        let mut map = Self::new(initial_bpm, beats_per_measure, steps_per_beat);

        let mut ordered: Vec<(usize, TimingEvent)> = events
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, event)| event.time.is_finite())
            .collect();
        // Haxe compares `Std.int(a.time - b.time)`, the truncated
        // difference: events less than a millisecond apart compare equal.
        // Reproduce that instead of a float comparison so ordering matches.
        ordered.sort_by(|(_, left), (_, right)| {
            let delta = (left.time - right.time).trunc();
            if delta < 0.0 {
                std::cmp::Ordering::Less
            } else if delta > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                let left_key = u8::from(!left.kind.is_time_signature());
                let right_key = u8::from(!right.kind.is_time_signature());
                left_key.cmp(&right_key)
            }
        });

        for (index, event) in ordered {
            map.apply(index, event);
        }
        map
    }

    /// Builds the map from a parsed Codename chart and its song metadata.
    ///
    /// `chart.events` is used; call [`Self::from_codename_song`] to also include
    /// the song-level `events.json`.
    pub fn from_codename_chart(chart: &Chart, metadata: Option<&SongMetadata>) -> Self {
        let (bpm, beats, steps) = initial_meta(metadata);
        let events = timing_events_from_chart_events(&chart.events);
        Self::from_timing_events(bpm, beats, steps, &events)
    }

    /// Builds the map from a Codename chart plus its song-level `events.json`.
    pub fn from_codename_song(
        chart: &Chart,
        song_events: Option<&SongEvents>,
        metadata: Option<&SongMetadata>,
    ) -> Self {
        let (bpm, beats, steps) = initial_meta(metadata);
        let mut events = timing_events_from_chart_events(&chart.events);
        if let Some(song_events) = song_events {
            events.extend(timing_events_from_chart_events(&song_events.events));
        }
        Self::from_timing_events(bpm, beats, steps, &events)
    }

    /// Current tempo for an absolute change index.
    pub fn bpm_at_index(&self, index: isize) -> f64 {
        self.changes
            .get(index.max(0) as usize)
            .map(|change| change.bpm)
            .unwrap_or(DEFAULT_BPM)
    }

    /// Tempo at a song position, interpolating continuous changes
    /// (`Conductor.getTimeInBPM`).
    pub fn bpm_at_time(&self, time: f64) -> f64 {
        if self.changes.is_empty() {
            return DEFAULT_BPM;
        }
        let index = self.change_index_at_time(time, 0);
        self.time_with_index_in_bpm(time, index)
    }

    /// `Conductor.getTimeInChangeIndex`.
    pub fn change_index_at_time(&self, time: f64, from: isize) -> isize {
        self.index_for(time, from, |change| change.song_time)
    }

    /// `Conductor.getStepsInChangeIndex`.
    pub fn change_index_at_step(&self, step: f64, from: isize) -> isize {
        self.index_for(step, from, |change| change.step_time)
    }

    /// `Conductor.getBeatsInChangeIndex`.
    pub fn change_index_at_beat(&self, beat: f64, from: isize) -> isize {
        self.index_for(beat, from, |change| change.beat_time)
    }

    /// `Conductor.getMeasuresInChangeIndex`.
    pub fn change_index_at_measure(&self, measure: f64, from: isize) -> isize {
        self.index_for(measure, from, |change| change.measure_time)
    }

    fn index_for<F>(&self, value: f64, from: isize, key: F) -> isize
    where
        F: Fn(&BpmChange) -> f64,
    {
        let len = self.changes.len();
        if len < 2 {
            return len as isize - 1;
        }
        let mut index = bound_int(from as i64, 0, len as i64 - 1) as usize;
        if key(&self.changes[index]) > value {
            while index > 1 {
                index -= 1;
                if value > key(&self.changes[index]) {
                    return index as isize;
                }
            }
            return 0;
        }
        for i in index..len {
            if key(&self.changes[i]) > value {
                return i as isize - 1;
            }
        }
        len as isize - 1
    }

    /// `Conductor.getTimeWithIndexInBPM`.
    pub fn time_with_index_in_bpm(&self, time: f64, index: isize) -> f64 {
        let Some(change) = self.changes.get(index.max(0) as usize) else {
            return DEFAULT_BPM;
        };
        if change.continuous && time < change.end_song_time && index > 0 {
            let previous = self.bpm_at_index(index - 1);
            if time <= change.song_time {
                return previous;
            }
            let ratio = (time - change.song_time) / (change.end_song_time - change.song_time);
            return previous.powf(1.0 - ratio) * change.bpm.powf(ratio);
        }
        change.bpm
    }

    /// `Conductor.getTimeWithBPMInSteps`.
    pub fn time_with_bpm_in_steps(&self, time: f64, index: isize, bpm: f64) -> f64 {
        let Some(change) = self.changes.get(index.max(0) as usize) else {
            return 0.0;
        };
        if change.continuous && time > change.song_time && index > 0 {
            let previous = self.bpm_at_index(index - 1);
            if time > change.end_song_time {
                return change.step_time
                    + (((change.end_song_time - change.song_time) * (change.bpm - previous))
                        / (change.bpm / previous).ln()
                        + (time - change.end_song_time) * bpm)
                        / MS_PER_STEP_MINUTE;
            } else if approx_equal(bpm, previous) {
                return change.step_time
                    + (time - change.song_time) * (MS_PER_STEP_MINUTE / previous);
            }
            return change.step_time
                + (time - change.song_time) * (bpm - previous)
                    / (bpm / previous).ln()
                    / MS_PER_STEP_MINUTE;
        }
        change.step_time + (time - change.song_time) / (MS_PER_STEP_MINUTE / bpm)
    }

    /// `Conductor.getStepsWithIndexInBPM`.
    pub fn steps_with_index_in_bpm(&self, step: f64, index: isize) -> f64 {
        let Some(change) = self.changes.get(index.max(0) as usize) else {
            return DEFAULT_BPM;
        };
        if change.continuous && index > 0 {
            let previous = self.bpm_at_index(index - 1);
            if step <= change.step_time {
                return previous;
            }
            let end_step_time = change.step_time
                + (change.end_song_time - change.song_time) * (change.bpm - previous)
                    / (change.bpm / previous).ln()
                    / MS_PER_STEP_MINUTE;
            if step < end_step_time {
                return remap_to_range(step, change.step_time, end_step_time, previous, change.bpm);
            }
        }
        change.bpm
    }

    /// `Conductor.getStepsWithBPMInTime`.
    pub fn steps_with_bpm_in_time(&self, step: f64, index: isize, bpm: f64) -> f64 {
        let Some(change) = self.changes.get(index.max(0) as usize) else {
            return 0.0;
        };
        if change.continuous && step > change.step_time && index > 0 {
            let previous = self.bpm_at_index(index - 1);
            if approx_equal(bpm, previous) {
                return change.song_time
                    + (step - change.step_time) * (MS_PER_STEP_MINUTE / previous);
            }
            let time = change.song_time
                + (step - change.step_time) / (bpm - previous)
                    * (bpm / previous).ln()
                    * MS_PER_STEP_MINUTE;
            if time > change.end_song_time {
                return (MS_PER_STEP_MINUTE * (step - change.step_time)
                    - ((change.end_song_time - change.song_time) * (bpm - previous))
                        / (bpm / previous).ln())
                    / bpm
                    + change.end_song_time;
            }
            return time;
        }
        change.song_time + (step - change.step_time) * (MS_PER_STEP_MINUTE / bpm)
    }

    /// `Conductor.getTimeInSteps`: song time to step index.
    pub fn steps_at_time(&self, time: f64) -> f64 {
        self.steps_at_time_from(time, 0)
    }

    /// `Conductor.getTimeInSteps(time, from)`.
    pub fn steps_at_time_from(&self, time: f64, from: isize) -> f64 {
        let index = self.change_index_at_time(time, from);
        if index < 1 {
            time / (MS_PER_STEP_MINUTE / self.bpm_at_time(time))
        } else {
            self.time_with_bpm_in_steps(time, index, self.time_with_index_in_bpm(time, index))
        }
    }

    /// `Conductor.getStepsInTime`: step index to song time.
    pub fn time_at_steps(&self, step: f64) -> f64 {
        self.time_at_steps_from(step, 0)
    }

    /// `Conductor.getStepsInTime(stepTime, from)`.
    pub fn time_at_steps_from(&self, step: f64, from: isize) -> f64 {
        let index = self.change_index_at_step(step, from);
        if index < 1 {
            step * (MS_PER_STEP_MINUTE / self.bpm_at_index(index))
        } else {
            self.steps_with_bpm_in_time(step, index, self.steps_with_index_in_bpm(step, index))
        }
    }

    /// `Conductor.getTimeInBeats`.
    pub fn beats_at_time(&self, time: f64) -> f64 {
        self.beats_at_time_from(time, 0)
    }

    /// `Conductor.getTimeInBeats(time, from)`.
    pub fn beats_at_time_from(&self, time: f64, from: isize) -> f64 {
        let index = self.change_index_at_time(time, from);
        if index == -1 {
            return time / (60_000.0 / DEFAULT_BPM);
        }
        if index == 0 {
            return time
                / (MS_PER_STEP_MINUTE / self.bpm_at_index(index))
                / self.changes[0].steps_per_beat as f64;
        }
        let change = &self.changes[index as usize];
        change.beat_time
            + (self.time_with_bpm_in_steps(time, index, self.time_with_index_in_bpm(time, index))
                - change.step_time)
                / change.steps_per_beat as f64
    }

    /// `Conductor.getBeatsInTime`: beat index to song time.
    pub fn time_at_beats(&self, beat: f64) -> f64 {
        self.time_at_beats_from(beat, 0)
    }

    /// `Conductor.getBeatsInTime(beatTime, from)`.
    pub fn time_at_beats_from(&self, beat: f64, from: isize) -> f64 {
        let index = self.change_index_at_beat(beat, from);
        if index == -1 {
            return beat * (MS_PER_STEP_MINUTE / DEFAULT_BPM);
        }
        if index == 0 {
            return beat
                * (MS_PER_STEP_MINUTE / self.bpm_at_index(index))
                * self.changes[0].steps_per_beat as f64;
        }
        let change = &self.changes[index as usize];
        let step = change.step_time + (beat - change.beat_time) * change.steps_per_beat as f64;
        self.steps_with_bpm_in_time(step, index, self.steps_with_index_in_bpm(step, index))
    }

    /// `Conductor.getMeasuresInTime`: measure index to song time.
    pub fn time_at_measures(&self, measure: f64) -> f64 {
        self.time_at_measures_from(measure, 0)
    }

    /// `Conductor.getMeasuresInTime(measureTime, from)`.
    pub fn time_at_measures_from(&self, measure: f64, from: isize) -> f64 {
        let index = self.change_index_at_measure(measure, from);
        if index == -1 {
            return measure * (MS_PER_STEP_MINUTE / DEFAULT_BPM);
        }
        if index == 0 {
            return measure
                * (MS_PER_STEP_MINUTE / self.bpm_at_index(index))
                * self.changes[0].steps_per_beat as f64
                * self.changes[0].beats_per_measure;
        }
        let change = &self.changes[index as usize];
        let step = change.step_time
            + (measure - change.measure_time)
                * change.steps_per_beat as f64
                * change.beats_per_measure;
        self.steps_with_bpm_in_time(step, index, self.steps_with_index_in_bpm(step, index))
    }

    /// Measure index at a song position. Mirrors the derivation in
    /// `Conductor.update` (Haxe has no public getter for it).
    pub fn measures_at_time(&self, time: f64) -> f64 {
        let index = self.change_index_at_time(time, 0);
        if index < 0 {
            return time / (MS_PER_STEP_MINUTE / DEFAULT_BPM);
        }
        if index == 0 {
            let change = &self.changes[0];
            return time
                / (MS_PER_STEP_MINUTE / change.bpm)
                / change.steps_per_beat as f64
                / change.beats_per_measure;
        }
        let change = &self.changes[index as usize];
        change.measure_time
            + (self.beats_at_time_from(time, index) - change.beat_time) / change.beats_per_measure
    }

    /// `Conductor.getMeasureLength`: steps in one measure.
    pub fn measure_length(&self) -> f64 {
        let change = &self.changes[self.changes.len() - 1];
        change.steps_per_beat as f64 * change.beats_per_measure
    }

    // --- map construction ---------------------------------------------------

    /// `Conductor.mapBPMChange`.
    fn push_change(&mut self, time: f64, bpm: f64) {
        let current = self.changes.last().cloned().unwrap_or_default();
        let step_time = (if current.continuous {
            current.end_step_time
        } else {
            current.step_time
        }) + (time
            - if current.continuous {
                current.end_song_time
            } else {
                current.song_time
            })
            / (MS_PER_STEP_MINUTE / current.bpm);
        let beat_time =
            current.beat_time + (step_time - current.step_time) / current.steps_per_beat as f64;
        let measure_time =
            current.measure_time + (beat_time - current.beat_time) / current.beats_per_measure;
        self.changes.push(BpmChange {
            song_time: time,
            bpm,
            beats_per_measure: current.beats_per_measure,
            steps_per_beat: current.steps_per_beat,
            end_song_time: 0.0,
            end_step_time: 0.0,
            continuous: false,
            step_time,
            beat_time,
            measure_time,
        });
    }

    /// `Conductor.mapEvent`.
    fn apply(&mut self, index: usize, event: TimingEvent) {
        let current = self.changes.last().cloned().unwrap_or_default();

        if current.continuous && approx_less_than(event.time, current.end_song_time) {
            self.rejected.push(RejectedEvent {
                index,
                time: event.time,
                name: event.kind.name().to_string(),
                reason: RejectReason::IntersectsContinuousChange,
            });
            return;
        }

        match event.kind {
            TimingEventKind::BpmChange { bpm } => {
                if current.bpm != bpm {
                    self.push_change(event.time, bpm);
                }
            }
            TimingEventKind::TimeSignatureChange {
                numerator,
                denominator,
                denominator_is_steps_per_beat,
            } => {
                if current.song_time != event.time {
                    self.push_change(event.time, current.bpm);
                }
                let last = self.changes.len() - 1;
                self.changes[last].beats_per_measure = numerator;
                self.changes[last].steps_per_beat = if denominator_is_steps_per_beat {
                    floor_int(denominator) as i32
                } else {
                    (16.0 / denominator).floor() as i32
                };
                self.changes[last].step_time =
                    floor_int(self.changes[last].step_time + 0.999_98) as f64;
                self.changes[last].beat_time =
                    floor_int(self.changes[last].beat_time + 0.999_98) as f64;
                self.changes[last].measure_time =
                    floor_int(self.changes[last].measure_time + 0.999_98) as f64;
            }
            TimingEventKind::ContinuousBpmChange { bpm, steps } => {
                let previous_bpm = current.bpm;
                if previous_bpm == bpm {
                    self.rejected.push(RejectedEvent {
                        index,
                        time: event.time,
                        name: event.kind.name().to_string(),
                        reason: RejectReason::ContinuousChangeToSameBpm,
                    });
                    return;
                }
                self.push_change(event.time, bpm);
                let last = self.changes.len() - 1;
                let change = &mut self.changes[last];
                change.end_song_time = event.time
                    + steps / (change.bpm - previous_bpm)
                        * (change.bpm / previous_bpm).ln()
                        * MS_PER_STEP_MINUTE;
                change.end_step_time = change.step_time + steps;
                change.continuous = true;
            }
        }
    }
}

impl Default for BpmChange {
    fn default() -> Self {
        Self::initial(
            DEFAULT_BPM,
            DEFAULT_BEATS_PER_MEASURE,
            DEFAULT_STEPS_PER_BEAT,
        )
    }
}

fn initial_meta(metadata: Option<&SongMetadata>) -> (f64, f64, i32) {
    let bpm = metadata
        .and_then(|meta| meta.bpm)
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
        .unwrap_or(DEFAULT_BPM);
    let beats = metadata
        .and_then(|meta| meta.beats_per_measure)
        .filter(|beats| beats.is_finite() && *beats > 0.0)
        .unwrap_or(DEFAULT_BEATS_PER_MEASURE);
    let steps = metadata
        .and_then(|meta| meta.steps_per_beat)
        .filter(|steps| steps.is_finite() && *steps >= 1.0)
        .map(|steps| floor_int(steps) as i32)
        .unwrap_or(DEFAULT_STEPS_PER_BEAT);
    (bpm, beats, steps)
}

fn json_number(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn json_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::Number(number) => number.as_u64().map(|value| value != 0),
        serde_json::Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Converts Codename chart events into interpreted timing events.
///
/// Codename's `Conductor.mapEvent` only accepts a `BPM Change` whose first
/// parameter is a Haxe `Float`; integer JSON literals are `Int` there and are
/// silently ignored. This port accepts any JSON number, which is the behaviour
/// the charter intends (its event editor stores `TFloat`). That is the single
/// deliberate deviation and it is recorded in
/// `docs/CODENAME_RUST_MIGRATION.md`.
pub fn timing_events_from_chart_events(events: &[ChartEvent]) -> Vec<TimingEvent> {
    let mut result = Vec::new();
    for event in events {
        let Some(time) = event.time.filter(|time| time.is_finite()) else {
            continue;
        };
        let Some(name) = event.name.as_deref() else {
            continue;
        };
        let kind = match name {
            "BPM Change" => match event.params.first().and_then(json_number) {
                Some(bpm) => TimingEventKind::BpmChange { bpm },
                None => continue,
            },
            "Time Signature Change" => {
                let Some(numerator) = event.params.first().and_then(json_number) else {
                    continue;
                };
                let Some(denominator) = event.params.get(1).and_then(json_number) else {
                    continue;
                };
                let denominator_is_steps_per_beat =
                    event.params.get(2).and_then(json_bool).unwrap_or(false);
                TimingEventKind::TimeSignatureChange {
                    numerator,
                    denominator,
                    denominator_is_steps_per_beat,
                }
            }
            "Continuous BPM Change" => {
                let Some(bpm) = event.params.first().and_then(json_number) else {
                    continue;
                };
                let Some(steps) = event.params.get(1).and_then(json_number) else {
                    continue;
                };
                TimingEventKind::ContinuousBpmChange { bpm, steps }
            }
            _ => continue,
        };
        result.push(TimingEvent { time, kind });
    }
    result
}
