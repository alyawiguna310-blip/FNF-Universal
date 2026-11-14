# Rendering plan and blockers

See `RENDERING_REFERENCE_VARVAR.md` for what was inspected and what is verified.

## Why this is not a "modify the existing renderer" task

The repository has no renderer, render loop, 2D scene or gameplay loop in Rust (checked: `fu-engine`, `rust-prototype`, plus searches; the other crates should be double-checked locally with `rg`). A 3D renderer cannot be "integrated with the existing 2D renderer and stage system" because those do not exist yet. Building the 3D renderer means building the engine's renderer as well.

## Exact blockers right now

1. **No renderer or gameplay core exists.** The rendering work needs a real foundation first: window and surface, sprite batcher, camera, stage loader, audio clock and note timing.
2. **I cannot compile or run Rust or a GPU in my environment.** The sandbox has no Rust toolchain and no network. Everything here is written but unbuilt. `cargo test -p fu-graphics` on your machine is the first real check.
3. **The Foxlite evidence is not in the community ZIP** (see reference doc section 3). It needs the Windows build.
4. **Architecture decision needed:** a wgpu renderer is a separate native window. Showing it inside the Flutter UI needs per-platform texture sharing. Recommended first step: a separate `fu-play` binary (winit + wgpu) that the launcher starts, desktop first. Mobile windowing and surface handling come after.
5. **Unknowns to confirm on your machine:** `rustc --version`, and the wgpu and winit versions to pin (I will not guess APIs).

## What was built now (real, testable, no GPU)

Crate `crates/fu-graphics`:

- Low / High / Custom presets, fully configurable Custom (3D, shadow quality, bloom, anti-aliasing, texture filtering, post-processing, render scale, lighting quality, shader effects, frame-rate limit).
- Per-device ceilings (`DeviceCaps`): the device always wins over the request; mobile is held to a lower ceiling; High is never chosen automatically for mobile.
- Persistence as JSON with versioning. Unreadable or newer files fall back to the device default with a warning.
- `reduce_flashing` accessibility option (mirrors the mod's own 0.25 bloom scale).
- Catalog of the 12 verified shaders (names, uniforms) and a gate `allowed(kind, settings)`: Low keeps cheap effects and drops bloom and snow.
- Unit tests for all of the above.

Run: `cargo test -p fu-graphics`.

## Order of work after that

1. Confirm the toolchain and pin wgpu/winit. Create `fu-render` (device, surface, depth buffer) with a clear-color smoke test on your GPU.
2. 2D foundation: sprite batcher, Sparrow XML atlas, orthographic camera, Codename stage XML loader (stage XML format is verified in the ZIP).
3. Post-processing chain with 2 or 3 ported shaders (saturation, vignette, bloom) and failure fallback.
4. 3D: glTF/GLB loader, depth buffer, mesh pipeline, perspective camera, one directional light.
5. Composition of 2D characters and notes over 3D, with timing kept off the render thread.
6. Shadows, cubemap sky, instancing, skeletal animation, frustum culling.
7. Adobe Animate atlas support (needed by the reference mod's characters).
8. Mod adapters (events to shader parameters, 3D stage registration).
9. Measurements on real desktop and mobile hardware.

Each step needs a successful local build and a measurement before the next one starts.
