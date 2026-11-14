# Account 1 Handoff — Renderer Foundation

`90% HANDOFF ALERT — ACCOUNT 1 READY` (with the verification caveat below)

## Honest status
- Written and verified on Windows: `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` passes (38 tests in fu-render3d).
- **Not done:** wgpu backend, WGSL builtin shaders, GLB/glTF loader, shadows, post effects. These are not stubbed on purpose.
- Verified glam 0.29.3 resolved and the API names used compile.
## Files
Created: `crates/fu-render3d/Cargo.toml`, `crates/fu-render3d/src/{lib,resources,transform,camera,mesh,shader,material,light,scene,quality,queue,backend,renderer}.rs`, `docs/RENDER3D.md`, this file.
Modified: root `Cargo.toml` (added member `crates/fu-render3d`). `Cargo.lock` will change when cargo resolves `glam`.

## Dependencies
Added: `glam = "0.29"` (crate-local). Reused workspace `serde`, `thiserror`, `serde_json` (dev). No graphics crate yet.
The glam 0.29 API names used (`Mat4::row`, `look_to_rh`, `perspective_rh`, `to_scale_rotation_translation`) are from
memory and unverified.

## Architecture decisions
1. Standalone crate; no dependency on any renderer, engine or Flutter code.
2. CPU-side core + `RenderBackend` trait, so wgpu can be a separate crate/feature and the core stays testable headless.
3. Right-handed, -Z forward, 0..1 depth (wgpu convention).
4. Mobile limits expressed as `QualitySettings::clamped_for(Platform::Mobile)`; backends further clamp via capabilities.
5. Shader sources are text keyed by `ShaderId` with revisions; no parsing in core.

## Steps for Account 2 (in order)
```bash
cargo fmt --all
cargo test -p fu-render3d
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
1. Fix any compile/test/clippy failures in `fu-render3d` first (expected: small API mismatches). Report results.
2. Add a wgpu backend as a separate crate (e.g. `fu-render3d-wgpu`) implementing `RenderBackend`; check desktop + Android
   limits; provide builtin `builtin/unlit` and `builtin/lit` WGSL (the registry ships empty).
3. Add GLB import behind a feature using the `gltf` crate, producing `MeshData`/`Material`/`TextureData`.
4. Then shadows, post-effect stack (reference: `darkenbutlights.frag`, `adjustColor.frag`), 3D stage format.

## Known risks
- Frustum plane extraction assumes 0..1 depth and column-major glam rows; covered by tests that have never run.
- `Store`/`Handle` use manual trait impls; reviewed by reading only.

