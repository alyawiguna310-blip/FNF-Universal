# Asset pipeline audit (checkpoint c8551e6)

Audit of the path from virtual file system to the playfield renderer, written
against branch `feature/wgpu-renderer-integration` at `c8551e6`. Findings marked
**verified against data** were checked against files in the Codename reference
repository; findings marked **from code reading** were not executed.

Nothing here was compiled or run. The author had read/write access to GitHub
only, with no Rust toolchain.

## How content reaches the screen today

Two parallel paths exist.

1. **Default pack path.** `GameResources` mounts the generated default pack, an
   optional user pack and the selected mod as `fu-vfs` layers (priority 0, 10,
   20), resolves logical keys such as `note.left` through `ResourceStack`, and
   loads pixels through `AssetCache`. This is the intended architecture.
2. **Codename path.** `CodenameContent` reads a Codename `assets` directory and a
   mod root with `std::fs`, parses stage and character XML with its own scanner,
   decodes PNGs directly, and overwrites keys with
   `GameResources::install_texture`. It does not use `fu-vfs`, `AssetCache` or
   the manifest, and the Codename `assets` directory is never mounted in the VFS.

`visuals.rs` consumes both and uploads textures to the renderer by cloning pixel
buffers.

## Findings

### Fixed on this branch

| Finding | Evidence | Status |
| --- | --- | --- |
| Trim offsets applied with the wrong sign when drawing character frames | **Verified against data.** `down press0000` in `assets/images/game/notes/default.xml` is 142x140 with `frameX=-4 frameY=-2 frameWidth=149 frameHeight=146`; the old code shifted it 7.5 px left of its source frame, Sparrow's convention shifts it 0.5 px right | `AtlasFrame::centre_offset`, used by `build_rig` |
| Non-looping poses always drew frame 0, and looping poses used absolute song time | **From code reading** (`codename_characters`). Character `loop` defaults to off, so sing poses never advanced | Per-rig start time, token per triggering note, `animation_frame_index` |
| Stage sprite names, folders and character ids could contain `..` and escape the mod folder | **From code reading** (`find_file` joined them unchecked) | `safe_relative` guard on every lookup |
| Mod health icons never loaded | **Verified against data.** Vs Var Overhaul ships `images/icons/neu.png`; the loader only tried `images/icons/<name>/icon.png`, which is the base installation's layout | `icon_image` accepts both layouts, mod first |

### Open

| Finding | Evidence | Why it is not fixed here |
| --- | --- | --- |
| `frameX`/`frameY` are read as integers, so a fractional value silently becomes `0` | **Verified against data.** `right confirm0000` has `frameX="-1.5"` | Needs `AtlasFrame.offset` to become `f32`, which changes a public field used outside the files touched here |
| Codename content bypasses `fu-vfs`, `AssetCache` and the manifest | **From code reading** | Needs the Codename `assets` directory mounted as a VFS layer and `CodenameContent` reading through it; also changes `song_meta_path`, whose callers were not inspected |
| Three separate hand-written XML scanners (`atlas.rs`, `codename_content.rs`, and the importer's) | **From code reading** | `fu-import` already depends on a real XML parser; `fu-assets` should reuse it so entities, CDATA and attribute quoting are handled once |
| Animate atlases (`images/characters/newBF/Animation.json`, used by the reference mod's `newbf`/`newgf`) are not loaded by the playfield | **Verified against data.** The reference mod has them; `fu-assets` has an `AnimateAtlas` parser, but `load_character` only looks for Sparrow `.png` plus `.xml` | Needs a character path that renders Animate quads |
| Countdown art mapped 1:1 onto four keys | `assets/images/game` has three images (`ready`, `set`, `go`); `apply_skin` maps them to `countdown.3/2/1` and leaves `countdown.go` generated. Whether the classic sequence is audio-only for "3" could not be verified without Codename source | Needs confirmation against Codename's countdown behavior |
| Health icons draw a whole icon sheet into a square | **From code reading.** `icon.png` is a multi-state sheet in Codename; its frame layout was not inspected | Needs the sheet dimensions and Codename's icon frame rules |
| `CharacterRig::frame_at` still returns frame 0 for non-looping poses | **From code reading.** Unused by `visuals.rs` | Its `song_ms` parameter has no start time; callers were not inspected |
| Mirrored characters do not mirror the trim offset | **From code reading** | Codename's behavior was not verified |
| Decoded textures are held three times (cache, `GameResources`, renderer upload) and uploaded once per lane | **From code reading** | Ownership change across crates |
| `CodenameContent::apply_skin` prints to stdout | **From code reading** | Cosmetic |

## Assumptions to verify locally

* Sparrow's draw offset is `(-frameX, -frameY)`. This matches the numbers in
  Codename's note atlas (the trimmed rectangle sits inside its source frame only
  with that sign) and Flixel's Sparrow loader as remembered by the author, which
  was not re-read.
* A character `<anim>` without `loop` does not loop (kept from the existing
  loader, not verified against Codename).
* The hit and miss feedback windows last at least 40 ms. The new rig test samples
  at 1040 ms after a hit at 1000 ms, as an existing test does.

## Verification commands

```
cargo test -p fu-assets
cargo test -p fu-play
cargo clippy --workspace --all-targets -- -D warnings
```

Graphical output was not checked. The renderer tests create a headless wgpu
device and may need the serialization lock described in commit `acb14ca`.
