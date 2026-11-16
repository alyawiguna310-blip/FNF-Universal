//! Judgement presets, ported from `funkin/game/scoring/HitWindowData.hx` and
//! `RatingManager.hx`.
//!
//! Values are exact copies of the Haxe tables; they are the numbers Codename
//! players see, so they must not be "rounded for convenience".

use serde::{Deserialize, Serialize};

/// `HitWindowData.JUDGE_SCALES`.
pub const JUDGE_SCALES: [f64; 9] = [1.5, 1.33, 1.16, 1.0, 0.84, 0.66, 0.5, 0.33, 0.2];

/// `WindowPreset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowPreset {
    /// `CNE_CLASSIC`: old Codename, very forgiving.
    CneClassic,
    /// `FNF_CLASSIC`: week 7 values.
    FnfClassic,
    /// `FNF_VSLICE`.
    FnFVSlice,
    /// The default preset (Etterna).
    Default,
}

impl WindowPreset {
    /// `HitWindowData.getWindows`.
    pub fn windows(self) -> HitWindows {
        match self {
            Self::CneClassic => HitWindows {
                sick: 50.0,
                good: 187.5,
                bad: 225.0,
                shit: 250.0,
            },
            Self::FnfClassic => HitWindows {
                sick: 33.334,
                good: 125.0025,
                bad: 150.003,
                shit: 166.67,
            },
            Self::FnFVSlice => HitWindows {
                sick: 45.0,
                good: 90.0,
                bad: 135.4,
                shit: 180.0,
            },
            Self::Default => HitWindows {
                sick: 37.8,
                good: 75.6,
                bad: 113.4,
                shit: 180.0,
            },
        }
    }

    /// The preset Codename would use for a user-facing name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name
            .to_ascii_lowercase()
            .replace(['_', '-', ' '], "")
            .as_str()
        {
            "cneclassic" | "codenameclassic" | "cn" => Some(Self::CneClassic),
            "fnfclassic" | "week7" | "classic" => Some(Self::FnfClassic),
            "fnfvslice" | "vslice" => Some(Self::FnFVSlice),
            "default" | "etterna" => Some(Self::Default),
            _ => None,
        }
    }
}

/// Half-widths of the four windows, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HitWindows {
    /// `sick`.
    pub sick: f64,
    /// `good`.
    pub good: f64,
    /// `bad`.
    pub bad: f64,
    /// `shit`.
    pub shit: f64,
}

/// The judgement a delta falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HitRating {
    /// Within the `sick` window.
    Sick,
    /// Within the `good` window.
    Good,
    /// Within the `bad` window.
    Bad,
    /// Within the `shit` window.
    Shit,
    /// Outside every window.
    Miss,
}

impl HitWindows {
    /// `HitWindowData.scaleWindows`.
    pub fn scaled(self, scale: f64) -> Self {
        Self {
            sick: self.sick * scale,
            good: self.good * scale,
            bad: self.bad * scale,
            shit: self.shit * scale,
        }
    }

    /// `HitWindowData.offsetWindows`.
    pub fn offset(self, offset: f64) -> Self {
        Self {
            sick: self.sick + offset,
            good: self.good + offset,
            bad: self.bad + offset,
            shit: self.shit + offset,
        }
    }

    /// `RatingManager.getRating`: the first rating whose window contains the
    /// absolute delta, otherwise [`HitRating::Miss`].
    pub fn rating_for(self, delta_ms: f64) -> HitRating {
        let delta = delta_ms.abs();
        if delta <= self.sick {
            HitRating::Sick
        } else if delta <= self.good {
            HitRating::Good
        } else if delta <= self.bad {
            HitRating::Bad
        } else if delta <= self.shit {
            HitRating::Shit
        } else {
            HitRating::Miss
        }
    }

    /// Widest window that still counts as a hit (`shit`).
    pub fn hit_window(self) -> f64 {
        self.shit
    }
}

impl Default for HitWindows {
    fn default() -> Self {
        WindowPreset::Default.windows()
    }
}
