//! Play-session state above the gameplay core.
//!
//! [`Session`] owns the judgement state and a screen state, and is driven by a
//! song position supplied by the caller. Keeping the clock outside means the
//! same logic runs behind real audio in the windowed runner and deterministically
//! in tests, with no second implementation of the timing rules.

use crate::game::{Chart, Gameplay, HitEvent, Hold, HoldRelease};
use crate::visuals::Screen;

/// Result of advancing a session to a new song position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionUpdate {
    /// Notes newly judged as missed by this advance.
    pub misses: usize,
    /// True when every note is judged and the song reached its length.
    pub completed: bool,
}

/// Result of simulating a whole chart without a window or audio device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationResult {
    /// Notes hit.
    pub hits: u32,
    /// Notes missed.
    pub misses: u32,
    /// Final score.
    pub score: u32,
    /// Highest combo reached.
    pub max_combo: u32,
    /// True when the chart ran to completion.
    pub completed: bool,
    /// Song position reached.
    pub position_ms: f32,
}

/// Gameplay state for one song playthrough.
#[derive(Debug, Clone)]
pub struct Session {
    game: Gameplay,
    screen: Screen,
    position_ms: f32,
}

impl Session {
    /// Creates a session for `chart`, starting on the loading screen.
    pub fn new(chart: Chart) -> Self {
        Self {
            game: Gameplay::new(chart),
            screen: Screen::Loading,
            position_ms: 0.0,
        }
    }

    /// Begins judging. Safe to call once the first frame is ready.
    pub fn start(&mut self) {
        if matches!(self.screen, Screen::Loading) {
            self.screen = Screen::Playing;
        }
    }

    /// Creates a fresh session for a new attempt at the same chart.
    pub fn restart(&mut self) {
        let chart = self.game.chart().clone();
        *self = Self::new(chart);
        self.start();
    }

    /// Judgement state, for rendering and score display.
    pub fn game(&self) -> &Gameplay {
        &self.game
    }

    /// Current screen.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// Last song position the session advanced to.
    pub fn position_ms(&self) -> f32 {
        self.position_ms
    }

    /// The timing map the session judges against.
    pub fn timing(&self) -> &fu_timing::BpmMap {
        self.game.timing()
    }

    /// Signals the last advance dispatched (steps, beats, measures, changes).
    pub fn tick(&self) -> &fu_timing::Tick {
        self.game.tick()
    }

    /// Tempo at the current position, continuous changes interpolated.
    pub fn bpm(&self) -> f64 {
        self.game.bpm()
    }

    /// Current integer step.
    pub fn step(&self) -> i64 {
        self.game.step()
    }

    /// Current integer beat.
    pub fn beat(&self) -> i64 {
        self.game.beat()
    }

    /// Current integer measure.
    pub fn measure(&self) -> i64 {
        self.game.measure()
    }

    /// Player lane count for this chart.
    pub fn lanes(&self) -> usize {
        self.game.lanes()
    }

    /// Judgement windows in force.
    pub fn windows(&self) -> fu_timing::HitWindows {
        self.game.windows()
    }

    /// Sustain holds started by this playthrough.
    pub fn holds(&self) -> &[Hold] {
        self.game.holds()
    }

    /// Chart events fired by the last advance, as indices into the chart.
    pub fn fired_events(&self) -> &[usize] {
        self.game.fired_events()
    }

    /// Opponent notes played by the last advance.
    pub fn opponent_hits(&self) -> &[crate::opponent::OpponentHit] {
        self.game.opponent_hits()
    }

    /// Replaces the screen state, for instance `Complete` after the song ends.
    pub fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
    }

    /// Pauses or resumes. Returns the new paused state.
    ///
    /// Pausing is only meaningful while playing; a completed song stays complete.
    pub fn toggle_pause(&mut self) -> bool {
        match self.screen {
            Screen::Playing => {
                self.screen = Screen::Paused;
                true
            }
            Screen::Paused => {
                self.screen = Screen::Playing;
                false
            }
            _ => matches!(self.screen, Screen::Paused),
        }
    }

    /// Registers a lane press at the current song position.
    ///
    /// Presses outside a playing screen are ignored, so a paused game cannot
    /// judge notes.
    pub fn press(&mut self, lane: usize) -> Option<HitEvent> {
        if self.screen != Screen::Playing {
            return None;
        }
        self.game.press(lane, self.position_ms)
    }

    /// Registers a lane release, finishing any sustain it was holding.
    ///
    /// Only a playing screen tracks holds, matching [`Self::press`].
    pub fn release(&mut self, lane: usize) -> Option<HoldRelease> {
        if self.screen != Screen::Playing {
            return None;
        }
        self.game.release(lane, self.position_ms)
    }

    /// Advances to `song_ms`, judging notes that fell out of the late window.
    ///
    /// The position never moves backwards, so a caller cannot rewind judgement
    /// by restarting the audio clock without going through [`Session::restart`].
    pub fn advance(&mut self, song_ms: f32) -> SessionUpdate {
        if !matches!(self.screen, Screen::Playing | Screen::Paused) {
            return SessionUpdate {
                misses: 0,
                completed: false,
            };
        }
        self.position_ms = self.position_ms.max(song_ms);
        let misses = self.game.update(self.position_ms);
        let completed = self.game.is_complete(self.position_ms);
        if completed {
            self.screen = Screen::Complete;
        }
        SessionUpdate { misses, completed }
    }
}

/// Plays `chart` start to finish without a window or audio device.
///
/// With `autoplay` every note is pressed exactly on time, which is how the
/// engine's own path is exercised in tests: it must produce a complete song with
/// no misses. Without it, nothing is pressed and every note must be missed.
pub fn simulate(chart: &Chart, autoplay: bool) -> SimulationResult {
    let mut session = Session::new(chart.clone());
    session.start();

    if autoplay {
        for note in &chart.notes {
            session.advance(note.time_ms as f32);
            session.press(note.lane as usize);
            if note.sustain_ms > 0.0 {
                // Hold through the sustain, then release on its last frame.
                session.advance(note.end_ms() as f32);
                session.release(note.lane as usize);
            }
        }
    }

    session.advance(chart.duration_ms());
    let game = session.game();
    SimulationResult {
        hits: game.hits(),
        misses: game.misses(),
        score: game.score(),
        max_combo: game.max_combo(),
        completed: session.screen() == Screen::Complete,
        position_ms: session.position_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::ChartNote;

    fn chart() -> Chart {
        Chart::new(
            "Session",
            120.0,
            3_000,
            vec![ChartNote::tap(1_000.0, 0), ChartNote::tap(1_500.0, 2)],
        )
    }

    #[test]
    fn a_started_session_judges_presses() {
        let mut session = Session::new(chart());
        assert_eq!(session.screen(), Screen::Loading);
        assert!(session.press(0).is_none(), "loading does not judge");

        session.start();
        session.advance(1_000.0);
        assert!(session.press(0).is_some());
        assert_eq!(session.game().hits(), 1);
    }

    #[test]
    fn the_position_never_moves_backwards() {
        let mut session = Session::new(chart());
        session.start();
        session.advance(2_000.0);
        session.advance(500.0);
        assert_eq!(session.position_ms(), 2_000.0);
        assert_eq!(
            session.game().misses(),
            2,
            "both notes are past their late window at 2000 ms"
        );
    }

    #[test]
    fn pausing_blocks_judgement() {
        let mut session = Session::new(chart());
        session.start();
        session.advance(1_000.0);
        assert!(session.toggle_pause());
        assert!(session.press(1).is_none(), "paused sessions ignore presses");
        assert!(!session.toggle_pause());
    }

    #[test]
    fn autoplay_completes_without_misses() {
        let result = simulate(&chart(), true);
        assert!(result.completed);
        assert_eq!(result.hits, 2);
        assert_eq!(result.misses, 0);
        assert_eq!(result.max_combo, 2);
    }

    #[test]
    fn silence_misses_every_note() {
        let result = simulate(&chart(), false);
        assert!(result.completed);
        assert_eq!(result.hits, 0);
        assert_eq!(result.misses, 2);
        assert_eq!(result.score, 0);
    }
}
