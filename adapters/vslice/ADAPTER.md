# V-Slice adapter

Two halves, deliberately independent:

## Data import (implemented)

`fu_import::vslice` reads a V-Slice mod directory into a typed, lossless model
plus a diagnostic report: mod metadata (`_polymod_meta.json` or `manifest.json`),
song metadata and charts (`2.2.x` / `2.0.x`), characters, stages, story weeks,
assets and unknown fields. Scripts and native binaries are inventoried and never
executed.

See `docs/VSLICE_ADAPTER.md` for the folder layout, support classifications,
safety model and known limitations. Compatibility is reported per finding; it is
not claimed for real mods until fixtures from a real V-Slice mod are added.

## Runtime integration (planned)

Initial target: official public V-Slice source as an independently built runtime.

Required work:
1. Pin a specific upstream commit/tag.
2. Reproduce a clean build for each target platform.
3. Record Lime/OpenFL/Haxe/HaxeFlixel and native extension versions.
4. Define launch arguments, working directory, asset root, save root and exit behavior.
5. Add smoke tests before marking `integrated`.

Do not assume the Play Store APK's version maps exactly to a public source tag.
