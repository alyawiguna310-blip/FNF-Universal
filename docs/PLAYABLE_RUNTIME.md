# Playable runtime (vertical slice)

The `fu-play` crate is now an actual playable runtime, not only a library of
parts. It loads one song out of an imported V-Slice mod, plays its instrumental,
scrolls the chart, judges keyboard input and finishes the song.

```bash
cargo run -p fu-play -- --mod "mods/My V-Slice Mod" --song dadbattle --difficulty hard
cargo run -p fu-play -- --mod "D:/Codename/assets" --song bopeebo --codename "D:/Codename/assets"
cargo run -p fu-play -- --mod "mods/My V-Slice Mod" --song dadbattle --simulate
cargo run -p fu-play -- --mod "mods/My V-Slice Mod" --song dadbattle --audio-check
```

Controls: `D F J K` or the arrow keys hit the four lanes, `P` pauses, `R`
restarts, `Esc` quits. `--codename <assets>` replaces the generated gameplay
art with the installed Codename content: the real note skin (notes, receptors,
press frames, sustain bodies and end caps), judgement graphics, countdown,
health icons, characters and stage layers. Without it the generated default
pack is used, and every missing Codename asset is printed by path. `--simulate` runs the same loading and judgement path
headlessly at perfect timing and prints the result, which is what CI can check.
`--audio-check` plays the instrumental for about three seconds and prints the
drift between the audio clock and the wall clock, so a real Ogg file can be
told apart from one that merely decoded. `FU_PLAY_TRACE=1` additionally prints
every judgement and key press with its song position.

## Pipeline

```text
mod directory
   |  fu_import::vslice::import_vslice_mod   (read-only, no execution)
   v
VsliceMod  ->  fu_play::song::load_song       (song + difficulty selection)
   |              |-- player notes only (d 0..=3), lane = d % 4
   |              |-- tempo from the first BPM change
   |              +-- instrumental read through fu-vfs + fu-assets
   v
fu_play::game::Chart  ->  fu_play::session::Session  (position, judgement, screens)
   |                          |-- fu_play::game::Gameplay (timing, score, combo, misses)
   |                          +-- fu_play::audio::AudioEngine (audio clock)
   v
fu_play::visuals::Visuals  ->  fu_render3d::Renderer3D  ->  fu_render3d_wgpu::WindowedRenderer
```

Each layer keeps its existing contract: the importer stays read-only and never
executes mod content, `game` stays free of windowing and audio, and the renderer
receives only scene data.

## What is real

* Mod discovery, metadata and chart import through `fu-import` (unchanged).
* Difficulty selection, case-insensitive, defaulting to the first declared and
  charted difficulty.
* Player-only note extraction. V-Slice stores opponent notes in the same array
  with directions `4..=7`; they are neither judged nor rendered as player notes.
* Note length from the decoded instrumental, falling back to `last note + 2000 ms`
  when no audio is available or decodable.
* Timing, grading, combo, score and miss rules from `fu_play::game`, driven by
  the audio position.
* A wall-clock guard: the song position is the later of the audio clock and a
  monotonic clock, so a stream shorter than the chart cannot freeze the song at
  its last sample. Real audio still wins while it is playing.
* Window, GPU surface, resize handling, pause/restart. The title bar is branded
  `FNF Universal | <Song Name>` and carries no gameplay statistics.
* An in-scene HUD drawn with a built-in 3x5 bitmap font: score, combo, misses,
  accuracy percentage, `FNF UNIVERSAL` branding and a live song progress bar.
  The HUD is part of the rendered scene, not the title bar, and it updates every
  frame from the judged state.
* Correct playfield depth order: the opaque lane backdrops sit behind the
  receptors, notes and HUD, so nothing in the playfield is occluded.
* Headless rendering of the chart through `WgpuBackend::new_headless` is covered
  by a test, so the render path is exercised without a window.

## Current limitations

* Notes, receptors, stage, health bar, icons, countdown, judgements, the
  menu/pause/results panels and the default character sheet come from the
  generated default resource pack, or from a pack/mod override (see
  `docs/RESOURCES.md`).
* Only the default character is drawn: there is one on-screen character built
  from the generated idle/sing spritesheet. A mod's `characters/*.json` rig,
  opponent parts, and Adobe Animate atlas bundles are inventoried by the importer
  but not rendered yet.
* Sound effects (`sfx.countdown`, `sfx.hit`, `sfx.miss`) are loaded and validated
  through the resource pipeline but are not played back; only the instrumental
  goes through the audio device.
* Text rendering is a built-in 3x5 bitmap font for the HUD only. Mod fonts,
  FreeType shaping, and text from mod data are **not** loaded or drawn.
* Opponent notes are not played back visually; only the player strumline exists.
* Only the default chart variation is used. `-erect` and other variations are
  imported but not selectable from the runner yet.
* Judgement windows are the engine's own (Perfect < 50 ms, Good < 100 ms, Bad
  <= 150 ms early, miss at 100 ms late); they are not V-Slice's windows.
* Ogg Vorbis decoding is enabled through rodio's `vorbis` feature and the
  symphonia Ogg/Vorbis codecs compile and link, but no real `.ogg` sample is
  committed, so decoding was verified with a generated WAV instead. A real
  V-Slice song's audio has not been played in this environment yet.
* Health, sustains, hold notes, events (`Zoom Camera` and friends), characters
  and stages are imported but not simulated.

## Default resources

`fu_play::resources::GameResources` loads a manifest-driven default pack, layered
under an optional `--assets <dir>` pack and the selected mod, with caching and a
visible fallback for anything missing. `docs/RESOURCES.md` documents the layers,
keys, formats and how to supply additional packs.

## Verification

`cargo test -p fu-play` covers the slice end to end:

* `tests/playable.rs` — loading a fixture mod into the engine, opponent-note
  exclusion, difficulty selection, error reporting for unknown songs and
  difficulties, length fallback, autoplay completion, a silent run that misses
  every note, session state after a full chart, WAV duration decoding, headless
  rendering of chart frames, and rejection of charts with no player notes.
* `tests/playable.rs::a_gameplay_frame_draws_receptors_notes_and_hud` renders a
  real frame on a headless GPU, reads the framebuffer back
  (`WgpuBackend::read_target_pixels`) and checks the pixels: the receptor and an
  approaching note are drawn in their own colours over the lane backdrop, and
  HUD glyphs appear in the top-left corner. This is what catches a blank or
  occluded playfield.
* `src/visuals.rs` — glyph coverage for the font, lane/receptor/note depth order,
  live HUD strings for score, combo, misses and accuracy, HUD text nodes being
  visible and anchored inside the viewport, and notes reaching the receptor line
  as the song advances.
* `src/session.rs` — session screens, non-rewinding position, pause gating.

The windowed runner was smoke-tested against a temporary fixture mod: it loaded
the song, opened a GPU surface, decoded the WAV instrumental and stayed in its
render loop without errors. It was also run against a real V-Slice mod: the Ogg
instrumental decoded, the audio clock tracked the wall clock, and the run
reached `COMPLETE`. It was not driven by a human pressing keys, and no native
window screenshot was captured, so the on-screen result is verified through the
headless framebuffer test rather than by eye.
