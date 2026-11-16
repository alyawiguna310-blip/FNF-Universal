# Default resources & asset pipeline

The playable runtime ships with a **generated** default resource pack and loads
everything through a manifest, so gameplay never mentions a file path and a mod
or resource pack can replace any sprite, sound or panel.

## Layers (lowest priority first)

| Priority | Layer | Source |
| --- | --- | --- |
| 0 | Default pack | generated in memory by `fu_assets::default_pack`, mounted read-only |
| 10 | Resource pack | optional directory passed with `--assets <dir>` |
| 20 | Mod | the `--mod <dir>` V-Slice mod |

All three are ordinary VFS mounts at `/`, so the existing overlay rules apply:
the highest-priority mount that contains a path wins, and anything it lacks falls
through to the layer below. A mod overrides `note.left` simply by shipping
`defaults/images/notes/note_left.png`; no code or configuration change is needed.

## Manifests

A manifest maps a **logical key** to a **virtual path**:

```json
{
  "name": "My Pack",
  "version": "1.0.0",
  "assets": {
    "note.left": "/my-images/notes/arrow_left.png",
    "stage.default": "/my-images/stages/factory.png"
  }
}
```

Two manifests are read, in order:

1. `/defaults/manifest.json` — the default pack's own manifest (a mod may replace
   this file to remap default keys).
2. `/assets/manifest.json` — an extra manifest supplied by a pack or mod, layered
   on top.

Every entry is validated when it is parsed: keys must be non-empty and contain no
whitespace, and paths must be absolute virtual paths (`/...`) that survive the
VFS traversal checks.

## Keys used by the runtime

| Category | Keys |
| --- | --- |
| Notes | `note.left`, `note.down`, `note.up`, `note.right` |
| Receptors | `receptor.left`, `receptor.down`, `receptor.up`, `receptor.right` |
| Stage | `stage.default` |
| Character | `character.default.image`, `character.default.atlas` |
| HUD | `hud.health.frame`, `hud.health.fill`, `hud.icon.player`, `hud.icon.opponent` |
| Countdown | `countdown.3`, `countdown.2`, `countdown.1`, `countdown.go` |
| Judgements | `judgement.perfect`, `judgement.good`, `judgement.bad`, `judgement.miss` |
| Screens | `ui.menu.background`, `ui.pause.panel`, `ui.results.panel` |
| Sounds | `sfx.countdown`, `sfx.hit`, `sfx.miss` |

The key list lives in `fu_play::resources::keys`; that module is the only place
gameplay code refers to resources.

## What the default pack contains

The pack is drawn and synthesised from code in
`crates/fu-assets/src/default_pack.rs`, so it contains **no proprietary
Friday Night Funkin' assets** and carries the project's own licence. It is
installed at priority 0 and can be replaced file-for-file.

| File | Contents |
| --- | --- |
| `images/notes/note_<lane>.png` | 128×128 arrow note per lane, drawn in that lane's colour |
| `images/receptors/receptor.png` | 128×128 white/grey arrow, tinted per lane and per hit state |
| `images/stage/stage.png` | 1280×720 stage backdrop with speakers, platform, crowd |
| `images/hud/healthbar_frame.png` | rounded health-bar frame |
| `images/hud/healthbar_fill.png` | red→green health fill |
| `images/hud/icon_player.png`, `icon_opponent.png` | character icon faces |
| `images/characters/character.png`, `character.xml` | 2×2 spritesheet + Sparrow atlas: two idle frames that breathe and two sing frames |
| `images/countdown/{3,2,1,go}.png` | countdown labels |
| `images/judgement/{perfect,good,bad,miss}.png` | judgement labels |
| `images/ui/menu_background.png` | loading/menu backdrop with the game logo |
| `images/ui/pause_panel.png`, `results_panel.png` | pause and results panels |
| `sounds/{countdown,hit,miss}.wav` | 16-bit mono synthesised blips |

## Supported formats

| Kind | Supported | Notes |
| --- | --- | --- |
| Images | 8-bit PNG (RGBA, RGB, grayscale, grayscale+alpha) | 16-bit PNGs are rejected with a clear error instead of being truncated |
| Atlases | Sparrow XML (`<TextureAtlas>` + `<SubTexture>`) | frames, trim offsets, source size, `rotated` flag, numeric-suffix animation ordering, per-frame UV meshes |
| Animations | `Animation` from atlas prefixes | frame-rate driven playback, looping or clamped, used by the default character (idle loop + sing pose) |
| Audio | whatever `rodio` decodes — Ogg Vorbis and WAV | sound effects are loaded and validated; only the instrumental is played back today |
| Adobe Animate atlas | not parsed | a zip + `spritemap1.json` bundle is inventoried by the importer only |
| Fonts | not parsed | the HUD uses a built-in 3×5 bitmap font |

## Loading, caching and fallback

`fu_play::resources::GameResources::load(mod, pack)` builds the VFS, resolves the
manifest stack, and eagerly decodes every key listed above through
`fu_assets::AssetCache`:

- **Caching**: textures are `pin`ned in the cache, which keeps them resident for
  the whole run and out of the LRU eviction ring. A `SparrowAtlas` is parsed once
  per virtual path.
- **Fallback**: a resource that is missing, unreadable or undecodable is replaced
  with a magenta/black checker texture (impossible to miss) and recorded in
  `GameResources::missing()`. Loading never aborts the game.
- **Diagnostics**: `GameResources::summary()` reports how many textures and
  sounds loaded, from how many layers, and what was missing; the runner prints it
  at startup, and `--assets` problems are reported the same way.
- **Separation**: this module owns pixels and bytes. `fu_play::visuals` uploads
  them to the GPU and knows nothing about manifests or paths.

## Adding or replacing resources

1. **Quick override** — drop the file at the same virtual path inside your mod or
   pack, e.g. `mods/MyMod/defaults/images/notes/note_left.png`. The VFS picks it
   up automatically.
2. **Full pack** — create a directory containing `assets/manifest.json` plus your
   images/sounds, and run with `--assets <dir>`.
3. **New keys** — add an entry to your own `assets/manifest.json` and read it
   with `GameResources::texture("your.key")`.

## Verification

`cargo test -p fu-assets` covers PNG round-trips, atlas parsing and animation
ordering, manifest validation, layer precedence and the generated pack (every
manifest key resolves to a generated file, every texture decodes with visible
pixels, lane arrows point the right way, sounds are valid RIFF/WAVE).

`cargo test -p fu-play` covers the runtime side: the default pack satisfies every
runtime key, a mod file shadows a default texture with no manifest, a pack
manifest remaps a key, a missing resource falls back and is reported, atlases
load through the VFS once, the default character sheet loads with both idle and
sing animations, the character animates and switches to its sing pose on a hit,
and a headless GPU frame proves the stage, receptor, note and HUD pixels come
from loaded textures rather than flat placeholders.
