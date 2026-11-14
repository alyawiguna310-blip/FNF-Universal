# fu-render3d — 3D rendering foundation (Task 1)

Status: **source written, not compiled or tested** (see `docs/handoff/ACCOUNT1_HANDOFF.md`).

## Why a standalone, backend-agnostic crate
The audit found no renderer in this repository: the workspace has no graphics dependency,
`ARCHITECTURE.md` says each engine runtime owns rendering, and `rust-prototype` is a note
data model only. So `fu-render3d` does not pretend to extend a 2D renderer. It owns CPU-side
3D state and produces a `FramePacket` for a `RenderBackend` implementation.

## Modules
| Module | Responsibility |
|---|---|
| `resources` | Generational `Handle<T>` + `Store<T>` arena (stale handles resolve to `None`) |
| `transform`, `camera` | TRS transform; perspective/orthographic camera, right-handed, 0..1 depth |
| `mesh` | Validated `MeshData` (bounds cached), `Aabb`, cuboid/plane builders; target type for a future GLB importer |
| `material`, `shader` | `Material`, `TextureData` (CPU RGBA8), `ShaderId`, named `ShaderParams`, `ShaderRegistry` with revisions |
| `light` | Directional/point/spot, `LightEnvironment::select` capped by quality |
| `scene` | Node hierarchy with cycle/dangling-parent detection, layer masks |
| `queue` | Frustum culling, opaque grouping by pipeline key + front-to-back, blended back-to-front |
| `quality` | `QualitySettings` presets (Low/Medium/High x Desktop/Mobile), validation, serde (camelCase) |
| `backend` | `RenderBackend` trait, `BackendCapabilities::clamp_quality`, `Composition` contract |
| `renderer` | `Renderer3D` ties it together and calls the backend |

## Integration seams
- **Engine runtime**: implement `RenderBackend`, choose a `Composition`, declare capability `render3d`
  (`fu_render3d::CAPABILITY_ID`) in its engine manifest. `docs/PLATFORM_CAPABILITIES.md` says scripts must
  check capabilities before using optional APIs.
- **GLB Stage Editor**: an importer should emit `MeshData`, `Material`, `TextureData`; the editor shares the
  same crate for preview, which matches `docs/EDITOR_ENGINE_CONTRACT.md` ("same Rust rendering logic").
- **Project model**: `fu-project::StageObject` is 2D (`x`, `y`) today. Extending it for 3D is deliberately
  not done here (format change; belongs to a later task).

## Reference mapping (Vs Var Overhaul, Codename Engine; reference only)
| Observed in the mod | Foundation hook |
|---|---|
| `lowquality` save flag disables shadows | `QualitySettings` (shadow_map_size = None) |
| `CustomShader` uniforms set by name (darkenbutlights, adjustColor) | `ShaderParams` name/value map |
| Per-camera shader stacks (camGame, charCam) | later post-effect stack; `post_effects` flag reserved |
| 17x9 `FlxStrip` mesh for floor shadows, render-texture sources | `MeshData` + future render targets (not implemented) |

Not implemented here: GPU backend, WGSL shaders, GLB loading, shadows, post effects.
