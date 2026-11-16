//! Opponent and girlfriend strum line simulation.
//!
//! Codename's CPU strum lines play their notes without judging the player.
//! [`Opponent`] mirrors that: each note becomes an [`OpponentHit`] when the
//! song position reaches it, with no score, accuracy or combo effect. The
//! player's own lines are never fed here, so player/opponent separation is
//! structural rather than a flag checked at scoring time.
//!
//! The simulator only moves forward, matching gameplay's monotonic clock: a
//! backwards seek does not replay opponent notes.

use crate::game::{Chart, StrumSlot};

/// One opponent note the simulator played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpponentHit {
    /// Index into [`Chart::strum_lines`].
    pub line: usize,
    /// Which side the line belongs to.
    pub slot: StrumSlot,
    /// Lane inside that line.
    pub lane: u8,
    /// Note time in milliseconds.
    pub time_ms: f64,
    /// Sustain length in milliseconds.
    pub sustain_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Opponent {
    notes: Vec<OpponentHit>,
    next: usize,
    last_hits: Vec<OpponentHit>,
    total_hits: u32,
}

impl Opponent {
    /// Collects every non-player note of `chart`, in time order.
    pub fn new(chart: &Chart) -> Self {
        let mut notes = Vec::new();
        for (line, strum_line) in chart.strum_lines.iter().enumerate() {
            if strum_line.slot == StrumSlot::Player {
                continue;
            }
            for note in &strum_line.notes {
                notes.push(OpponentHit {
                    line,
                    slot: strum_line.slot,
                    lane: note.lane,
                    time_ms: note.time_ms,
                    sustain_ms: note.sustain_ms,
                });
            }
        }
        notes.sort_by(|left, right| {
            left.time_ms
                .total_cmp(&right.time_ms)
                .then(left.lane.cmp(&right.lane))
        });
        Self {
            notes,
            next: 0,
            last_hits: Vec::new(),
            total_hits: 0,
        }
    }

    /// Plays every note at or before `song_ms`. Returns how many were hit.
    pub fn update(&mut self, song_ms: f64) -> usize {
        self.last_hits.clear();
        while self.next < self.notes.len() && self.notes[self.next].time_ms <= song_ms {
            self.last_hits.push(self.notes[self.next]);
            self.next += 1;
            self.total_hits = self.total_hits.saturating_add(1);
        }
        self.last_hits.len()
    }

    /// Notes played by the last [`Self::update`].
    pub fn last_hits(&self) -> &[OpponentHit] {
        &self.last_hits
    }

    /// Notes played so far this playthrough.
    pub fn total_hits(&self) -> u32 {
        self.total_hits
    }

    /// Notes still waiting to be played.
    pub fn remaining(&self) -> usize {
        self.notes.len().saturating_sub(self.next)
    }

    /// Total number of opponent notes in the chart.
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{ChartNote, ChartStrumLine};

    fn chart() -> Chart {
        Chart::new("Versus", 120.0, 4_000, vec![ChartNote::tap(2_000.0, 0)]).with_strum_lines(vec![
            ChartStrumLine {
                slot: StrumSlot::Opponent,
                key_count: 4,
                notes: vec![ChartNote::tap(500.0, 1), ChartNote::held(900.0, 2, 400.0)],
            },
            ChartStrumLine {
                slot: StrumSlot::Girlfriend,
                key_count: 4,
                notes: vec![ChartNote::tap(700.0, 3)],
            },
        ])
    }

    #[test]
    fn opponent_notes_play_in_time_order() {
        let mut opponent = Opponent::new(&chart());
        assert_eq!(opponent.note_count(), 3);

        assert_eq!(opponent.update(400.0), 0);
        assert_eq!(opponent.update(600.0), 1);
        assert_eq!(opponent.last_hits()[0].lane, 1);
        assert_eq!(opponent.last_hits()[0].slot, StrumSlot::Opponent);

        assert_eq!(opponent.update(800.0), 1);
        assert_eq!(opponent.last_hits()[0].lane, 3);
        assert_eq!(opponent.last_hits()[0].slot, StrumSlot::Girlfriend);

        assert_eq!(opponent.update(950.0), 1);
        assert_eq!(opponent.last_hits()[0].lane, 2);
        assert_eq!(opponent.last_hits()[0].sustain_ms, 400.0);
        assert_eq!(opponent.remaining(), 0);
        assert_eq!(opponent.total_hits(), 3);
    }

    #[test]
    fn player_lines_are_never_simulated() {
        let mut chart = chart();
        chart.strum_lines.push(ChartStrumLine {
            slot: StrumSlot::Player,
            key_count: 4,
            notes: vec![ChartNote::tap(500.0, 0)],
        });
        let opponent = Opponent::new(&chart);
        assert_eq!(opponent.note_count(), 3);
    }
}
