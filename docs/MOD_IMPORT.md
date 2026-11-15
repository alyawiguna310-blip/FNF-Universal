# Codename Engine Mod Import

`fu-import` reads a Codename Engine mod directory and produces a complete,
serializable model plus a diagnostic report. It is a read-only foundation: it
records what the mod contains and what the importer does not understand, and it
never executes, loads or renders imported content.

This document describes the importer as implemented. It is not a compatibility
claim. Anything that cannot be verified from the files alone is reported as
*unverified*.

## Architecture

```text
crates/fu-import/
  src/lib.rs                public API: import_mod, import_discovered, import_mods_root
  src/report.rs             format-agnostic Diagnostic / Severity / SupportLevel / ImportReport
  src/path.rs               SafeRelativePath, canonicalized containment, guarded reads
  src/ini.rs                lossless INI reader
  src/xml.rs                tolerant XML reader (no DTD or entity resolution)
  src/codename/
    mod.rs                  module wiring and re-exports
    model.rs                serializable Codename model
    discovery.rs            autoload.txt, mod resolution, non-following tree scan
    parse.rs                model construction and JSON/XML interpretation
    validate.rs             semantic checks over the parsed model
```

Generic report and path types are independent of Codename: a future importer
(another engine or chart format) reuses `report`, `path`, `ini` and `xml`
without touching `codename`.

Data flow:

1. `open_mod_dir` canonicalizes the selected directory.
2. `discover_mods_root` reads `mods/autoload.txt` (or scans the directory when
   the list is absent) and resolves each entry inside the mods root.
3. `scan_mod_tree` walks the mod with `follow_links(false)` and a depth cap.
4. `AssetInventory::build` sorts files, classifies them and recognizes atlases.
5. `parse_mod` interprets configuration, characters, stages, events and songs.
6. `validate_into` adds semantic findings.
7. `ImportReport::sort` makes the output deterministic.

## Supported formats

| Content | Source | Handling |
| --- | --- | --- |
| Mod list | `mods/autoload.txt` | newline separated, BOM, CRLF, blank lines and `#`/`;` comments |
| Mod metadata | `data/config/modpack.ini` | sections, quoted values, inline section comments, `#`/`;` comments |
| Mod flags | `data/config/flags.ini` | merged into the same flag map as `modpack.ini` |
| Characters | `data/characters/*.xml` | attributes, `<anim>` entries, unknown attributes and elements |
| Stages | `data/stages/*.xml` | attributes, `<sprite>`, `dad`/`boyfriend`/`gf`/`girlfriend` anchors, draw order |
| Song metadata | `songs/<name>/meta.json` | typed fields plus every unknown property |
| Song events | `songs/<name>/events.json` | event times, names and heterogeneous `params` |
| Charts | `songs/<name>/charts/*.json` | Codename `strumLines` layout, notes, events, unknown properties |
| Event definitions | `data/events/*.json` | parameter names, type expressions and default values |
| Audio | `songs/<name>/song/` | inventoried by kind and size |
| Assets | `images/`, `songs/`, `music/`, `sounds/`, `shaders/`, `fonts/`, `videos/` | inventoried, categorized, case-checked |

### INI details

* `[Section] # comment` keeps `comment` on the section.
* `KEY="value"` records `value` with `quoted = true`.
* `#` and `;` start a comment when separated from the value by whitespace, so
  `COLOR=#8E0533` stays a value.
* Repeated sections and repeated keys are preserved in file order; lookups use
  the last occurrence ("later wins").
* `[StateRedirects]` and `[StateRedirects.force]` are collected in load order.
* Entries before the first section land in a `""` (global) section with an
  issue recorded.

### Chart details

Charts are read as Codename `strumLines` documents. Preserved per strum line:
`keyCount`, `position`, `characters`, `type`, `visible`, `strumScale`,
`strumSpacing`, `strumPos`, `strumLinePos`, `vocalsSuffix` and `notes`.
Preserved per note: `time`, `id`, `sLen`, `type` and any extra property.
Chart-level fields `chartVersion`, `codenameChart`, `scrollSpeed`, `stage`,
`noteTypes`, `events` and `players` are read; everything else is stored in
`unknown_fields` and kept verbatim in `raw`.

`noteTypes` entries are kept exactly as JSON, so both `"No Animation"` and
`{ "name": "Alt", "anim": "alt" }` survive.

## Partial support

These are represented but cannot be imported faithfully, and are reported as
`partial`:

* **Atlases.** Sparrow pairs (`name.png` + `name.xml`) and Animate atlas folders
  (folder containing `Animation.json`) are recognized. Frames and animations are
  not decoded.
* **XML quirks.** Unquoted attribute values, mismatched closing tags, duplicate
  attributes and unknown entities are recovered with a diagnostic; the document
  is still usable.
* **INI quirks.** Unterminated quotes and lines without `=` are reported; the
  rest of the file is still read.
* **Stage semantics.** Sprites, parallax, zoom factors and anchors are stored,
  but ordering rules, animation selection and shader hooks are not modelled.
* **Character anchors.** `gf` and `girlfriend` both map to the girlfriend slot
  and both tags are preserved on the anchor.

## Unsupported features

Recorded, never executed or interpreted (`unsupported`):

* **Scripts.** `.hx`, `.lua` and other script files are inventoried as path,
  language and size. Their contents are never read, compiled or executed.
* **Native code.** `.dll`, `.ndll`, `.so`, `.dylib`, `.exe`, `.msi`, `.bat`,
  `.cmd`, `.sh`, `.jar` and similar files are listed in `unsupported_files` and
  never loaded.
* **Archives.** `.zip`, `.rar`, `.7z` and similar are listed and never expanded.
* **Legacy chart layouts.** Psych-style `{"song": {"notes": [...]}}` documents
  are detected, preserved verbatim and reported as `legacy-chart-format`. No
  conversion is attempted, so nothing is invented.
* **Mod code paths.** Shader scripts, state scripts, event scripts and global
  scripts are treated as data references only.

## Validation

`validate_into` runs over the parsed model. Every diagnostic carries a stable
`code`, a `severity` (`info`, `warning`, `error`) and a `support` classification
(`supported`, `unverified`, `partial`, `unsupported`).

| Area | Code | Meaning |
| --- | --- | --- |
| Assets | `missing-asset` | a referenced sprite, icon or stage image is absent |
| Assets | `case-mismatch` | the reference exists with different casing |
| Assets | `duplicate-case-name` | two files differ only by casing |
| Assets | `partial-atlas` | atlas recognized, frames not decoded |
| Redirects | `duplicate-state-redirect` | same state redirected twice to the same target |
| Redirects | `conflicting-state-redirect` | same state redirected to different targets |
| Redirects | `missing-state-reference` | redirect target is not in this mod (unverified) |
| Paths | `unsafe-path` | absolute, drive-qualified, traversing or ambiguous path |
| Characters | `missing-character-reference` | chart names a character this mod lacks (unverified) |
| Characters | `invalid-animation` | animation missing a name or `anim`, or duplicated |
| Stages | `missing-stage-reference` | chart names a stage this mod lacks (unverified) |
| Events | `undefined-event` | event not defined by this mod; may be built-in (unverified) |
| Songs | `missing-song-file` | instrumental, voices or a declared chart is absent |
| Charts | `invalid-chart` | missing/empty `strumLines`, lane out of range, missing time |
| Charts | `legacy-chart-format` | legacy layout preserved but not converted |
| Charts | `unknown-chart-field` | unrecognized property preserved |
| Metadata | `malformed-ini`, `malformed-xml`, `malformed-json` | recoverable syntax problems |
| Metadata | `malformed-metadata`, `missing-metadata` | unexpected or absent metadata |
| Metadata | `unknown-metadata` | section, key or property preserved but not interpreted |
| Metadata | `duplicate-definition` | identifier defined more than once (character, animation, sprite, anchor, strum line, event) |
| Scripts | `unsupported-script` | inventoried, never executed |
| Binaries | `unsupported-binary`, `unsupported-archive` | recorded, never loaded or expanded |

Unknown event, character and stage references are deliberately classified as
`unverified`: a mod may rely on Codename Engine built-ins, the base game or
another loaded mod, so the importer does not claim they are missing.

## Running the importer

```rust
use fu_import::{import_mod, import_mods_root};

// One mod directory.
let outcome = import_mod(std::path::Path::new("mods/Vs Var Overhaul [Demo]"))?;
println!("{}", outcome.report.to_text());
println!(
    "{} characters, {} stages, {} songs",
    outcome.module.characters.len(),
    outcome.module.stages.len(),
    outcome.module.songs.len()
);

// Everything listed by a mods/ directory.
let root = import_mods_root(std::path::Path::new("mods"));
for outcome in &root.mods {
    println!("{}: {}", outcome.module.name, outcome.support());
}
# Ok::<(), fu_import::ImportError>(())
```

`ImportReport::to_text()` renders a human summary; `summary()` returns counts and
the overall support classification for tooling. `overall_support()` aggregates
the weakest classification among warnings and errors only: informational notes
such as "58 script files were inventoried and not executed" keep their own
`unsupported` classification without downgrading the whole import.

Limits are configurable through `ParseOptions` (`max_depth`, `max_file_bytes`)
with `import_mod_with` and `import_mods_root_with`.

## Security boundaries

The importer treats every file as untrusted input.

* **Path validation.** All mod-relative paths become `SafeRelativePath`, which
  rejects absolute paths, drive letters, UNC prefixes, `..` components, NUL
  bytes, control characters, NTFS alternate data streams (`file:stream`) and
  Windows device names (`CON`, `NUL`, `COM1`, ...). Both `/` and `\` are treated
  as separators on every platform.
* **Containment.** Files are opened only through `read_text_within`, which
  canonicalizes the target and verifies it is still inside the canonical mod
  root. A junction or symlink pointing outside is refused.
* **No link following.** Tree scanning uses `WalkDir::new(root).follow_links(false)`;
  symlinks are recorded as `scan-symlink-skipped` and never traversed or read.
* **Bounded reads.** Recursion depth and per-file byte limits are enforced.
* **No code execution.** Scripts are never read; native libraries and
  executables are never loaded; archives are never expanded.
* **No XML entity expansion.** The XML reader decodes only the five predefined
  entities and numeric references. `<!DOCTYPE>` content is stored as text. A
  `SYSTEM` entity therefore stays literal instead of reading a file or URL.
* **No `unsafe`.** The crate sets `#![forbid(unsafe_code)]`.

## Current limitations

* Only the Codename `strumLines` chart layout is understood. Legacy note arrays
  are preserved but not converted.
* Atlases are recognized, not decoded: sprite frames, offsets and animations are
  not available.
* Shader, state and event scripts are not analysed, so script-driven behaviour
  cannot be validated.
* Sprite resolution is filename-based (`images/<folder>/<name>.png|.xml`, or an
  Animate folder). Mods that assemble images dynamically are not detected.
* Voices are checked as `song/Voices.<ext>`. Per-strum-line `vocalsSuffix`
  variants are recorded but not resolved.
* The model stores a display path for the mod root; it is not portable between
  machines.
* Validation describes the importer's own understanding. It does not prove that
  Codename Engine would render or play the mod identically.
* No `ProjectDocument` conversion is provided. The Codename model keeps data
  that the editor project schema cannot represent (unknown chart properties,
  arbitrary INI sections, atlas metadata), so any future skeleton conversion
  must document what it drops.

## Related

* `docs/VSLICE_ADAPTER.md` — the V-Slice importer (`fu_import::vslice`), which
  shares the diagnostics, path-safety and discovery layers described here.
* `docs/SECURITY_AND_IMPORT.md` — the import pipeline and compatibility labels
  every importer follows.
