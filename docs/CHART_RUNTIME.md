# Chart runtime integration (P2)

This milestone connects the `fu-timing` Conductor port to the chart and
gameplay path, so a song's tempo map — not a single metadata BPM — drives
playback. It is additive: the existing V-Slice loader, the Codename loader, the
windowed runner and its UI are the ones that already shipped, now fed by one
timing source.

## Architecture

```
fu_import::codename ─┐                         ┌─ fu_timing::BpmMap ── Conductor
                     ├─► fu_chart::RuntimeChart┤        (BPM / time-signature /
fu_import::vslice  ──┘   (notes f64, strum      │         continuous changes)
                          lines, events,         └─ HitWindows / WindowPreset
                          steps resolved)
                                   │
                       Chart::from_runtime (fu-play::game)
                                   │
                    ┌──────────────┴──────────────┐
              Gameplay (judging,           visuals (unchanged
              Conductor + Tick)             drawing path)
                    │
              Session (screen state, clock supplied by the runner)
```

* **`fu_chart::runtime`** is the model: [`RuntimeChart`] holds the
  `BpmMap`, every strum line with its notes, the events, the scroll speed and
  the derived player-note list. `RuntimeChart::new` resolves each note's
  fractional `step` through the map, so a note cannot carry a step that
  disagrees with the timing it is played against.
* **`fu-play::game::Chart`** is the gameplay projection. It keeps `song`,
  `bpm`, `length_ms`, `notes` and now `timing: BpmMap`. `Chart::from_runtime`
  copies the runtime notes (player side, lanes `< LANES`) and the same map; no
  timing is recomputed.
* **`Gameplay`** owns a `Conductor`. `Gameplay::update(song_ms)` advances it and
  stores the returned `Tick`, so hit judging, tempo reporting and step/beat/
  measure signals all come from one clock. `tick()`, `bpm()`, `step()`,
  `beat()`, `measure()`, `timing()` and `conductor()` expose that state;
  `Session` forwards the same values.
* **Loaders** (`song.rs`, `codename_song.rs`) build the `RuntimeChart`, apply
  the audio-derived length, then project with `Chart::from_runtime`. Difficulty
  resolution, audio lookup, error reporting and the `PlayableSong` shape are
  unchanged.

## What changed in numbers

Note times are no longer rounded. `ChartNote::time_ms` is `f64` end to end, so
a V-Slice note authored at `1000.5 ms` stays there instead of becoming `1001`.
That is the only behaviour change on the V-Slice path; every other V-Slice
assertion in `tests/playable.rs` still holds unchanged.

## Timing source per feature

| Feature | Source |
| --- | --- |
| Note positions | `RuntimeChart.notes` → `Chart.notes` (`f64` ms, sorted by `(time, lane)`) |
| Tempo, step/beat/measure | `Chart.timing` → one `Conductor`, advanced once per `update` |
| Step / beat / measure events | `Gameplay::tick()` (`Tick`, Codename's dispatch order) |
| BPM / time-signature signals | `Tick::bpm_change`, `Tick::time_signature_change` |
| Judgement windows | unchanged gameplay constants (`50/100/150 ms`); `fu_timing::WindowPreset` is available through `RuntimeChart::windows` but gameplay does not switch presets yet |

## Codename chart loading

* `chart.events` **and** the song-level `events.json` are interpreted by
  `BpmMap::from_codename_song`, so `BPM Change`, `Time Signature Change` and
  `Continuous BPM Change` reach gameplay.
* Only notes on `boyfriend`/`player` lines are judged, matching the V-Slice
  loader's player-only rule; opponent lines remain in `RuntimeChart.strum_lines`.
* Notes whose lane is outside their line's `keyCount` are dropped, as before.
* `--simulate` now prints the tempo map before running:

```
loaded 'Timing Song' (hard) - 3 notes, 9000 ms, tempo 100 bpm, no instrumental, using clock timing
timing: 3 section(s), 0 rejected event(s)
  @      0.00 ms   100.00 bpm    4 beat(s)/measure  4 steps/beat
  @   3000.00 ms   200.00 bpm    4 beat(s)/measure  4 steps/beat
  @   6000.00 ms   200.00 bpm    3 beat(s)/measure  4 steps/beat
simulation: 3 hits, 0 misses, score 456, max combo 3, COMPLETE at 9000 ms
```

A shipped song (`dadbattle`, hard) reports one section and 162 notes and
completes headlessly; `bopeebo` reports 100 bpm and 13 steps at 2000 ms.

## Verification

* `crates/fu-chart/tests/runtime.rs` (5 tests): Codename fixture with a BPM
  change and a 3/4 change; conductor/`RuntimeChart` agreement on step, beat and
  measure floats; V-Slice `timeChanges` → map; sub-millisecond note precision
  and event projection; real `bopeebo`/`dadbattle`/`cocoa` charts whose note
  times match the authored values exactly.
* `crates/fu-play/tests/timing_integration.rs` (6 tests): timing events reach
  `Chart`; gameplay tempo/step/beat/measure across 100 → 200 bpm and 4/4 → 3/4;
  `Session` reports the same timing; a multi-tempo chart autoplays to
  completion; V-Slice `timeChanges` reach gameplay; a real `bopeebo` chart runs
  headlessly.
* Existing suites are unchanged and green: `fu-play` 30 unit tests, 13 V-Slice
  end-to-end tests, 8 Codename loader tests; `fu-timing` 14 golden + 1 parity
  test; whole workspace passes.

## Remaining gaps

* Judgement windows are now Codename presets (see
  `docs/CODENAME_GAMEPLAY.md`), selectable per chart and overridable.
* `Tick` is exposed and chart events are dispatched by step, but the playfield
  still has no beat pulse, step-driven receptor animation or measure bar.
* Sustains are judged (hold, release, per-step health); see the gameplay doc for
  the remaining differences.
* Variable lane counts reach gameplay and the playfield up to eight lanes;
  opponent/girlfriend lines are simulated but not rendered.
* No vocals, stage effects, camera effects or scripting hooks consume
  `RuntimeChart.events` yet; they are dispatched to gameplay only.
* The windowed runner was not played interactively for this milestone; headless
  `--simulate` and the headless renderer tests are the evidence. Gameplay parity
  claims therefore stop at "headless run with chart timing", not "verified in
  the running game".
