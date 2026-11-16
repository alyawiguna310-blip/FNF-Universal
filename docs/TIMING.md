# `fu-timing`: Codename Conductor in Rust

First migrated subsystem (roadmap phase P1). Reference: `Conductor.hx` and
`HitWindowData.hx` / `RatingManager.hx` at Codename commit
`c2b73fa11ee2daecb639909d782e652f8a0b925f`.

`fu-timing` is a behavioural port, not a syntax translation. It owns no audio
device: the caller feeds the song position in milliseconds (Codename reads it
from `FlxG.sound.music.time`) and receives the step/beat/measure counters plus
the signals `Conductor.update()` would dispatch.

## API

```rust
use fu_timing::{BpmMap, Conductor, WindowPreset};

// From the existing importer model: no second chart parser.
let map = BpmMap::from_codename_song(&chart, song.events.as_ref(), song.metadata.as_ref());
let mut conductor = Conductor::new(map);
let tick = conductor.update(song_position_ms);
// tick.steps / tick.beats / tick.measures : dispatch order, incl. multi-step frames
// tick.bpm_change / tick.time_signature_change
```

| Item | Purpose |
| --- | --- |
| `BpmMap::new` / `default_map` | single-change map (`Flags.DEFAULT_BPM` = 100) |
| `BpmMap::from_timing_events` | build from interpreted events |
| `BpmMap::from_codename_chart` / `from_codename_song` | build from `fu_import::codename` |
| `BpmMap::steps_at_time` / `time_at_steps` | `getTimeInSteps` / `getStepsInTime` |
| `BpmMap::beats_at_time` / `time_at_beats` | `getTimeInBeats` / `getBeatsInTime` |
| `BpmMap::measures_at_time` / `time_at_measures` | `Conductor.update` derivation / `getMeasuresInTime` |
| `BpmMap::bpm_at_time` | `getTimeInBPM`, interpolates continuous ramps |
| `BpmMap::changes` / `rejected` | `bpmChangeMap` / `invalidEvents` |
| `Conductor::update` | `Conductor.update`, returns a `Tick` |
| `Conductor::{step_crochet, crochet, current_bpm, steps_per_beat, beats_per_measure}` | the same getters |
| `WindowPreset::{CneClassic, FnfClassic, FnFVSlice, Default}.windows()` | `HitWindowData.getWindows` |
| `HitWindows::{scaled, offset, rating_for}` + `JUDGE_SCALES` | `scaleWindows`, `offsetWindows`, `RatingManager.getRating` |

## Haxe → Rust mapping

| Haxe | Rust |
| --- | --- |
| `BPMChangeEvent` | `bpm::BpmChange` (same fields, camelCase serde) |
| `Conductor.mapBPMChanges(song)` | `BpmMap::from_timing_events` / `from_codename_song` |
| `Conductor.mapEvent` | `BpmMap::apply` |
| `Conductor.mapBPMChange` | `BpmMap::push_change` |
| `Conductor.getTimeInChangeIndex` and the step/beat/measure variants | `BpmMap::change_index_at_time/_at_step/_at_beat/_at_measure` |
| `Conductor.getTimeWithIndexInBPM` | `BpmMap::time_with_index_in_bpm` |
| `Conductor.getTimeWithBPMInSteps` | `BpmMap::time_with_bpm_in_steps` |
| `Conductor.getStepsWithIndexInBPM` | `BpmMap::steps_with_index_in_bpm` |
| `Conductor.getStepsWithBPMInTime` | `BpmMap::steps_with_bpm_in_time` |
| `Conductor.getTimeInBPM` / `getTimeInSteps` / `getStepsInTime` / `getTimeInBeats` / `getBeatsInTime` / `getMeasuresInTime` | `bpm_at_time` / `steps_at_time` / `time_at_steps` / `beats_at_time` / `time_at_beats` / `time_at_measures` |
| `Conductor.stepCrochet` / `crochet` | `Conductor::step_crochet` / `crochet` |
| `Conductor.update` signals | `Conductor::update` → `Tick` |
| `CoolUtil.floorInt` / `boundInt` | `bpm::floor_int` / `bound_int` |
| `MathUtil.equal` / `lessThan` | `bpm::approx_equal` / `approx_less_than` (margin `1e-7`) |
| `FlxMath.remapToRange` | `bpm::remap_to_range` |
| `HitWindowData.getWindows` / `JUDGE_SCALES` | `WindowPreset::windows` / `windows::JUDGE_SCALES` |

Reproduced details that are easy to lose:

- `stepCrochet = 15000 / bpm`, `crochet = 15000 * stepsPerBeat / bpm`.
- Change entries accumulate `stepTime/beatTime/measureTime`; a continuous
  change starts its next segment at `endStepTime/endSongTime`.
- `Continuous BPM Change`: `endSongTime = t + steps/(bpm1-bpm0) * ln(bpm1/bpm0) * 15000`,
  `endStepTime = stepTime + steps`; tempo interpolates geometrically.
- Events inside a running continuous change are rejected into
  `BpmMap::rejected()` (`invalidEvents`), as is a continuous change whose
  target equals the current tempo.
- `Time Signature Change` maps a same-tempo change first, sets
  `beatsPerMeasure`, derives `stepsPerBeat` as `floor(16/denominator)` unless
  `params[2]` says the denominator is already steps, then floors
  `stepTime/beatTime/measureTime` after adding `0.99998`.
- Event ordering uses `Std.int(a.time - b.time)`: events less than 1 ms apart
  compare equal, and at equal times the time-signature event sorts first.
- `Conductor.update` dispatches every crossed step (`for i in oldStep...curStep
  emit i+1`) and, on a backwards seek, emits the single index it landed on.
- Boundaries use strict `>`: a change at exactly `t` is already active at `t`.

## Numerical verification

- `crates/fu-timing/tests/golden.rs`: 14 tests, hand-derived closed forms
  (constant tempo, single and multiple BPM changes, 3/4 and 7/8 signatures,
  continuous ramp midpoint/end, ramp-then-change accumulation), rejection
  rules, conductor dispatch order and backwards seek, all four judgement
  presets and boundary ratings, plus a real-asset test.
- `crates/fu-timing/tests/parity.rs`: 47 corpus cases / 599 samples compared
  against `tests/reference/conductor_ref.js`, a direct JavaScript transcription
  of the Haxe functions, generated by `tests/reference/generate_parity.js`.
  Compares every change field, rejected events, `bpm_at_time`,
  `step/beat/measure` floats and integers per sample, and the four
  time↔index conversions. Tolerance `1e-9` relative.
- `real_codename_charts_keep_constant_tempo_timing` parses
  `D:\idk\Codename\assets\songs\{bopeebo,dadbattle,cocoa}` through
  `fu-import`, builds the map with `from_codename_song`, and checks the
  constant-tempo closed form `steps = t * bpm / 15000` plus round trips. It
  skips (with a printed reason) when the installation or a timing event is
  absent.

Haxe itself cannot be executed here (no `haxe`/hxcpp toolchain, and the shipped
`CodenameEngine.exe` exposes no timing dump), so "numerical comparison against
the Haxe implementation" means comparison against a second transcription of the
same Haxe source, anchored by independently hand-derived closed forms. This is
stated rather than implied.

## Deliberate deviations

1. **Integer BPM parameters are accepted.** Haxe's `params[0] is Float` check
   silently ignores a `BPM Change` written with an integer literal (`180`) while
   accepting `180.0`; Codename's own event editor stores a `TFloat`, so the
   port accepts any JSON number. Everything else about the event is identical.
2. **Stable ordering for true ties.** Haxe's sort is unstable; this port keeps
   source order for two events that compare equal, which is deterministic.
3. **`BpmMap::measures_at_time` is new.** Haxe only exposes the reverse
   conversion; this helper mirrors the `Conductor.update` derivation so callers
   do not have to duplicate it.
4. **No audio clock inside the crate.** `songOffset` is modelled
   (`set_song_offset`, `song_position`) but the position is supplied, which
   keeps tests deterministic.

## Integration status with `fu-play`

Wired (P2, see `docs/CHART_RUNTIME.md`). `fu_chart::RuntimeChart` interprets a
Codename chart's events (and the song-level `events.json`) or a V-Slice
`timeChanges` list into one `BpmMap`; `fu_play::game::Chart` carries that map
and `Gameplay` owns a `Conductor`, so tempo, step/beat/measure signals and note
times all come from the same source. Note times are now `f64` end to end
instead of rounded to whole milliseconds.

Still open: judgement windows remain the prototype's constants rather than a
Codename preset, `Tick` is not rendered, sustains are not judged, and only the
first four lanes are playable.

```bash
cargo test -p fu-timing
cargo test --workspace
node crates/fu-timing/tests/reference/generate_parity.js   # regenerate corpus
```
