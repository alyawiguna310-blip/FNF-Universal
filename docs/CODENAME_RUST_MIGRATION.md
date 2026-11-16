# Codename Engine → Rust: architecture inventory & migration roadmap

Status: **milestone 1 (inventory + mapping + roadmap)**. No engine code was ported yet.
Date: 2026-11-16.

## 0. What was actually inspected (and where the source came from)

| Claim | Verified result |
| --- | --- |
| `D:\idk\Codename` is the engine source | **No.** It is a compiled distribution: `CodenameEngine.exe` (67 MB, hxcpp), `lime.ndll`, `libvlc*.dll`, `assets/`, `mods/`, `plugins/`, `lua/`, `manifest/`. |
| Haxe engine source exists locally | **No.** Only 35 `.hx` files, all *content* scripts (`assets/data/stages/*.hx`, `assets/songs/*/cutscene.hx`, note types, dialogue). Those are mod-facing script samples, not engine modules. |
| Source obtained for inspection | **Yes**, read-only, from upstream `github.com/CodenameCrew/CodenameEngine`, branch `main`, commit `c2b73fa11ee2daecb639909d782e652f8a0b925f` (2026-11-15), sparse checkout of `source/` only, cloned to `.reference/CodenameEngine` (git-ignored). `D:\idk\Codename` was not modified. |
| Inventory is reproducible | `docs/codename_module_inventory.csv` — all 369 files with per-file LOC and subsystem tag. |

## 1. Scale

- **369** Haxe files, **64,655** lines in `source/`.
- Largest modules: `editors/charter/Charter.hx` 2,812; `game/PlayState.hx` 2,571; `backend/utils/CoolUtil.hx` 1,561; `editors/stage/StageEditor.hx` 1,373; `editors/character/CharacterEditor.hx` 1,015; `menus/ui/Alphabet.hx` 965; `backend/system/console/ConsoleUI.hx` 820; `backend/shaders/BloomEffect.hx` 765.
- Subsystem distribution (files): Editors 98, Scripting events 53, Utilities 28, Options 27, Menus/UI 20, Gameplay 13, Core runtime/timing/input 13, Build macros 13, Updater 12, Framerate/capture 10, Assets/VFS/mods 10, Scripting core 9, Dev console 9, MusicBeat framework 7, Shaders 6, Cutscenes 6, Chart 6.

## 2. Architecture inventory

### 2.1 Boot & platform
| Module | Role |
| --- | --- |
| `backend/system/Main.hx` (entry, `project.xml` `app main`) | Window creation, Lime/OpenFL init, `Paths.init()`, `ModsFolder.init()`, `Conductor.init()`, options/save load, first state. |
| `backend/system/FunkinGame.hx`, `MainState.hx`, `FunkinRatioScaleMode.hx` | Game subclass, scale mode (1280×720 letterbox), frame hooks. |
| `backend/system/Flags.hx` | Compiled defaults + runtime flags. Verified: `DEFAULT_BPM 100.0`, `DEFAULT_BEATS_PER_MEASURE 4`, `DEFAULT_STEPS_PER_BEAT 4`, `DEFAULT_DIFFICULTY "normal"`, `DEFAULT_HEALTH_ICON "face"`, `DEFAULT_COLOR 0xFF9271FD`, `DEFAULT_COOP_MODE false`, `DEFAULT_OPPONENT_MODE false`. |
| `backend/system/modules/CrashHandler.hx`, `FunkinCache.hx` | Crash reporting, graphics cache. |
| `backend/utils/{WindowUtils,RegistryUtil,NativeAPI,NdllUtil,SysZip,HttpUtil}` | Platform glue, Windows registry, native libs, zips, HTTP. |
| `backend/system/{updating,github}` | GitHub-release auto-updater. |

### 2.2 State / frame model
- `backend/MusicBeatState.hx` / `MusicBeatSubstate.hx` / `MusicBeatGroup.hx` / `MusicBeatTransition.hx`: state stack built on Flixel's `FlxState`.
- Exposes `curStep/curBeat/curMeasure` (+ float variants), `songPos`, `controls`, `controlsP1/P2`, `stateScripts:ScriptPack`, `call()/callOne()/event()` hooks into HScript, `stepHit/beatHit/measureHit` overrides, transitions.
- **`backend/system/Conductor.hx` is the timing authority** (623 lines, verified semantics):
  - Static global; `songPosition` mirrors `FlxG.sound.music.time` (audio is the clock), minus `songOffset`.
  - `bpmChangeMap: Array<BPMChangeEvent>` derived from *chart events*, not the chart header: `BPM Change`, `Time Signature Change`, `Continuous BPM Change`.
  - `crochet = 15000 * stepsPerBeat / bpm`, `stepCrochet = 15000 / bpm` (ms per step at 4 steps/beat).
  - Per-change accumulators `stepTime/beatTime/measureTime` + `endSongTime/endStepTime`; continuous BPM changes use an integral formula: `endSongTime = time + (deltaSteps)/(bpm1-bpm0) * ln(bpm1/bpm0) * 15000`.
  - Signals `onStepHit/onBeatHit/onMeasureHit/onBPMChange/onTimeSignatureChange`; dispatches to every state/substate implementing `IBeatReceiver` unless `cancelConductorUpdate`.
- Input: `backend/system/Controls.hx` extends Flixel `FlxActionSet`; per-action `pressed/justPressed/justReleased` for `up/down/left/right` and `note-up/down/left/right` (P1/P2), rebound via `options/keybinds`.

### 2.3 Assets & mods
| Module | Role |
| --- | --- |
| `backend/assets/Paths.hx` | Single asset resolution entry: `getPath`, `character`, `getFrames`, `getMultiFrames`, `framesExists`, folder listing; `AssetSource` (SOURCE / MODS / BOTH). |
| `backend/assets/AssetSource.hx`, `AssetsLibraryList.hx`, `IModsAssetLibrary.hx`, `ScriptedAssetLibrary.hx`, `TranslatedAssetLibrary.hx` | Layered asset libraries with priority and translation overlay. |
| `backend/assets/ModsFolder.hx`, `ModsFolderLibrary.hx`, `ZipFolderLibrary.hx` | `mods/autoload.txt` list, folder and **zip** mods, per-mod INI config, library prepare/switch. |
| `backend/utils/XMLUtil.hx` | Character/stage/week XML parsing beyond Flixel's atlas XML. |

Resolution order is layered: engine `assets/` → each loaded mod (autoload order) → translations; last wins per file. `USE_ADAPTED_ASSETS` replaces OpenFL's asset system with this tree.

### 2.4 Chart formats & song data (`backend/chart/`)
- `Chart.hx` — `ChartFormat` abstract: `CODENAME(0)`, `LEGACY(1)`, `VSLICE(2)`, `PSYCH_NEW(3)`.
  Detection (verified): `codenameChart == true | "true"` → CODENAME; has `version` **and** `scrollSpeed` → VSLICE; `song.format` starts with `psych_v1` → PSYCH_NEW; else LEGACY.
- `ChartData.hx` — runtime song: `meta`, `events`, `strumLines`, `notes`; `ChartMetaData` with `displayName, bpm, beatsPerMeasure, stepsPerBeat, icon, color, coopAllowed, opponentModeAllowed, instSuffix, vocalsSuffix, needsVoices, difficulties, variants, metas`.
- `loadChartMeta()` — per difficulty/variant meta resolution: `meta-<variant>-<difficulty>.json`, `meta-<variant>.json`, `meta-<difficulty>.json`, `meta.json`; auto-detects difficulties from `charts/` when `difficulties` is empty, and reorders the classic 3 as easy/normal/hard.
- Parsers: `FNFLegacyParser.hx`, `PsychParser.hx`, `VSliceParser.hx` (+ Codename `strumLines`), `EventsData.hx`.

### 2.5 Gameplay (`funjin/game/`)
- `PlayState.hx` (2,571) — the whole song loop: `SONG:ChartData`, `strumLines`, `stage`, `boyfriend/dad/gf`, `inst`/`vocals`, `updateRating`, `generateSong`, `startSong`, `endSong`, camera zoom, death/retry, campaign accumulators, `loadSong(name, difficulty, variation, opponentMode, coopMode)`, charting mode.
- `Note.hx` — `strumTime`, `noteData`, `sustainLength`, `isSustainNote`, `sustainParent`, `nextSustain`, `noteTypeID`, `extra:Map<String,Dynamic>`, `scrollSpeed`, `noteAngle`, early/late press windows, sustain clipping.
- `StrumLine.hx` — `characters[]`, `cpu`, `opponentSide`, `controls`, `hitWindow`, `ghostTapping`, `animSuffix`, `strumScale`, `vocals`, signals `onHit/onMiss/onNoteUpdate/onNoteDelete`.
- `Strum.hx`, `NoteGroup.hx`, `Character.hx`, `Stage.hx`, `HealthIcon.hx`, `HudCamera.hx`, `Splash*`.
- `scoring/RatingManager.hx` + `HitWindowData.hx` — presets (verified):
  - `CNE_CLASSIC` sick 50 / good 187.5 / bad 225 / shit 250 ms
  - `FNF_CLASSIC` (week 7) 33.334 / 125.0025 / 150.003 / 166.67
  - `FNF_VSLICE` 45 / 90 / 135.4 / 180
  - default (Etterna) 37.8 / 75.6 / 113.4 / 180
  - plus `JUDGE_SCALES` and window scaling/offset helpers.
- Cutscenes: `game/cutscenes/*` + dialogue (6 files, driven by HScript/JSON).

### 2.6 Scripting (`backend/scripting/`, 62 files)
- `HScript.hx`, `Script.hx`, `ScriptPack.hx`, `GlobalScript.hx`, `ModState.hx`, `ModSubState.hx`, `MultiThreadedScript.hx`, `DummyScript.hx`, `EventManager.hx`.
- Engine is built on **hscript-improved** (external haxelib): imports, public/static vars, erased accessor bypass, maps, abstract support; `hscriptPos` for error positions.
- **53 typed event classes** under `scripting/events/`: `gameplay`, `note`, `character`, `stage`, `sprite`, `soundtray`, `shader`, `dialogue`, `menu/*`, `gameover`, `discord`, `healthicon`, `splash`.
- `SOFTCODED_STATES`: every state can be replaced/patched by `data/states/<StateName>.hx`; `GLOBAL_SCRIPT` runs scripts for the whole mod; `ScriptPack` scopes song scripts.
- Content scripts present in the shipped install confirm the API surface: stages (`limo.hx`, `philly.hx`, `tank.hx`), note types (`Alt Anim Note.hx`), songs (`stress/scripts/tankmenRun.hx`), states (`StageEditor.hx`), dialogue boxes.

### 2.7 Rendering
- HaxeFlixel (`FlxSprite`, `FlxAtlasFrames`, `FlxCamera`, `FlxTypedGroup`) on OpenFL/Lime → OpenGL.
- **Sparrow** (`png` + `xml`) and **Adobe Animate** atlases via `flxanimate`; `paths.getFrames` chooses the right one.
- Shaders (`backend/shaders/`): `BloomEffect` (765), `WiggleEffect`, `OverlayShader`, `ScriptableShader`, `CustomShader`, `FunkinShader`.
- Cameras: gameplay camera, `HudCamera`, `FakeCamera`; ratio scale mode 1280×720.
- Optional 3D through `foxlite` (`THREE_D_SUPPORT`), optional Nape physics, optional ImGui.

### 2.8 Audio & video
- `FlxSound` (OpenAL through Lime). Music = `inst` + optional `vocals`, both **audio clocks**; per-strum-line `vocalsSuffix`; per-song `instSuffix`/`vocalsSuffix`; `Options.songOffset`.
- `backend/utils/AudioAnalyzer.hx` (691) — waveform/FFT for the charter.
- `hxvlc` for MP4 cutscenes (`VIDEO_CUTSCENES`), `lua/` folder ships Lua modules.

### 2.9 Menus, options, editors, save, weeks
- Menus (9): `TitleState`, `MainMenuState`, `StoryMenuState`, `FreeplayState`, `PauseSubState`, `ModSwitchMenu`, `WarningState`, `PlaytestingWarningSubstate`, `GitarooPause`; `menus/ui/Alphabet.hx` + effects.
- Options (27 files): `Options.hx`, `OptionsMenu.hx`, `TreeMenu`, `PlayerSettings`, typed options (13), keybinds (3), categories (6).
- Editors (98 files): charter (29 incl. `Charter.hx` 2,812, waveform, selections, bookmarks, event/meta/variation screens), character (11), stage, alphabet, shared UI (31), undo list, save warnings.
- `savedata/FunkinSave.hx` — one save module for options/progress; `SAVE_PATH CodenameEngine`, `SAVE_NAME save-default`.
- `backend/week/{Week,WeekData}.hx` — story weeks from `data/weeks/`.

## 3. External Haxe dependencies → Rust candidates

| Haxe library | Used for | Rust direction | Notes |
| --- | --- | --- | --- |
| `flixel`, `flixel-addons` | sprites, cameras, groups, `FlxActionSet` input | **replace** with `fu-render3d`/`fu-render3d-wgpu` + new `fu-input` | Existing wgpu stack stays. |
| `openfl` / `lime` | window, GL context, audio device, asset system | winit + wgpu (present) + rodio (present) | Same role, different APIs. |
| `flixel-animate` | Adobe Animate texture atlases | custom reader (JSON + spritemap) in `fu-assets` | Needed for girlfriend/BF in several songs. |
| `hscript-improved` | mod scripting | **architectural decision** — see §7 | Cannot be translated; needs interpreter choice. |
| `hxvlc` | MP4 cutscenes | `ffmpeg-next` or ffmpeg CLI, deferred | Only 8 shipped cutscenes use it. |
| `hxdiscord_rpc` | rich presence | `discord-rich-presence` (optional) | Deferred. |
| `nape-haxe4` | physics | `rapier2d` if ever needed | Not used by base gameplay. |
| `foxlite` | optional 3D stages | existing `fu-render3d` | Already 3D-capable. |
| `hxWebSockets`, `markdown`, `hxcpp-debug-server` | net, docs viewer, debug | `tokio-tungstenite`, `pulldown-cmark`, built-in | Low priority. |

## 4. Haxe module → Rust crate mapping

Legend: **Reuse** = crate already does it; **Adapt** = exists but needs redesign; **New** = crate/module must be created; **Defer** = not needed for playable parity.

| Haxe subsystem | Rust target | Strategy |
| --- | --- | --- |
| `backend/system/Main`, `FunkinGame`, `MainState` | `fu-play` binary + `fu-core` app shell | Adapt |
| `MusicBeatState/Substate/Group/Transition` | `fu-play::state` (state stack) + `fu-play::session` | New |
| `Conductor`, `BPMChangeEvent` | **new `fu-timing`** (`conductor`, `bpm_map`) | New (high fidelity port) |
| `Controls`, `options/keybinds` | **new `fu-input`** (action map, rebinds, gamepads) | New |
| `Paths`, `AssetSource`, libraries | `fu-vfs` (layered mounts) + `fu-assets` | Reuse/Adapt |
| `ModsFolder`, `ZipFolderLibrary`, `autoload.txt` | `fu-vfs` (zip mounts) + `fu-import::codename` | Adapt |
| `Chart`, `ChartData`, 4 parsers, meta resolution | **`fu-chart` v2** | Extend existing stub |
| `PlayState` loop | `fu-play::{game,session}` | Adapt (currently fixed 4 lanes) |
| `Note`, `Strum`, `StrumLine`, `NoteGroup` | `fu-play::notes` | New |
| `Character`, `Stage`, `HealthIcon` | `fu-play::{characters,stage,hud}` | New |
| `scoring/RatingManager`, `HitWindowData` | `fu-play::scoring` | New (constants verified) |
| `backend/shaders/*` | `fu-render3d` post-fx pipeline | Adapt |
| `backend/scripting/*` (62 files) | **new `fu-script`** (event bus + sandbox) | New, phased |
| `options/*` | `fu-config` + host UI | Adapt |
| `savedata/FunkinSave` | `fu-storage` | Adapt |
| `backend/week/*` | `fu-chart::week` | New (small) |
| `menus/*` (9 states, Alphabet) | **existing Flutter UI** (do not port Codename UI) | Not ported |
| `editors/*` (98 files) | `fu-project` data model + existing Flutter UI | Adapt, no 1:1 port |
| `editors/charter` waveform | `fu-play`/editor backend | Defer |
| `system/{updating,github,net,console}` | new `fu-platform` (optional) | Defer |
| `utils/{CoolUtil,MathUtil,XMLUtil,IniUtil}` | `fu-core`/`fu-assets` helpers | Port piecemeal |

## 5. Compatibility contract (must not regress)

Assets and formats that define "Codename-compatible":

1. `songs/<name>/meta.json` (+ `meta-<variant>.json`, `meta-<difficulty>.json`, `meta-<variant>-<difficulty>.json`) with the exact defaults listed in §2.4.
2. Four chart layouts: Codename `strumLines`, legacy `song.notes[].sectionNotes`, Psych v1 (`format: psych_v1*`), V-Slice (`version` + `scrollSpeed`). Detection order per `Chart.detectChartFormat`.
3. `songs/<name>/charts/<difficulty>.json`, auto-detected difficulties, easy/normal/hard reorder.
4. `songs/<name>/song/Inst[-suffix].ogg`, `Voices[-suffix].ogg`; `needsVoices`.
5. Character XML (`data/characters/*.xml`): `<anim name anim label x y>` + unknown attributes.
6. Stage XML (`data/stages/*.xml`): sprites with scroll/zoom/alpha/scale, `<boyfriend|girlfriend|dad|character name>` anchors, draw order.
7. Week XML (`data/weeks/`), `weeks.txt` ordering.
8. `mods/autoload.txt`, folder and zip mods, per-mod INI config, asset override order.
9. Events: `BPM Change`, `Time Signature Change`, `Continuous BPM Change`, plus the 53 scripting event names.
10. Save format (`CodenameEngine/save-default`).

Acceptance for "compatible": the same chart yields the same note set, the same step/beat/measure at the same ms, the same judgement at the same delta, and the same asset chosen at the same path — verified against golden values extracted from `D:\idk\Codename\assets` (read-only).

## 6. Gap analysis vs. the current Rust workspace

Existing (verified by inspection): `fu-import` (13.0k LOC; lossless Codename + V-Slice read-only models), `fu-assets` (7.2k; Sparrow XML atlas, default pack, texture/atlas/cache), `fu-play` (4.9k; 4-lane judgement, session, visuals, single-track audio clock), `fu-vfs` (4.0k; layered mounts, case-insensitivity), `fu-render3d`/`-wgpu` (4.3k), plus stubs (`fu-chart` 40 LOC, `fu-config` 29, `fu-project` 384, `fu-engine` 139, `fu-core` 4).

| Codename capability | Current Rust | Gap |
| --- | --- | --- |
| Chart runtime model + 4 format parsers | `fu-import` parses 2 formats read-only; `fu-chart` is a 40-line metadata stub | **Missing runtime model**, Psych/V-Slice unified, variants |
| BPM/time-signature map, continuous BPM | single `bpm: f32` | **Missing `fu-timing`** |
| Step/beat/measure + hit signals | none | Missing |
| Scoring presets (4 verified presets, scaling) | hardcoded 50/100/150 ms | Missing presets + accuracy/rating |
| Variable strum lines, opponent/coop, ghost tapping | fixed 4 player lanes | Missing |
| Sustains/holds (parse, clip, judge, score) | parsed then dropped | Missing |
| Vocals track + per-line suffix + offsets | instrumental only | Missing |
| Character XML animation playback | generated default character only | Missing |
| Stage XML | single generated backdrop | Missing |
| Adobe Animate atlases | Sparrow only | Missing |
| Note skins/note types/splash | generated notes | Missing |
| Scripting (HScript) + 53 events | none | Missing (decision needed) |
| Options/rebinds/save | stubs | Missing |
| Weeks/story/freeplay | none | Missing |
| Editors | `fu-project` model only | Out of scope for 1:1 port |
| Updater/Discord/video/console | none | Deferred |

## 7. Roadmap

Phases are ordered by dependency. Each ends with headless, reproducible checks; no phase claims behavior without one.

**P0 — Reference & golden data (done in this milestone)**
Vendored source, `docs/codename_module_inventory.csv`, verified constants and format rules.

**P1 — Timing & chart runtime (`fu-chart` v2 + `fu-timing`)** ← recommended first (§8)

Status: **done for timing and the runtime model**. `crates/fu-timing` (see `docs/TIMING.md`) ports the BPM map, all three timing events, Conductor update/signals and the four judgement presets; `crates/fu-chart` (see `docs/CHART_RUNTIME.md`) now provides the `RuntimeChart` that both importers project into, and gameplay judges against its `BpmMap`. Remaining chart work is chart-editing/serialization and the systems above gameplay.
`ChartData`, `ChartMetaData`, 4 parsers behind one `detect_format`, meta/variant/difficulty resolution, `BpmMap` + `Conductor` (exact formulas), `HitWindowPreset`. CLI `fu-chart-check`.
Exit: for ≥10 real songs, parsed note counts and `bpmChangeMap` match golden JSON; step/beat/measure at sampled ms match a reference table; unit tests for all four formats.

**P2 — Note model & judgement parity (`fu-play`)**
Variable strum lines, sustains/holds, opponent/coop, ghost tapping, 4 hit-window presets, accuracy/rating, health/death/retry, campaign counters. Depends on P1.
Exit: headless simulation over real Codename charts reproduces counts per judgement preset; sustain tests.

**P3 — Audio graph**
Inst + vocals (+ per-line suffix), offsets, pause/resume/restart, audio as the clock, drift check. Depends on P1.
Exit: `--audio-check` from P1 output on ≥3 real songs; vocal-lane mapping test.

**P4 — Rendering parity**
Sparrow **and** Animate atlas runtime, character XML animations/offsets, stage XML, note skins/types, cameras, downscroll, HUD/health icons. Depends on P2.
Exit: deterministic frame tests for receptor/note/stage/character; visual comparison notes per song.

**P5 — Assets & mods parity**
`autoload.txt` order, zip mods, per-mod INI, override precedence, translations. Depends on P1.
Exit: fixture mod tree resolves the same path as Codename for a table of cases.

**P6 — Scripting & events** (largest unknown)
Typed event bus mirroring the 53 event classes; declarative JSON events; sandboxed mod language; script packs/global scripts; softcoded states (last). Depends on P2/P4.
Exit: the shipped content scripts that matter for gameplay (`tankmenRun`, note types, stage scripts) run or fail with an explicit "unsupported API" error — never silently.

**P7 — Options, input, save, host UI integration**
Rebinds, downscroll/ghost-tapping options, save round-trip, Flutter UI consumes engine state. Depends on P3/P6.

**P8 — Weeks/story/freeplay data**: week XML, playlists, progression. Depends on P1/P7.

**P9 — Editors**: charter/character/stage as data + existing Flutter UI. Depends on P1/P5/P8. Explicitly **not** a port of Codename's editor UI.

**P10 — Platform extras**: updater, Discord RPC, video cutscenes, crash reporter, dev console. Optional.

## 8. Recommended first subsystem: timing + chart runtime (P1)

**Why this one**
1. **It is the compatibility keystone.** Every other parity claim (notes, sustains, cameras, events, editor) is expressed in steps/beats/measures produced by `Conductor`. If the map is wrong, everything downstream is wrong.
2. **It is pure, deterministic logic** — no GPU, no audio device, no window — so it is fully testable headlessly on this machine and in CI.
3. **Real fixtures already exist**: 20 song folders with 23 Codename charts plus legacy charts in `D:\idk\Codename\assets\songs`.
4. **It reuses finished work**: `fu-import::codename` already produces a lossless model (`Chart`, `StrumLine`, `Note`, `SongMetadata`); P1 is a *runtime interpretation layer*, not a second parser.
5. **It unblocks P2–P5 simultaneously** and removes the biggest false-confidence risk (a chart that "loads" but drifts).

**Concrete P1 plan**
- `crates/fu-timing/`: `conductor.rs` (port §2.2 semantics exactly, including the continuous-BPM integral), `bpm_map.rs`, `hit_windows.rs` (4 presets + scaling), `time_signature.rs`.
- `crates/fu-chart/src/model.rs`: `ChartData { meta, events, strum_lines, notes }`, `Note { strum_time, lane, sustain_ms, note_type, extra }`, variant/meta resolution.
- `crates/fu-chart/src/parsers/{codename,legacy,psych,vslice}.rs` + `detect.rs` implementing `Chart.detectChartFormat` verbatim.
- `crates/fu-chart/src/convert.rs`: `From<fu_import::codename::Chart>` / `From<fu_import::vslice::Chart>` so import and runtime cannot diverge.
- `apps/` or `fu-play --chart-check`: prints note counts, difficulty list, BPM change table, and step/beat/measure samples as JSON.
- Tests: `tests/golden/*.json` generated from a reference script pinned to commit `c2b73fa`, plus unit tests per parser and per conductor formula.
- Non-goals for P1: judgement changes, rendering, scripting, audio. `fu-play` keeps working as-is.

**Explicitly deferred**
- Full HScript compatibility (architectural decision required: Lua via `mlua` as the supported sandboxed language vs. an HScript subset interpreter vs. declarative events only). This must be decided before P6 starts.
- Codename's menu/editor UI: the existing Flutter UI stays; only data and engine state are exposed to it.
- Video cutscenes, updater, Discord, dev console.

## 9. Risks & open decisions

1. **Scripting compatibility is the largest risk.** Codename mods lean on hscript-improved (imports, static/public vars, softcoded states). No Rust equivalent exists. Deciding the story early prevents wasted ports.
2. **Rendering fidelity**: Sparrow vs Animate, per-character offsets, stage parallax and camera zooms are what makes gameplay "look like Codename"; these need reference screenshots from the real engine.
3. **Audio timing**: Codename uses the music stream's `time` as `songPosition`; `rodio`'s position is sink-based. P3 must validate drift with and without vocals.
4. **Continuous BPM changes** are rare but have exact math; port the formula, not an approximation.
5. **Editors are 98 files** and mostly UI; porting them 1:1 would violate the "keep the Claude-designed UI" constraint. Treat as data + Flutter.
6. **Upstream drift**: pin the reference commit and re-diff deliberately.

## 10. Reproducing this inventory

```bash
# read-only reference (never committed; .reference/ is git-ignored)
git clone --depth 1 --branch main --filter=blob:none --sparse \
  https://github.com/CodenameCrew/CodenameEngine.git .reference/CodenameEngine
cd .reference/CodenameEngine && git sparse-checkout set source

# per-file inventory
cat docs/codename_module_inventory.csv

# scale
find source -name '*.hx' | wc -l            # 369
find source -name '*.hx' -exec cat {} + | wc -l   # 64655
```
