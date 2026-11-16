//! Pure gameplay state for the playable prototype.
//!
//! This module is deliberately independent of winit, wgpu and audio so the
//! timing/hit/miss/hold rules can be tested without a window or a sound device.
//!
//! The rules follow Codename where the port can express them:
//!
//! * hit windows come from [`WindowPreset`] (default [`WindowPreset::Default`],
//!   which is also `RatingManager`'s default preset) or an explicit override;
//! * ratings are `sick` / `good` / `bad` / `shit`, scored 300 / 200 / 100 / 50
//!   with accuracy 1.0 / 0.75 / 0.45 / 0.25 and health
//!   `+0.023 / +0.015 / 0 / -0.05`;
//! * a `shit` breaks the combo and counts as a miss, as
//!   `Flags.SHITS_BREAK_COMBO` does;
//! * a note missed past the widest window costs `-0.0475` health;
//! * sustains award health per held step and no score, combo or accuracy, like
//!   Codename's sustain pieces.

use fu_chart::{RuntimeChart, StrumPosition};
pub use fu_timing::HitRating;
use fu_timing::{
    bpm::floor_int, BpmMap, Conductor, HitWindows, Tick, WindowPreset, DEFAULT_BEATS_PER_MEASURE,
    DEFAULT_STEPS_PER_BEAT,
};
use serde::Deserialize;

use crate::opponent::Opponent;

/// Lane count of a plain JSON chart and of the stock four-key UI.
pub const LANES: usize = 4;

/// How early a note becomes visible before its hit time.
pub const APPROACH_MS: f32 = 2_000.0;

pub const HIT_FLASH_MS: f32 = 160.0;
pub const MISS_FLASH_MS: f32 = 180.0;

/// Health the bar starts at, matching the series' half-full start.
pub const START_HEALTH: f32 = 0.5;

/// Codename `RatingManager.initDefaultData` scores.
pub const SICK_SCORE: u32 = 300;
pub const GOOD_SCORE: u32 = 200;
pub const BAD_SCORE: u32 = 100;
pub const SHIT_SCORE: u32 = 50;

/// Codename `RatingManager.initDefaultData` accuracy weights.
pub const SICK_ACCURACY: f32 = 1.0;
pub const GOOD_ACCURACY: f32 = 0.75;
pub const BAD_ACCURACY: f32 = 0.45;
pub const SHIT_ACCURACY: f32 = 0.25;

/// Codename `RatingManager.initDefaultData` health values. `sick` has no
/// explicit value there, so `addRating`'s `0.023` default applies.
pub const SICK_HEALTH: f32 = 0.023;
pub const GOOD_HEALTH: f32 = 0.015;
pub const BAD_HEALTH: f32 = 0.0;
pub const SHIT_HEALTH: f32 = -0.05;

/// Health lost by a missed note (`PlayState`'s `-0.0475` for a real note).
pub const MISS_HEALTH: f32 = -0.0475;

/// Health gained per sustain step held, matching one sustain piece's rating
/// health in Codename.
pub const SUSTAIN_HEALTH_PER_STEP: f32 = 0.023;

/// Score a rating is worth.
pub fn rating_score(rating: HitRating) -> u32 {
    match rating {
        HitRating::Sick => SICK_SCORE,
        HitRating::Good => GOOD_SCORE,
        HitRating::Bad => BAD_SCORE,
        HitRating::Shit => SHIT_SCORE,
        HitRating::Miss => 0,
    }
}

/// Accuracy weight a rating contributes.
pub fn rating_accuracy(rating: HitRating) -> f32 {
    match rating {
        HitRating::Sick => SICK_ACCURACY,
        HitRating::Good => GOOD_ACCURACY,
        HitRating::Bad => BAD_ACCURACY,
        HitRating::Shit => SHIT_ACCURACY,
        HitRating::Miss => 0.0,
    }
}

/// Health change a rating applies.
pub fn rating_health(rating: HitRating) -> f32 {
    match rating {
        HitRating::Sick => SICK_HEALTH,
        HitRating::Good => GOOD_HEALTH,
        HitRating::Bad => BAD_HEALTH,
        HitRating::Shit => SHIT_HEALTH,
        HitRating::Miss => MISS_HEALTH,
    }
}

/// Which side of the stage a chart strum line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrumSlot {
    Player,
    Opponent,
    Girlfriend,
}

impl From<StrumPosition> for StrumSlot {
    fn from(position: StrumPosition) -> Self {
        match position {
            StrumPosition::Player => Self::Player,
            StrumPosition::Opponent => Self::Opponent,
            StrumPosition::Girlfriend => Self::Girlfriend,
            StrumPosition::Other => Self::Opponent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ChartNote {
    /// Hit time in milliseconds, at the precision the chart authored.
    pub time_ms: f64,
    /// Lane inside its strum line.
    pub lane: u8,
    /// Sustain length in milliseconds; `0` for a tap note.
    #[serde(default)]
    pub sustain_ms: f64,
}

impl ChartNote {
    /// A tap note.
    pub fn tap(time_ms: f64, lane: u8) -> Self {
        Self {
            time_ms,
            lane,
            sustain_ms: 0.0,
        }
    }

    /// A note held until `time_ms + sustain_ms`.
    pub fn held(time_ms: f64, lane: u8, sustain_ms: f64) -> Self {
        Self {
            time_ms,
            lane,
            sustain_ms: sustain_ms.max(0.0),
        }
    }

    /// End of the hold, equal to [`Self::time_ms`] for a tap.
    pub fn end_ms(&self) -> f64 {
        self.time_ms + self.sustain_ms.max(0.0)
    }
}

/// A non-player strum line kept for opponent/girlfriend simulation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChartStrumLine {
    pub slot: StrumSlot,
    pub key_count: u8,
    pub notes: Vec<ChartNote>,
}

/// A chart event, with the step/beat/measure it lands on resolved once.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChartEvent {
    pub time_ms: f64,
    pub name: String,
    pub step: i64,
    pub beat: i64,
    pub measure: i64,
}

/// The gameplay view of a RuntimeChart.
///
/// `timing` is the single timing source: note times, the tempo the conductor
/// reports, step/beat/measure signals and (from the same map) step positions
/// all come from it. `bpm` is retained as the starting tempo for display.
///
/// `lanes` is the player line's key count, so nothing in gameplay assumes four
/// lanes. `preset` is always explicit on a loaded chart; `windows` overrides it
/// when a chart or mod supplies custom values.
#[derive(Debug, Clone, Deserialize)]
pub struct Chart {
    pub song: String,
    #[serde(default = "default_bpm")]
    pub bpm: f32,
    pub length_ms: u32,
    pub notes: Vec<ChartNote>,
    #[serde(default = "default_timing")]
    pub timing: BpmMap,
    #[serde(default = "default_lanes")]
    pub lanes: usize,
    #[serde(default = "default_preset")]
    pub preset: WindowPreset,
    #[serde(default)]
    pub windows: Option<HitWindows>,
    #[serde(default)]
    pub events: Vec<ChartEvent>,
    #[serde(default)]
    pub strum_lines: Vec<ChartStrumLine>,
}

fn default_bpm() -> f32 {
    120.0
}

fn default_lanes() -> usize {
    LANES
}

/// Codename's `RatingManager` uses `WindowPreset.DEFAULT` when no preset is
/// passed, so that is the explicit default here too.
fn default_preset() -> WindowPreset {
    WindowPreset::Default
}

fn default_timing() -> BpmMap {
    BpmMap::new(
        default_bpm() as f64,
        DEFAULT_BEATS_PER_MEASURE,
        DEFAULT_STEPS_PER_BEAT,
    )
}

impl Chart {
    /// Builds a constant-tempo, four-lane chart from gameplay notes.
    pub fn new(song: impl Into<String>, bpm: f32, length_ms: u32, notes: Vec<ChartNote>) -> Self {
        Self::with_timing(
            song,
            length_ms,
            notes,
            BpmMap::new(
                bpm as f64,
                DEFAULT_BEATS_PER_MEASURE,
                DEFAULT_STEPS_PER_BEAT,
            ),
        )
    }

    /// Builds a chart whose timing comes from `timing`.
    pub fn with_timing(
        song: impl Into<String>,
        length_ms: u32,
        mut notes: Vec<ChartNote>,
        timing: BpmMap,
    ) -> Self {
        sort_notes(&mut notes);
        let bpm = timing.changes()[0].bpm as f32;
        Self {
            song: song.into(),
            bpm,
            length_ms,
            notes,
            timing,
            lanes: default_lanes(),
            preset: default_preset(),
            windows: None,
            events: Vec::new(),
            strum_lines: Vec::new(),
        }
    }

    /// Projects a runtime chart onto the gameplay view.
    ///
    /// The first player strum line is the one gameplay judges; any further
    /// player (co-op) lines stay in the runtime chart and are not judged here.
    /// Opponent and girlfriend lines become [`Chart::strum_lines`] so the
    /// opponent simulator can play them.
    pub fn from_runtime(runtime: &RuntimeChart, length_ms: u32) -> Self {
        let (notes, lanes) = match runtime.player_line() {
            Some(line) => (
                line.notes
                    .iter()
                    .map(|note| ChartNote {
                        time_ms: note.time_ms,
                        lane: note.lane,
                        sustain_ms: note.sustain_ms,
                    })
                    .collect(),
                (line.key_count as usize).max(1),
            ),
            None => (Vec::new(), default_lanes()),
        };

        let events = runtime
            .events
            .iter()
            .map(|event| ChartEvent {
                time_ms: event.time_ms,
                name: event.name.clone(),
                step: floor_int(runtime.timing.steps_at_time(event.time_ms)),
                beat: floor_int(runtime.timing.beats_at_time(event.time_ms)),
                measure: floor_int(runtime.timing.measures_at_time(event.time_ms)),
            })
            .collect();

        let strum_lines = runtime
            .strum_lines
            .iter()
            .filter(|line| !line.is_player())
            .map(|line| ChartStrumLine {
                slot: StrumSlot::from(line.position),
                key_count: line.key_count,
                notes: line
                    .notes
                    .iter()
                    .map(|note| ChartNote {
                        time_ms: note.time_ms,
                        lane: note.lane,
                        sustain_ms: note.sustain_ms,
                    })
                    .collect(),
            })
            .collect();

        let mut chart = Self::with_timing(
            runtime.song.clone(),
            length_ms,
            notes,
            runtime.timing.clone(),
        );
        chart.lanes = lanes;
        chart.events = events;
        chart.strum_lines = strum_lines;
        chart
    }

    /// Overrides the player lane count.
    pub fn with_lanes(mut self, lanes: usize) -> Self {
        self.lanes = lanes.max(1);
        self
    }

    /// Selects a Codename judgement preset.
    pub fn with_preset(mut self, preset: WindowPreset) -> Self {
        self.preset = preset;
        self.windows = None;
        self
    }

    /// Overrides the preset with explicit windows.
    pub fn with_windows(mut self, windows: HitWindows) -> Self {
        self.windows = Some(windows);
        self
    }

    /// Replaces the chart events (already step-resolved).
    pub fn with_events(mut self, events: Vec<ChartEvent>) -> Self {
        self.events = events;
        self
    }

    /// Replaces the non-player strum lines.
    pub fn with_strum_lines(mut self, strum_lines: Vec<ChartStrumLine>) -> Self {
        self.strum_lines = strum_lines;
        self
    }

    /// The timing map every consumer reads.
    pub fn timing(&self) -> &BpmMap {
        &self.timing
    }

    /// The judgement windows in force: an explicit override, else the preset.
    pub fn windows(&self) -> HitWindows {
        self.windows.unwrap_or_else(|| self.preset.windows())
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("chart JSON: {e}"))?;
        let mut chart: Chart =
            serde_json::from_value(value.clone()).map_err(|e| format!("chart JSON: {e}"))?;

        // A plain JSON chart declares a single `bpm`; a chart that carries a
        // `timing` map is already authoritative and supplies the display tempo.
        if value
            .as_object()
            .map(|object| object.contains_key("timing"))
            .unwrap_or(false)
        {
            chart.bpm = chart.timing.changes()[0].bpm as f32;
        } else {
            chart.timing = BpmMap::new(
                chart.bpm as f64,
                DEFAULT_BEATS_PER_MEASURE,
                DEFAULT_STEPS_PER_BEAT,
            );
        }

        if chart.song.trim().is_empty() {
            return Err("chart song name is empty".into());
        }
        if !chart.bpm.is_finite() || chart.bpm <= 0.0 {
            return Err("chart bpm must be positive and finite".into());
        }
        if chart.length_ms == 0 {
            return Err("chart length_ms must be positive".into());
        }
        if !(1..=32).contains(&chart.lanes) {
            return Err("chart lanes must be between 1 and 32".into());
        }
        if chart.notes.iter().any(|note| {
            !note.time_ms.is_finite()
                || note.time_ms < 0.0
                || !note.sustain_ms.is_finite()
                || note.sustain_ms < 0.0
        }) {
            return Err("chart note times must be finite and non-negative".into());
        }
        if chart
            .notes
            .iter()
            .any(|note| note.lane as usize >= chart.lanes)
        {
            return Err(format!(
                "chart contains a lane outside 0..{}",
                chart.lanes.saturating_sub(1)
            ));
        }
        sort_notes(&mut chart.notes);
        Ok(chart)
    }

    pub fn duration_ms(&self) -> f32 {
        self.length_ms as f32
    }
}

fn sort_notes(notes: &mut [ChartNote]) {
    notes.sort_by(|left, right| {
        left.time_ms
            .total_cmp(&right.time_ms)
            .then(left.lane.cmp(&right.lane))
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgement {
    Pending,
    Hit(HitRating),
    Miss,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitEvent {
    pub lane: usize,
    pub rating: HitRating,
    pub note_time_ms: f32,
    pub sustain_ms: f32,
    /// Index into [`Chart::notes`] the press landed on.
    pub note_index: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Feedback {
    pub lane: usize,
    pub until_ms: f32,
    pub rating: Option<HitRating>,
}

/// State of a sustain hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldState {
    /// The head was hit and the lane is still down.
    Holding,
    /// The lane was held to the end of the sustain.
    Completed,
    /// The lane was released before the sustain ended.
    ReleasedEarly,
}

/// One started sustain hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    /// Index into [`Chart::notes`].
    pub note: usize,
    pub lane: usize,
    pub start_ms: f64,
    pub end_ms: f64,
    pub state: HoldState,
    /// Steps crossed while the lane was held.
    pub held_steps: u32,
}

/// Result of releasing a lane that was holding a sustain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldRelease {
    pub note: usize,
    pub lane: usize,
    pub state: HoldState,
    pub held_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Clone)]
pub struct Gameplay {
    chart: Chart,
    judged: Vec<Judgement>,
    score: u32,
    combo: u32,
    max_combo: u32,
    hits: u32,
    misses: u32,
    accuracy_total: f32,
    accuracy_pressed: u32,
    health: f32,
    hit_feedback: Option<Feedback>,
    miss_feedback: Option<Feedback>,
    conductor: Conductor,
    tick: Tick,
    holds: Vec<Hold>,
    active_hold: Vec<Option<usize>>,
    held: Vec<bool>,
    fired_events: Vec<usize>,
    event_fired: Vec<bool>,
    opponent: Opponent,
}

impl Gameplay {
    pub fn new(chart: Chart) -> Self {
        let judged = vec![Judgement::Pending; chart.notes.len()];
        let conductor = Conductor::new(chart.timing.clone());
        let lanes = chart.lanes.max(1);
        let opponent = Opponent::new(&chart);
        let event_fired = vec![false; chart.events.len()];
        Self {
            chart,
            judged,
            score: 0,
            combo: 0,
            max_combo: 0,
            hits: 0,
            misses: 0,
            accuracy_total: 0.0,
            accuracy_pressed: 0,
            health: START_HEALTH,
            hit_feedback: None,
            miss_feedback: None,
            conductor,
            tick: Tick::default(),
            holds: Vec::new(),
            active_hold: vec![None; lanes],
            held: vec![false; lanes],
            fired_events: Vec::new(),
            event_fired,
            opponent,
        }
    }

    pub fn chart(&self) -> &Chart {
        &self.chart
    }

    pub fn notes(&self) -> &[ChartNote] {
        &self.chart.notes
    }

    pub fn lanes(&self) -> usize {
        self.chart.lanes
    }

    pub fn judgement(&self, index: usize) -> Judgement {
        self.judged[index]
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn max_combo(&self) -> u32 {
        self.max_combo
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// Current health in `0..=1`, driving the health bar.
    pub fn health(&self) -> f32 {
        self.health
    }

    pub fn hit_feedback(&self) -> Option<Feedback> {
        self.hit_feedback
    }

    pub fn miss_feedback(&self) -> Option<Feedback> {
        self.miss_feedback
    }

    /// Every hold this playthrough has started or finished.
    pub fn holds(&self) -> &[Hold] {
        &self.holds
    }

    /// The hold currently active on `lane`, if any.
    pub fn active_hold(&self, lane: usize) -> Option<&Hold> {
        self.active_hold
            .get(lane)
            .copied()
            .flatten()
            .and_then(|index| self.holds.get(index))
    }

    /// Events dispatched by the last [`Self::update`], as indices into
    /// [`Chart::events`].
    pub fn fired_events(&self) -> &[usize] {
        &self.fired_events
    }

    /// Opponent notes the simulator hit during the last [`Self::update`].
    pub fn opponent_hits(&self) -> &[crate::opponent::OpponentHit] {
        self.opponent.last_hits()
    }

    /// The opponent simulator, for callers that want its full state.
    pub fn opponent(&self) -> &Opponent {
        &self.opponent
    }

    /// Records that `lane` is held down.
    pub fn set_held(&mut self, lane: usize, held: bool) {
        if let Some(slot) = self.held.get_mut(lane) {
            *slot = held;
        }
    }

    /// Attempts to hit the closest pending note in `lane` at `song_ms`.
    ///
    /// Returns `None` for an empty lane, a lane outside the chart, a note
    /// outside the widest (`shit`) window, or an already judged note. A press
    /// with no hittable note is a harmless ghost tap.
    pub fn press(&mut self, lane: usize, song_ms: f32) -> Option<HitEvent> {
        if lane >= self.chart.lanes {
            return None;
        }
        self.set_held(lane, true);

        let windows = self.chart.windows();
        let mut best: Option<(usize, f64)> = None;
        for (index, note) in self.chart.notes.iter().enumerate() {
            if note.lane as usize != lane || self.judged[index] != Judgement::Pending {
                continue;
            }
            let delta = (note.time_ms - song_ms as f64).abs();
            if delta <= windows.shit && best.is_none_or(|(_, best_delta)| delta < best_delta) {
                best = Some((index, delta));
            }
        }

        let (index, delta) = best?;
        let rating = windows.rating_for(delta);
        if rating == HitRating::Miss {
            return None;
        }

        self.judged[index] = Judgement::Hit(rating);
        self.score = self.score.saturating_add(rating_score(rating));
        self.hits = self.hits.saturating_add(1);
        self.accuracy_total += rating_accuracy(rating);
        self.accuracy_pressed = self.accuracy_pressed.saturating_add(1);
        self.health = (self.health + rating_health(rating)).clamp(0.0, 1.0);

        // Codename's `SHITS_BREAK_COMBO`: a shit rating counts as a miss and
        // breaks the combo even though the note was hit.
        if rating == HitRating::Shit {
            self.misses = self.misses.saturating_add(1);
            self.combo = 0;
        } else {
            self.combo = self.combo.saturating_add(1);
            self.max_combo = self.max_combo.max(self.combo);
        }

        self.hit_feedback = Some(Feedback {
            lane,
            until_ms: song_ms + HIT_FLASH_MS,
            rating: Some(rating),
        });

        let note = self.chart.notes[index];
        if note.sustain_ms > 0.0 && self.active_hold[lane].is_none() {
            let hold_index = self.holds.len();
            self.holds.push(Hold {
                note: index,
                lane,
                start_ms: note.time_ms,
                end_ms: note.end_ms(),
                state: HoldState::Holding,
                held_steps: 0,
            });
            self.active_hold[lane] = Some(hold_index);
        }

        Some(HitEvent {
            lane,
            rating,
            note_time_ms: note.time_ms as f32,
            sustain_ms: note.sustain_ms as f32,
            note_index: index,
        })
    }

    /// Releases `lane`, finishing any sustain it was holding.
    ///
    /// `None` when the lane was not holding (or was not down). Releasing before
    /// the sustain end records [`HoldState::ReleasedEarly`]; releasing at or
    /// after it records [`HoldState::Completed`]. Neither costs health, which
    /// matches Codename dropping the remaining sustain pieces.
    pub fn release(&mut self, lane: usize, song_ms: f32) -> Option<HoldRelease> {
        self.set_held(lane, false);
        let index = self.active_hold.get_mut(lane)?.take()?;
        let hold = &mut self.holds[index];
        let at = song_ms as f64;
        hold.state = if at >= hold.end_ms {
            HoldState::Completed
        } else {
            HoldState::ReleasedEarly
        };
        Some(HoldRelease {
            note: hold.note,
            lane,
            state: hold.state,
            held_ms: (at - hold.start_ms).max(0.0),
            total_ms: (hold.end_ms - hold.start_ms).max(0.0),
        })
    }

    /// Applies miss detection, sustain ticks, chart events and opponent notes
    /// for the current song position.
    ///
    /// Returns the number of notes newly missed. A miss resets the combo.
    pub fn update(&mut self, song_ms: f32) -> usize {
        // One clock feeds judgement, sustain ticks and step/beat signals.
        self.tick = self.conductor.update(song_ms as f64);

        // Chart events fire on the step codename would dispatch them from.
        self.fired_events.clear();
        for (index, event) in self.chart.events.iter().enumerate() {
            if self.event_fired[index] {
                continue;
            }
            if self.tick.steps.contains(&event.step) {
                self.event_fired[index] = true;
                self.fired_events.push(index);
            }
        }

        // Sustain health is awarded per crossed step, so a hold that spans a
        // BPM change keeps lining up with the chart's own step grid.
        let step_times: Vec<f64> = self
            .tick
            .steps
            .iter()
            .map(|step| self.conductor.map().time_at_steps(*step as f64))
            .collect();
        for lane in 0..self.active_hold.len() {
            let Some(index) = self.active_hold[lane] else {
                continue;
            };
            let (start_ms, end_ms) = (self.holds[index].start_ms, self.holds[index].end_ms);
            if self.held[lane] {
                let mut held = 0;
                for time in &step_times {
                    if *time > start_ms && *time <= end_ms {
                        held += 1;
                    }
                }
                if held > 0 {
                    self.holds[index].held_steps =
                        self.holds[index].held_steps.saturating_add(held);
                    self.health =
                        (self.health + held as f32 * SUSTAIN_HEALTH_PER_STEP).clamp(0.0, 1.0);
                }
            }
            if song_ms as f64 >= end_ms {
                self.holds[index].state = HoldState::Completed;
                self.active_hold[lane] = None;
            }
        }

        let windows = self.chart.windows();
        let mut missed_now = 0;
        for (index, note) in self.chart.notes.iter().enumerate() {
            if self.judged[index] != Judgement::Pending {
                continue;
            }
            if song_ms as f64 - note.time_ms >= windows.shit {
                self.judged[index] = Judgement::Miss;
                self.misses = self.misses.saturating_add(1);
                self.combo = 0;
                self.health = (self.health + MISS_HEALTH).clamp(0.0, 1.0);
                self.miss_feedback = Some(Feedback {
                    lane: note.lane as usize,
                    until_ms: song_ms + MISS_FLASH_MS,
                    rating: None,
                });
                missed_now += 1;
            }
        }

        self.opponent.update(song_ms as f64);
        missed_now
    }

    pub fn all_judged(&self) -> bool {
        self.judged
            .iter()
            .all(|judgement| *judgement != Judgement::Pending)
    }

    pub fn is_complete(&self, song_ms: f32) -> bool {
        self.all_judged() && song_ms >= self.chart.duration_ms()
    }

    pub fn progress(&self, song_ms: f32) -> f32 {
        (song_ms / self.chart.duration_ms()).clamp(0.0, 1.0)
    }

    /// The timing map this chart is judged against.
    pub fn timing(&self) -> &BpmMap {
        self.chart.timing()
    }

    /// The judgement windows in force.
    pub fn windows(&self) -> HitWindows {
        self.chart.windows()
    }

    /// The conductor advanced by the last [`Self::update`].
    pub fn conductor(&self) -> &Conductor {
        &self.conductor
    }

    /// Signals the last [`Self::update`] dispatched: step, beat and measure
    /// hits in Codename's order, plus tempo and time-signature changes.
    ///
    /// Receptor and character animations read `steps`/`beats`/`measures` from
    /// here, so they run on the same clock as judgement.
    pub fn tick(&self) -> &Tick {
        &self.tick
    }

    /// Tempo at the current song position, continuous changes interpolated.
    pub fn bpm(&self) -> f64 {
        self.conductor.current_bpm()
    }

    /// Current integer step.
    pub fn step(&self) -> i64 {
        self.conductor.cur_step()
    }

    /// Current integer beat.
    pub fn beat(&self) -> i64 {
        self.conductor.cur_beat()
    }

    /// Current integer measure.
    pub fn measure(&self) -> i64 {
        self.conductor.cur_measure()
    }

    /// Mean of the accuracy weights of pressed notes, as Codename computes it.
    /// A song with no presses yet reads `1.0`.
    pub fn accuracy(&self) -> f32 {
        if self.accuracy_pressed == 0 {
            1.0
        } else {
            self.accuracy_total / self.accuracy_pressed as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opponent::OpponentHit;
    use fu_timing::{TimingEvent, TimingEventKind};

    fn chart() -> Chart {
        Chart::from_json(
            br#"{
                "song": "Unit Pulse",
                "bpm": 120,
                "length_ms": 4000,
                "notes": [
                    {"time_ms": 2000, "lane": 0},
                    {"time_ms": 2500, "lane": 1},
                    {"time_ms": 3000, "lane": 2}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn chart_parses_sorts_and_validates() {
        let parsed = Chart::from_json(
            br#"{
                "song": "Sorting",
                "bpm": 100,
                "length_ms": 1000,
                "notes": [
                    {"time_ms": 500, "lane": 2},
                    {"time_ms": 100, "lane": 1}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.notes[0].time_ms, 100.0);
        assert_eq!(parsed.notes[1].time_ms, 500.0);
        assert_eq!(parsed.lanes, 4);
        assert_eq!(parsed.preset, WindowPreset::Default);

        assert!(Chart::from_json(
            br#"{"song":"x","length_ms":1000,"notes":[{"time_ms":1,"lane":4}]}"#
        )
        .is_err());
        assert!(Chart::from_json(
            br#"{"song":"x","length_ms":1000,"lanes":6,"notes":[{"time_ms":1,"lane":5}]}"#
        )
        .is_ok());
        assert!(Chart::from_json(br#"{"song":"","length_ms":1000,"notes":[]}"#).is_err());
        assert!(Chart::from_json(br#"{"song":"x","length_ms":0,"notes":[]}"#).is_err());
    }

    #[test]
    fn codename_sick_hit_scores_and_builds_combo() {
        let mut game = Gameplay::new(chart());
        let event = game.press(0, 2020.0).unwrap();
        assert_eq!(event.rating, HitRating::Sick);
        assert_eq!(game.score(), SICK_SCORE);
        assert_eq!(game.combo(), 1);
        assert_eq!(game.accuracy(), 1.0);
        assert_eq!(game.judgement(0), Judgement::Hit(HitRating::Sick));
    }

    #[test]
    fn codename_windows_and_ratings_follow_the_default_preset() {
        let mut game = Gameplay::new(chart());
        // Default preset: 37.8 / 75.6 / 113.4 / 180.
        assert_eq!(game.press(0, 2037.0).unwrap().rating, HitRating::Sick);
        assert_eq!(game.press(1, 2575.0).unwrap().rating, HitRating::Good);
        assert_eq!(game.press(2, 3113.0).unwrap().rating, HitRating::Bad);
        assert_eq!(game.score(), SICK_SCORE + GOOD_SCORE + BAD_SCORE);
        assert_eq!(game.combo(), 3);
        assert!(game.press(2, 3190.0).is_none(), "outside the shit window");

        // A note far enough out is a shit: it scores 50, breaks the combo and
        // counts as a miss (`Flags.SHITS_BREAK_COMBO`).
        let mut game = Gameplay::new(Chart::new(
            "Shit",
            120.0,
            4_000,
            vec![ChartNote::tap(2_000.0, 0)],
        ));
        assert_eq!(game.press(0, 2_150.0).unwrap().rating, HitRating::Shit);
        assert_eq!(game.score(), SHIT_SCORE);
        assert_eq!(game.combo(), 0);
        assert_eq!(game.misses(), 1);
        assert_eq!(game.accuracy(), SHIT_ACCURACY);
    }

    #[test]
    fn an_explicit_preset_or_custom_windows_are_honoured() {
        let cne = Chart::new("Cne", 120.0, 4_000, vec![ChartNote::tap(2_000.0, 0)])
            .with_preset(WindowPreset::CneClassic);
        assert_eq!(cne.windows().sick, 50.0);
        assert_eq!(cne.windows().shit, 250.0);
        let mut game = Gameplay::new(cne);
        assert_eq!(game.press(0, 2_240.0).unwrap().rating, HitRating::Shit);

        let custom = Chart::new("Custom", 120.0, 4_000, vec![ChartNote::tap(2_000.0, 0)])
            .with_windows(HitWindows {
                sick: 10.0,
                good: 20.0,
                bad: 30.0,
                shit: 40.0,
            });
        let mut game = Gameplay::new(custom.clone());
        assert_eq!(game.press(0, 2_010.0).unwrap().rating, HitRating::Sick);
        let mut game = Gameplay::new(custom);
        assert!(game.press(0, 2_040.1).is_none());
    }

    #[test]
    fn misses_cost_health_reset_combo_and_do_not_touch_accuracy() {
        let mut game = Gameplay::new(chart());
        game.press(0, 2000.0).unwrap();
        assert_eq!(game.combo(), 1);
        assert_eq!(game.update(2_700.0), 1);
        assert_eq!(game.misses(), 1);
        assert_eq!(game.combo(), 0);
        assert_eq!(game.judgement(1), Judgement::Miss);
        assert_eq!(game.update(2_700.0), 0);
        assert!((game.health() - (START_HEALTH + SICK_HEALTH + MISS_HEALTH)).abs() < 1e-6);
        assert_eq!(game.accuracy(), 1.0, "misses do not add accuracy samples");
    }

    #[test]
    fn completion_requires_judgement_and_song_length() {
        let mut game = Gameplay::new(chart());
        game.press(0, 2000.0).unwrap();
        game.press(1, 2500.0).unwrap();
        game.press(2, 3000.0).unwrap();
        game.update(3000.0);
        assert!(!game.is_complete(3999.0));
        game.update(4000.0);
        assert!(game.is_complete(4000.0));
        assert_eq!(game.max_combo(), 3);
    }

    #[test]
    fn health_rises_on_hits_and_falls_on_misses() {
        let mut game = Gameplay::new(chart());
        assert_eq!(game.health(), START_HEALTH);

        game.press(0, 2000.0).unwrap();
        assert!((game.health() - (START_HEALTH + SICK_HEALTH)).abs() < 1e-6);

        game.update(2700.0);
        assert!(
            (game.health() - (START_HEALTH + SICK_HEALTH + MISS_HEALTH)).abs() < 1e-6,
            "a miss costs health, got {}",
            game.health()
        );

        let mut collapsing = Gameplay::new(Chart::new(
            "Many Notes",
            120.0,
            5_000,
            (0..30)
                .map(|index| ChartNote::tap(1_000.0, (index % LANES) as u8))
                .collect(),
        ));
        collapsing.update(5_000.0);
        assert_eq!(collapsing.health(), 0.0, "health never goes below zero");
    }

    #[test]
    fn ghost_taps_do_not_change_state() {
        let mut game = Gameplay::new(chart());
        assert!(game.press(3, 2000.0).is_none());
        assert!(game.press(0, 1000.0).is_none());
        assert_eq!(game.score(), 0);
        assert_eq!(game.combo(), 0);
        assert_eq!(game.hits(), 0);
    }

    #[test]
    fn lanes_are_not_hardcoded_to_four() {
        let six = Chart::new(
            "Six Key",
            120.0,
            4_000,
            vec![ChartNote::tap(2_000.0, 5), ChartNote::tap(2_500.0, 0)],
        )
        .with_lanes(6);
        assert_eq!(six.lanes, 6);
        let mut game = Gameplay::new(six);
        assert_eq!(game.lanes(), 6);
        assert!(game.press(6, 2_000.0).is_none(), "lane outside the chart");
        assert_eq!(game.press(5, 2_000.0).unwrap().rating, HitRating::Sick);
        assert_eq!(game.press(0, 2_500.0).unwrap().rating, HitRating::Sick);
    }

    #[test]
    fn a_sustain_hold_tracks_steps_and_release() {
        // 120 bpm: one step every 125 ms.
        let chart = Chart::new(
            "Hold",
            120.0,
            4_000,
            vec![ChartNote::held(1_000.0, 0, 1_000.0)],
        );
        let mut game = Gameplay::new(chart);
        game.press(0, 1_000.0).expect("head hit");
        assert_eq!(game.active_hold(0).unwrap().state, HoldState::Holding);

        game.update(1_500.0);
        let hold = *game.active_hold(0).unwrap();
        assert_eq!(hold.held_steps, 4, "1125, 1250, 1375, 1500");
        assert!(
            (game.health() - (START_HEALTH + SICK_HEALTH + 4.0 * SUSTAIN_HEALTH_PER_STEP)).abs()
                < 1e-6
        );

        let release = game.release(0, 1_500.0).expect("release");
        assert_eq!(release.state, HoldState::ReleasedEarly);
        assert_eq!(release.held_ms, 500.0);
        assert_eq!(release.total_ms, 1_000.0);
        assert!(game.release(0, 1_600.0).is_none(), "already released");
    }

    #[test]
    fn holding_to_the_end_completes_the_sustain() {
        let chart = Chart::new(
            "Hold",
            120.0,
            4_000,
            vec![ChartNote::held(1_000.0, 0, 1_000.0)],
        );
        let mut game = Gameplay::new(chart);
        game.press(0, 1_000.0).unwrap();
        game.update(2_000.0);
        assert_eq!(game.active_hold(0), None, "hold finished at its end");
        assert_eq!(game.holds()[0].state, HoldState::Completed);
        assert_eq!(game.holds()[0].end_ms, 2_000.0);
    }

    #[test]
    fn a_sustain_across_a_bpm_change_keeps_its_end_and_uses_the_new_step_grid() {
        // 100 bpm until 1000 ms, then 200 bpm; the sustain spans both.
        let timing = BpmMap::from_timing_events(
            100.0,
            4.0,
            4,
            &[TimingEvent {
                time: 1_000.0,
                kind: TimingEventKind::BpmChange { bpm: 200.0 },
            }],
        );
        let chart = Chart::with_timing(
            "Ramp Hold",
            4_000,
            vec![ChartNote::held(500.0, 0, 2_000.0)],
            timing.clone(),
        );
        let mut game = Gameplay::new(chart);
        game.press(0, 500.0).unwrap();
        game.update(2_500.0);
        let hold = game.holds()[0];
        assert_eq!(
            hold.end_ms, 2_500.0,
            "timing is preserved across the change"
        );
        assert_eq!(hold.state, HoldState::Completed);

        let mut expected = 0;
        let last = timing.steps_at_time(2_500.0) as i64;
        for step in 1..=last {
            let time = timing.time_at_steps(step as f64);
            if time > 500.0 && time <= 2_500.0 {
                expected += 1;
            }
        }
        assert_eq!(hold.held_steps as i64, expected);
        let unclamped = START_HEALTH + SICK_HEALTH + expected as f32 * SUSTAIN_HEALTH_PER_STEP;
        assert!(
            (game.health() - unclamped.min(1.0)).abs() < 1e-5,
            "health {} follows {} held steps",
            game.health(),
            expected
        );
    }

    #[test]
    fn chart_events_fire_once_on_their_step() {
        let mut game =
            Gameplay::new(
                Chart::new("Cues", 120.0, 4_000, vec![]).with_events(vec![ChartEvent {
                    time_ms: 1_000.0,
                    name: "Camera Zoom".to_string(),
                    step: 8,
                    beat: 2,
                    measure: 0,
                }]),
            );
        game.update(900.0);
        assert!(game.fired_events().is_empty());
        game.update(1_000.0);
        assert_eq!(game.fired_events(), &[0]);
        assert!(game.tick().steps.contains(&8));
        game.update(1_100.0);
        assert!(game.fired_events().is_empty(), "an event fires once");
    }

    #[test]
    fn opponent_lines_are_simulated_without_touching_player_state() {
        let chart = Chart::new("Versus", 120.0, 4_000, vec![ChartNote::tap(2_000.0, 0)])
            .with_strum_lines(vec![ChartStrumLine {
                slot: StrumSlot::Opponent,
                key_count: 4,
                notes: vec![ChartNote::tap(500.0, 2), ChartNote::tap(750.0, 1)],
            }]);
        let mut game = Gameplay::new(chart);

        game.update(500.0);
        let hits: &[OpponentHit] = game.opponent_hits();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].lane, 2);
        assert_eq!(hits[0].slot, StrumSlot::Opponent);

        game.update(750.0);
        assert_eq!(game.opponent_hits().len(), 1);
        assert_eq!(game.opponent_hits()[0].lane, 1);

        assert_eq!(game.hits(), 0, "opponent notes are not player hits");
        assert_eq!(game.score(), 0);
        assert_eq!(game.judgement(0), Judgement::Pending);
        assert_eq!(game.opponent().total_hits(), 2);
    }

    #[test]
    fn a_whole_chart_can_be_hit_at_perfect_timing() {
        let chart = chart();
        let mut game = Gameplay::new(chart.clone());
        for note in &chart.notes {
            game.update(note.time_ms as f32);
            game.press(note.lane as usize, note.time_ms as f32);
        }
        game.update(4_000.0);
        assert!(game.is_complete(4_000.0));
        assert_eq!(game.hits(), 3);
        assert_eq!(game.misses(), 0);
        assert_eq!(game.max_combo(), 3);
        assert_eq!(game.accuracy(), 1.0);
        assert_eq!(game.score(), 3 * SICK_SCORE);
    }
}
