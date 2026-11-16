//! The stateful Conductor.
//!
//! Ported from `Conductor.update()` and its getters. Codename reads the song
//! position from the music stream; here the caller passes it to
//! [`Conductor::update`] each frame, which keeps this crate free of audio
//! devices and makes the behaviour deterministic in tests.

use serde::{Deserialize, Serialize};

use crate::bpm::{floor_int, BpmChange, BpmMap};

/// A BPM signal fired during an update (`Conductor.onBPMChange`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BpmSignal {
    /// Tempo from this change on.
    pub bpm: f64,
    /// End of a continuous change, in milliseconds (`0` when not continuous).
    pub end_song_time: f64,
}

/// A time-signature signal fired during an update
/// (`Conductor.onTimeSignatureChange`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeSignatureSignal {
    /// Numerator.
    pub beats_per_measure: f64,
    /// Steps per beat.
    pub steps_per_beat: i32,
}

/// Everything one update produced.
///
/// Step, beat and measure hits are the exact indices Codename dispatches, in
/// dispatch order, so a caller can forward them to scripts and state
/// callbacks unchanged. Multiple entries appear when a frame crosses more than
/// one boundary; a backwards seek reports the single index it landed on, which
/// is what `Conductor.update` does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    /// `onStepHit` arguments.
    pub steps: Vec<i64>,
    /// `onBeatHit` arguments.
    pub beats: Vec<i64>,
    /// `onMeasureHit` arguments.
    pub measures: Vec<i64>,
    /// `onBPMChange`, when the change index moved between changes of tempo.
    pub bpm_change: Option<BpmSignal>,
    /// `onTimeSignatureChange`, when the change index moved between different
    /// time signatures.
    pub time_signature_change: Option<TimeSignatureSignal>,
}

impl Tick {
    /// True when nothing fired.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
            && self.beats.is_empty()
            && self.measures.is_empty()
            && self.bpm_change.is_none()
            && self.time_signature_change.is_none()
    }
}

/// Codename's timing state.
#[derive(Debug, Clone)]
pub struct Conductor {
    map: BpmMap,
    song_position: f64,
    song_offset: f64,
    cur_change_index: isize,
    cur_step: i64,
    cur_beat: i64,
    cur_measure: i64,
    cur_step_float: f64,
    cur_beat_float: f64,
    cur_measure_float: f64,
    last_song_position: f64,
}

impl Conductor {
    /// Builds a conductor over `map`.
    pub fn new(map: BpmMap) -> Self {
        Self {
            map,
            song_position: 0.0,
            song_offset: 0.0,
            cur_change_index: 0,
            cur_step: 0,
            cur_beat: 0,
            cur_measure: 0,
            cur_step_float: 0.0,
            cur_beat_float: 0.0,
            cur_measure_float: 0.0,
            last_song_position: 0.0,
        }
    }

    /// `Conductor.reset`: position, counters and change index back to zero.
    pub fn reset(&mut self) {
        self.song_position = 0.0;
        self.last_song_position = 0.0;
        self.cur_change_index = 0;
        self.cur_step = 0;
        self.cur_beat = 0;
        self.cur_measure = 0;
        self.cur_step_float = 0.0;
        self.cur_beat_float = 0.0;
        self.cur_measure_float = 0.0;
    }

    /// Replaces the map and resets, like `Conductor.setupSong`.
    pub fn set_map(&mut self, map: BpmMap) {
        self.map = map;
        self.reset();
    }

    /// The change map.
    pub fn map(&self) -> &BpmMap {
        &self.map
    }

    /// Mutable access to the map. The caller is responsible for calling
    /// [`Self::reset`] if the change layout was replaced.
    pub fn map_mut(&mut self) -> &mut BpmMap {
        &mut self.map
    }

    /// `Options.songOffset`, subtracted by [`Self::song_position`].
    pub fn set_song_offset(&mut self, offset_ms: f64) {
        self.song_offset = offset_ms;
    }

    /// Offset currently applied.
    pub fn song_offset(&self) -> f64 {
        self.song_offset
    }

    /// Raw position the player is at, offset applied (`Conductor.songPosition`).
    pub fn song_position(&self) -> f64 {
        self.song_position - self.song_offset
    }

    /// Position from the previous update.
    pub fn last_song_position(&self) -> f64 {
        self.last_song_position
    }

    /// Current change entry.
    pub fn current_change(&self) -> &BpmChange {
        let len = self.map.changes().len();
        self.map
            .changes()
            .get(self.cur_change_index.clamp(0, len as isize - 1) as usize)
            .unwrap_or_else(|| &self.map.changes()[0])
    }

    /// Index of the current change.
    pub fn cur_change_index(&self) -> isize {
        self.cur_change_index
    }

    /// `Conductor.bpm`.
    pub fn current_bpm(&self) -> f64 {
        if self.cur_change_index == 0 || self.map.changes().is_empty() {
            self.starting_bpm()
        } else {
            self.map
                .time_with_index_in_bpm(self.song_position, self.cur_change_index)
        }
    }

    /// `Conductor.startingBPM`.
    pub fn starting_bpm(&self) -> f64 {
        self.map.changes()[0].bpm
    }

    /// `Conductor.stepCrochet`: milliseconds per step.
    pub fn step_crochet(&self) -> f64 {
        15_000.0 / self.current_bpm()
    }

    /// `Conductor.crochet`: milliseconds per beat.
    pub fn crochet(&self) -> f64 {
        15_000.0 * self.steps_per_beat() as f64 / self.current_bpm()
    }

    /// `Conductor.stepsPerBeat`.
    pub fn steps_per_beat(&self) -> i32 {
        self.current_change().steps_per_beat
    }

    /// `Conductor.beatsPerMeasure`.
    pub fn beats_per_measure(&self) -> f64 {
        self.current_change().beats_per_measure
    }

    /// `Conductor.getMeasureLength`.
    pub fn measure_length(&self) -> f64 {
        self.steps_per_beat() as f64 * self.beats_per_measure()
    }

    /// Current integer step.
    pub fn cur_step(&self) -> i64 {
        self.cur_step
    }

    /// Current integer beat.
    pub fn cur_beat(&self) -> i64 {
        self.cur_beat
    }

    /// Current integer measure.
    pub fn cur_measure(&self) -> i64 {
        self.cur_measure
    }

    /// Fractional step.
    pub fn cur_step_float(&self) -> f64 {
        self.cur_step_float
    }

    /// Fractional beat.
    pub fn cur_beat_float(&self) -> f64 {
        self.cur_beat_float
    }

    /// Fractional measure.
    pub fn cur_measure_float(&self) -> f64 {
        self.cur_measure_float
    }

    /// Advances to `song_position` (milliseconds, raw) and reports the signals
    /// `Conductor.update` would dispatch.
    pub fn update(&mut self, song_position: f64) -> Tick {
        let mut tick = Tick::default();
        let previous_position = self.song_position;
        self.song_position = song_position;
        self.last_song_position = previous_position;

        let old_step = self.cur_step;
        let old_beat = self.cur_beat;
        let old_measure = self.cur_measure;
        let old_index = self.cur_change_index;

        self.cur_change_index = self
            .map
            .change_index_at_time(song_position, self.cur_change_index);

        if self.cur_change_index > 0 {
            let change = self.current_change().clone();
            self.cur_step_float = self.map.time_with_bpm_in_steps(
                song_position,
                self.cur_change_index,
                self.map
                    .time_with_index_in_bpm(song_position, self.cur_change_index),
            );
            self.cur_beat_float = change.beat_time
                + (self.cur_step_float - change.step_time) / change.steps_per_beat as f64;
            self.cur_measure_float = change.measure_time
                + (self.cur_beat_float - change.beat_time) / change.beats_per_measure;
        } else {
            self.cur_step_float = song_position / self.step_crochet();
            self.cur_beat_float = self.cur_step_float / self.steps_per_beat() as f64;
            self.cur_measure_float = self.cur_beat_float / self.beats_per_measure();
        }

        if self.cur_change_index != old_index {
            match self.map.changes().get(old_index.max(0) as usize) {
                Some(previous) => {
                    let current = self.current_change();
                    if current.beats_per_measure != previous.beats_per_measure
                        || current.steps_per_beat != previous.steps_per_beat
                    {
                        tick.time_signature_change = Some(TimeSignatureSignal {
                            beats_per_measure: current.beats_per_measure,
                            steps_per_beat: current.steps_per_beat,
                        });
                    }
                    if current.bpm != previous.bpm {
                        tick.bpm_change = Some(BpmSignal {
                            bpm: current.bpm,
                            end_song_time: current.end_song_time,
                        });
                    }
                }
                None => {
                    let current = self.current_change();
                    tick.time_signature_change = Some(TimeSignatureSignal {
                        beats_per_measure: current.beats_per_measure,
                        steps_per_beat: current.steps_per_beat,
                    });
                    tick.bpm_change = Some(BpmSignal {
                        bpm: current.bpm,
                        end_song_time: current.end_song_time,
                    });
                }
            }
        }

        let new_step = floor_int(self.cur_step_float);
        if new_step != self.cur_step {
            if new_step > old_step {
                for index in old_step..new_step {
                    tick.steps.push(index + 1);
                }
            } else {
                tick.steps.push(new_step);
            }
            self.cur_step = new_step;
        }

        let new_beat = floor_int(self.cur_beat_float);
        if new_beat != self.cur_beat {
            if new_beat > old_beat {
                for index in old_beat..new_beat {
                    tick.beats.push(index + 1);
                }
            } else {
                tick.beats.push(new_beat);
            }
            self.cur_beat = new_beat;
        }

        let new_measure = floor_int(self.cur_measure_float);
        if new_measure != self.cur_measure {
            if new_measure > old_measure {
                for index in old_measure..new_measure {
                    tick.measures.push(index + 1);
                }
            } else {
                tick.measures.push(new_measure);
            }
            self.cur_measure = new_measure;
        }

        tick
    }
}

impl Default for Conductor {
    fn default() -> Self {
        Self::new(BpmMap::default_map())
    }
}
