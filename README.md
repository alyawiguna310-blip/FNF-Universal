# FNF Universal — Editor/Engine Contract v0.1

This package defines a shared mod format and API contract for the Flutter editor and Rust engine.

**Scope:** specification and starter schemas/examples. It is not a finished game engine, a working script VM, or a drop-in replacement for the existing foundation. The runtime bindings and editor screens still need implementation and tests.

## Design rules

1. One mod project format is shared by the editor, validator, importer and runtime.
2. Visual editing and scripting are two ways to author the same project data.
3. Input/controller configuration is player-owned and separate from note rendering.
4. Note visuals can move independently from gameplay timing and judgment.
5. Mods may add mechanics and settings, but player accessibility preferences remain protected.
6. Imported scripts are inspected, never executed during conversion.
7. Scripts run only in a sandbox with explicit capabilities and resource limits.
8. Android storage uses the Storage Access Framework (SAF); do not assume unrestricted filesystem access.

## Package contents

- `schemas/`: JSON Schemas for project, note visuals, mod settings and enemy definitions.
- `examples/`: sample mod project with scripted note movement and a boss mechanic.
- `docs/`: editor/runtime contract, scripting model, security and platform capabilities.
- `rust-prototype/`: dependency-free Rust data model showing the separation between note visuals and gameplay state.

## Suggested integration

Copy the `schemas`, `examples`, and `docs` folders into the repository. Treat `rust-prototype` as a reference module, not as a replacement for existing crates. Before wiring it into the project, align crate names and Rust edition with the actual workspace.

## Validation

The JSON files are intended to be valid JSON. The schemas use JSON Schema Draft 2020-12. Validate examples with a Draft 2020-12 validator before making them part of a release.
