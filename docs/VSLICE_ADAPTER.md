# V-Slice data adapter

`fu_import::vslice` reads a V-Slice (Friday Night Funkin') mod directory into a
typed, lossless Rust model plus a diagnostic report. It is the data half of the
V-Slice adapter: it inspects mod content and never launches a game.

The runtime half of the adapter (pinning a build, launch arguments, exit
behaviour) is still planned and is described in `adapters/vslice/ADAPTER.md`.

## Status

| Aspect | State |
|---|---|
| Mod folder discovery | Implemented |
| Mod metadata | Implemented (`_polymod_meta.json`, `manifest.json`) |
| Song metadata | Implemented (format `2.2.x`) |
| Song charts, notes, events, scroll speed | Implemented (format `2.0.x`) |
| Characters | Implemented (JSON `CharacterData` shape) |
| Stages | Implemented (`props` and `characters`; legacy `layers` preserved) |
| Story weeks | Implemented |
| Asset references and inventory | Implemented |
| Unknown properties | Preserved verbatim on every structure |
| Scripts and native binaries | Inventoried only, never read or executed |
| Verified against a real mod | **Not yet** — see limitations |

## Folder layout understood

```text
<mod>/
  _polymod_meta.json          # or manifest.json
  songs/<id>/
    <id>-metadata.json        # also metadata.json, and <id>-metadata-<variation>.json
    <id>-chart.json           # also chart.json, and <id>-chart-<variation>.json
    charts/<name>.json        # fallback when the song directory has no chart file
    Inst.ogg                  # instrumental; Inst-<variation>.ogg for variations
  characters/*.json           # alternatively data/characters/*.json
  stages/*.json               # alternatively data/stages/*.json
  weeks/*.json                # alternatively data/weeks/*.json
  images/...
  scripts/...
```

Discovery is flat: every directory inside `mods/` that contains a recognized
metadata file is a mod. A `_polymod_meta.json` is preferred over a `manifest.json`
when both exist. Directories without either file are reported at info severity
and skipped, because they may belong to another engine.

## What each structure keeps

Every structure stores `raw` (the decoded JSON it was built from) and
`unknown_fields` (every key the importer does not interpret). Typed fields are a
convenient view over that data, not a replacement for it:

* `ProjectMetadata` — id, title, description, homepage, `api_version`,
  `mod_version`, license, icon, authors, contributors, dependencies.
* `SongMetadata` — version, song name, artist, charter, time format, looped,
  divisions, offsets, BPM changes, and `playData` (album, preview range, ratings,
  variations, difficulties, characters, stage, note style).
* `Chart` — version, scroll speed per difficulty, notes per difficulty, events.
* `Note` — `t` (ms), `d` (direction; `0..=3` player, `4..=7` opponent), `l`
  (sustain ms), `k` (kind) and `p` (parameters). Values that are not numbers are
  kept verbatim rather than coerced.
* `Character` — name, render type, asset path, scale, flags, offsets, dance and
  sing timing, health icon, death data, atlas settings, animations.
* `Stage` — name, directory, camera zoom (`cameraZoom` or `defaultZoom`), pixel
  flag, props, character placements, and a legacy `layers` array.
* `Week` — story name and song ids.

Documents that cannot be decoded at all (invalid JSON, a non-object document, a
typed-shape mismatch) are reported and their raw text is kept in
`VsliceMod::unparsed`, so nothing is silently dropped.

## Support classification

Findings use the shared `DiagnosticKind`/`Severity`/`SupportLevel` types:

| Finding | Meaning |
|---|---|
| `unsupported-format-version` | Declared version is outside `2.2.x` (metadata) or `2.0.x` (chart), or `timeFormat` is not `ms`. Data is still read, but compatibility is unverified. |
| `missing-difficulty` | Metadata declares a difficulty the default chart has no notes for. |
| `undeclared-difficulty` | A chart has notes for a difficulty the metadata does not declare (informational). |
| `missing-song-audio` | No `Inst.ogg` in the song directory; the base game may provide it. |
| `invalid-note` / `invalid-event` | Malformed timestamp, direction or sustain; the offending object is preserved verbatim. |
| `missing-character-reference` / `missing-stage-reference` | The song names a character or stage this mod does not define. Reported as unverified, because V-Slice mods are override layers over the base game. |
| `missing-asset` / `case-mismatch` | A referenced sprite was not found, or exists with different casing. |
| `unsafe-path` | A referenced asset path is absolute, traversal-based or otherwise unusable. It is never resolved. |
| `duplicate-definition` | Two songs, characters, stages or weeks share an id (case-insensitive). |
| `unsupported-script` / `unsupported-binary` / `unsupported-archive` | Content that is inventoried but never executed or expanded. |

`ImportReport::overall_support` rolls the actionable findings up into a single
`SupportLevel`; informational findings such as script inventory do not downgrade
an otherwise supported import.

## Safety model

* Every file read goes through `fu_import::path::read_text_within`, which proves
  the target is inside the mod root after canonicalization.
* Scanning never follows symbolic links, and a symlinked metadata file is not
  accepted as mod metadata.
* Asset paths that come from mod JSON are parsed as `SafeRelativePath` before any
  lookup, so `../../outside` is reported instead of resolved.
* Scripts, native libraries and archives are recorded by path and size only.
* The crate contains no `unsafe` code and performs no network access.

## Usage

```rust
use fu_import::vslice::import_vslice_mod;

let outcome = import_vslice_mod(std::path::Path::new("mods/My V-Slice Mod"))?;
println!("{}", outcome.report.to_text());
println!("{} songs, {} characters", outcome.module.songs.len(), outcome.module.characters.len());
# Ok::<(), fu_import::ImportError>(())
```

A whole `mods/` directory can be imported with `import_vslice_mods_root`.

## Known limitations

* The importer has been verified against synthetic fixtures built from the public
  V-Slice formats, not against a real V-Slice mod. Compatibility is therefore
  reported per finding and never claimed wholesale.
* Song `music/<id>/<id>-metadata.json` (separated music data) is inventoried as an
  asset but not interpreted; song metadata is read from the song directory.
* Charts using the nested `{ "song": { ... } }` layout (Psych-family charts) are
  reported and preserved, not converted.
* Legacy stage `layers` arrays are preserved but only `props` are interpreted.
* Atlases are recognized by filename, not decoded: frame rectangles, offsets and
  animation frames are not extracted.
* Character and stage sprites inherited from the base game are reported as
  unverified rather than missing.
* `VsliceMod` is not converted into `ProjectDocument`. The V-Slice model holds
  data the editor project schema cannot express yet (unknown fields per document,
  arbitrary event values, stage props, legacy layers), so a future skeleton
  conversion must state exactly what it drops.

## Verification

`cargo test -p fu-import` covers the V-Slice layer with unit tests in each module
and an end-to-end suite in `crates/fu-import/tests/vslice.rs`: full-mod import,
unknown-field preservation, manifest-only mods, mods root discovery, script and
binary inventory, variations, malformed documents, traversal rejection, the
`charts/` fallback, validation findings, format-version classification and model
JSON round-tripping.
