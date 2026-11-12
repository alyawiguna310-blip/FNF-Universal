// Reference data model only. Not wired into the main engine or script VM.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteVisualState {
    pub offset: Vec2,
    pub rotation_degrees: f32,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteGameplayState {
    pub lane: u8,
    pub strum_time_ms: i64,
    pub sustain_duration_ms: u32,
    pub judged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteState {
    pub visual: NoteVisualState,
    pub gameplay: NoteGameplayState,
}

impl NoteState {
    /// Changes presentation only. Timing, lane and judgment remain untouched.
    pub fn set_visual_offset(&mut self, x: f32, y: f32) {
        self.visual.offset = Vec2 { x, y };
    }

    /// Gameplay timing is deliberately changed through a separate API.
    pub fn set_strum_time(&mut self, time_ms: i64) {
        self.gameplay.strum_time_ms = time_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_movement_does_not_change_gameplay_state() {
        let mut note = NoteState {
            visual: NoteVisualState {
                offset: Vec2 { x: 0.0, y: 0.0 },
                rotation_degrees: 0.0,
                scale: 1.0,
            },
            gameplay: NoteGameplayState {
                lane: 2,
                strum_time_ms: 1200,
                sustain_duration_ms: 500,
                judged: false,
            },
        };

        let gameplay_before = note.gameplay;
        note.set_visual_offset(40.0, -12.0);

        assert_eq!(note.gameplay, gameplay_before);
        assert_eq!(note.visual.offset, Vec2 { x: 40.0, y: -12.0 });
    }
}
