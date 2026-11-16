# Codename gameplay mechanics (P3)

The gameplay rules now come from the chart runtime rather than the prototype's
fixed four-lane, fixed-window assumptions. Sources in
`.reference/CodenameEngine` (`main`, `c2b73fa`):

* `source/funkin/game/scoring/RatingManager.hx` — rating table and windows
* `source/funkin/game/scoring/HitWindowData.hx` — presets
* `source/funkin/game/PlayState.hx` — `goodNoteHit`, miss handling, scoring
* `source/funkin/game/Note.hx`, `StrumLine.hx` — sustains and CPU lines

## Variable strum lines

* `RuntimeChart` already kept every line with its `keyCount` and `position`;
  `RuntimeChart::player_line` now selects the player line gameplay judges and
  `opponent_lines` exposes the rest.
* `fu_play::game::Chart` carries `lanes` (the player line's key count) and the
  non-player lines as `strum_lines`. Nothing in judging, scoring or input
  validation assumes four: `Gameplay::press` accepts any lane below
  `Chart::lanes`.
* Only the first player line is judged, matching solo play. Additional player
  lines (co-op) stay in the runtime chart and are neither judged nor simulated;
  this is a documented gap, not a silent remap.
* The playfield builds eight receptor and backdrop slots. Four-lane charts keep
  their original positions `[-3.6, -1.2, 1.2, 3.6]`; wider charts tighten the
  spacing to stay inside the camera. Lanes above eight are judged but not drawn.
* Input: `D F J K` and the arrow keys cover lanes 0-3, `A S L ;` cover lanes
  4-7. Release events are delivered too, which is what sustains need.

## Sustains

* `ChartNote` carries `sustain_ms`; `RuntimeChart` has preserved it since P2 and
  the gameplay projection now keeps it instead of dropping it.
* Hitting a head with `sustain_ms > 0` starts a `Hold { start_ms, end_ms,
  held_steps, state }`. `end_ms` is `head + sustain_ms` in chart milliseconds,
  so a hold that spans a BPM change ends at exactly the authored time.
* Health is awarded per crossed conductor step while the lane is held
  (`+0.023`, Codename's sustain-piece health). Because the steps come from the
  same `BpmMap` as judgement, the tick count changes with tempo exactly as the
  chart's grid does.
* `release(lane, ms)` finishes the hold: `Completed` at or after `end_ms`,
  `ReleasedEarly` before it. Releasing early costs nothing, matching Codename
  dropping the remaining sustain pieces.
* Sustains add no score, combo or accuracy sample — Codename's `NoteHitEvent`
  sets `countScore`/`countAsCombo` false for sustain notes.
* Sustain bodies are drawn as a quad trailing the head; they are a geometric
  approximation of Codename's tiled sustain sprites.

## Judgement system

`Chart::preset: WindowPreset` is always populated (Codename's `RatingManager`
defaults to `WindowPreset.DEFAULT`), and `Chart::windows` lets a chart override
it. `Gameplay::windows()` exposes what is in force; `main` prints it at load.

| Preset | sick | good | bad | shit |
| --- | --- | --- | --- | --- |
| `Default` (Codename default) | 37.8 | 75.6 | 113.4 | 180 |
| `CneClassic` | 50 | 187.5 | 225 | 250 |
| `FnfClassic` | 33.334 | 125.0025 | 150.003 | 166.67 |
| `FnFVSlice` | 45 | 90 | 135.4 | 180 |

Rating table (`RatingManager.initDefaultData`):

| Rating | Window | Score | Accuracy | Health |
| --- | --- | --- | --- | --- |
| sick | ≤ sick | 300 | 1.0 | +0.023 |
| good | ≤ good | 200 | 0.75 | +0.015 |
| bad | ≤ bad | 100 | 0.45 | 0 |
| shit | ≤ shit | 50 | 0.25 | −0.05 |

* Boundaries are inclusive, as `judgeNote`'s `time <= rating.window` is.
* A note is pressable only inside the `shit` window; beyond it the press is a
  ghost tap and the note later misses.
* `Flags.SHITS_BREAK_COMBO` is reproduced: a shit hit scores 50 but resets the
  combo and increments misses.
* A missed note costs `−0.0475` health and adds no accuracy sample; accuracy is
  `sum(weights) / presses`, exactly `totalAccuracyAmount / accuracyPressedNotes`.
* Scoring no longer uses the prototype's flat 150/100/50 with a combo
  multiplier.

## Beat, step and measure events

* `Gameplay::update` advances one `Conductor` and stores its `Tick`; `tick()`
  exposes `steps`, `beats`, `measures`, `bpm_change` and
  `time_signature_change` in Codename's dispatch order. Multiple indices appear
  when a frame crosses several boundaries.
* Chart events are projected with their step/beat/measure resolved once through
  the same `BpmMap` (`ChartEvent { time_ms, name, step, beat, measure }`).
  `Gameplay::fired_events()` lists the events whose step the last update
  crossed; each fires once per playthrough.
* This is the feed receptor and character animations need: `tick().beats` for a
  bop, `tick().steps` for per-step callbacks, `fired_events()` for chart-scripted
  moments. The playfield does not render a beat pulse yet; the character still
  reacts to player hit/miss feedback only.

## Opponent gameplay

* `fu-play::opponent::Opponent` collects every non-player note (opponent and
  girlfriend lines) in time order and plays each at its chart time.
  `Gameplay::update` advances it and `opponent_hits()` reports the frame's
  hits with their line, slot and lane.
* Opponent hits never touch player score, combo, accuracy or health, and player
  notes never enter the opponent queue — separation is structural.
* Opponent lines are not rendered and their characters do not animate; the
  simulation is the data layer those animations will read.

## Real Codename visual assets

`--codename <assets dir>` reads the installed game's art instead of the
generated default pack. `codename_content::CodenameContent` resolves it and
`apply_skin` installs it under the playfield's existing logical keys:

| Playfield element | Source |
| --- | --- |
| Notes | `images/game/notes/default.xml` frames `purple0000`, `blue0000`, `green0000`, `red0000` |
| Receptors | `arrowLEFT/DOWN/UP/RIGHT0000`, pressed states from `left press0002` and siblings |
| Sustain body | `purple/blue/green hold piece0000` |
| Sustain cap | `pruple end hold0000` (sic), `blue/green/red hold end0000` |
| Judgement | `images/game/score/{sick,good,bad,shit}.png` |
| Countdown | `images/game/{ready,set,go}.png` |
| Health icons | `images/icons/<icon>/icon.png` |
| Characters | `data/characters/<id>.xml` + `images/characters/<sprite>.png/.xml` |
| Stage | `data/stages/<id>.xml` + `images/<folder>/<sprite>.png` |

Notes, receptors and sustains are drawn at the skin's own aspect ratio from the
cropped frame, not stretched over placeholder geometry. Characters are fitted
to the playfield's fixed camera and mirrored through flipped UVs (a negative
scale would be back-face culled); their left/right placement still comes from
the stage anchor and character offsets.

Anything not found is printed with its exact path ("codename: missing ...") and
the generated pack remains only as an explicit fallback.

## Verification

* `crates/fu-play/src/game.rs`: 15 unit tests (windows, ratings, scoring,
  combo/shit, accuracy, health, lanes, holds, release, sustain across a BPM
  change, step events, opponent simulation, completion).
* `crates/fu-play/src/opponent.rs`: 2 tests.
* `crates/fu-play/src/visuals.rs`: 11 tests, two of them new (lane layout at
  six and four lanes, sustain body follows the head).
* `crates/fu-play/tests/mechanics.rs`: 6 Codename-fixture tests — six-lane chart,
  sustain spanning a BPM change, step-resolved events, opponent/girlfriend
  simulation, explicit presets, and a real `bopeebo` run.
* `cargo test --workspace` is green; `--simulate` on the six-lane fixture and on
  `dadbattle` reports the expected lanes/preset/windows and completes.

## Remaining differences from Codename

* Co-op player lines past the first are parsed, not judged or simulated.
* Note types (alt notes, mines, lifts, custom note types) are not modelled;
  every note is judged as a normal note.
* Vocals, stage/camera events, note splashes, combo animations and rating
  sprites are not implemented; chart events are dispatched but have no built-in
  effects.
* The generated pack is still used when `--codename` is not passed, and for
  assets Codename lacks a Sparrow version of (for example the girlfriend's
  Adobe Animate atlas in this installation), the HUD bitmap font and the
  health-bar fill. Those are reported, not silently replaced.
* Sustain release has no penalty; Codename's `SUSTAINS_AS_ONE_NOTE` path can
  count an early release differently depending on mod API version.
* Opponent hit health (`addHealth` with `opponentSide`) is not applied because
  Codename's CPU hits carry zero health; mods that override that are not
  supported.
* The windowed runner was not played interactively for this milestone. Input
  mapping, release handling and rendering are covered by unit and headless
  renderer tests only, so no visual or input parity is claimed.
