# Shared Project Model

The `fu-project` crate is the first typed Rust representation of the editor/engine project contract. It provides serializable project, song, chart-note, character, stage, asset-reference, and creator-defined setting data.

## Ownership boundaries

- `NoteGameplay` owns lane identity, strum time, and sustain duration.
- `NoteVisualBinding` references renderer-side profiles and movement paths.
- Adding or changing visual metadata does not modify gameplay timing or judgment data.
- `ModSettingDefinition` belongs to the mod; engine-owned player preferences such as controller mode remain outside this model.

## Validation

`ProjectDocument::validate` checks the format/version, required identifiers and names, duplicate IDs, positive finite BPM, non-zero lane counts, and note lane bounds. Validation is data-only: it does not load assets or execute scripts.

## Current integration status

This crate is an initial shared model and validation boundary. The Flutter editor still has its existing version-1 in-memory map and local JSON save/load path; it has **not** been migrated to this Rust model or connected through FFI yet. The next integration step is to align the Flutter serializer with this model, then expose validation through the Rust bridge.

Run the checks on a Rust development machine:

```powershell
cargo fmt --all -- --check
cargo test --workspace
```
