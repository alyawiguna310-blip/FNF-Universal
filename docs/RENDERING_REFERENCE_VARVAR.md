# Rendering reference: Vs Var Overhaul [Demo]

Status of this document: **inspection report and verification checklist**. No renderer has been built yet (see `RENDERING_PLAN.md` for why and what comes next).

The community mod's art, music and charts were **not** copied into this repository. Only interface facts (file names, shader uniform names, parameter semantics) are recorded here. Do not commit the zip.

## What was inspected

- `Vs_Var_Overhaul__Demo_.zip` (305 files, about 113 MB). Only text sources were read: 12 shaders, 58 `.hx` scripts, stage/character/config XML and INI, event JSON, song `meta.json` and `events.json`. Two Adobe Animate atlas JSON files were read only for their first bytes. Audio and images were listed, not opened.
- **Not provided yet:** `VsVarOverhaul-Fanbuild-PC` (the Windows build).
- The FNF Universal repository (branch `ui/fnf-main-menu-phase1` and `main`).

## 1. The repository today (verified)

- Cargo workspace of 9 crates (`fu-core`, `fu-engine`, `fu-chart`, `fu-assets`, `fu-config`, `fu-storage`, `fu-benchmark`, `fu-bridge`, `fu-project`). `fu-engine` is an engine-manifest registry (about 3.8 KB). `rust-prototype` is a small note-state data model that says it is not wired into the engine.
- Searches for `wgpu`, `render`, `shader` and `gltf` in the repository's Rust sources returned nothing. GitHub flagged the search results as possibly incomplete, so confirm locally: `rg -n "wgpu|winit|render|shader|gltf" crates rust-prototype`.
- Not individually read: `fu-core`, `fu-chart`, `fu-assets`, `fu-config`, `fu-storage`, `fu-benchmark`, `fu-bridge`, `fu-project`.
- There is no render loop, 2D renderer, stage system or gameplay loop in the Rust code. The Flutter app is the launcher and editor UI. The task description assumed an existing Rust renderer, stage system and gameplay; that does not match what is in the repository.

## 2. Verified from the mod's source

| Fact | Evidence |
| --- | --- |
| The mod is a **Codename Engine** mod | `data/config/modpack.ini` (`API_VERSION=3`, `[StateRedirects]`), `data/stages/*.xml` with `<!DOCTYPE codename-engine-stage>`, `songs/<id>/meta.json` |
| Stages are 2D sprite stacks with per-layer scroll factors | `castle-neu.xml`, `red-castle.xml`: `<sprite ... scroll=".." zoomFactor=".."/>`, `<boyfriend>`, `<dad>`, `<girlfriend>` |
| Shaders are loaded by name with `new CustomShader("name")` and parameters are set as properties | `castle-neu.hx`, `red-castle.hx`, `snowing-shader.hx`, `Bloom Effect.hx` |
| Effects attach to a **sprite**, a **camera** or the **whole game** | `windows.shader = heatSun`; `camera.addShader(bloom)`; `FlxG.game.addShader(bloom)` |
| Effects are gated by a user option, and the mod disables its script when it is off | `if (!Options.gameplayShaders) { disableScript(); ... }` in `Bloom Effect.hx`; the same check in stage scripts. A song script, `songs/things.hx`, also sets `Options.gameplayShaders = false` |
| A photosensitivity option scales bloom to a quarter | `setBloom(params[1] * (FlxG.save.data.epilflashingepsy ? .25 : 1))` |
| Bloom semantics | `size = max(v - 1, 0) * 4.5`, `brightness = max(v, 1)`, `directions = 8`, `quality = 4` |
| Events tween shader values in beat-based time | `Conductor.crochet / 4 / 1000 * steps`, with FlxEase names (`linear back bounce circ cube elastic expo quad quart quint sine smoothStep smootherStep`, modes `In Out InOut`) |
| The HUD cameras have their own effect (vignette on `camHUD` or `camHUD2`) | `Screen Vignette.json` |
| "Shadows" in the stage are **2D deformable triangle-strip meshes** (`FlxStrip`, a 9x17 vertex grid), not 3D shadow maps | `castle-neu.hx` `addShadowMesh` / `drawShadowMesh` |
| Characters use two atlas formats: Sparrow XML (`Neu.png` + `Neu.xml`) and **Adobe Animate** (`Animation.json` + `spritemap1.json/png`) | file listing; Animate JSON header inspected |
| The 12 shaders are an OpenFL/Flixel GLSL dialect | every file starts with `#pragma header` and uses `openfl_TextureCoordv`, `openfl_TextureSize`, `flixel_texture2D`, `gl_FragColor` |

## 3. NOT found in the mod ZIP

A case-sensitive search for `Fox` and for `gltf`, `glb`, `obj`, `cubemap`, `armature`, `skybox` found nothing, and there are no 3D model files in the archive. The only matches for "mesh" are the 2D `FlxStrip` shadows above.

So none of these are visible in the community mod: Foxlite, FoxCamera, FoxGLTFLoader, FoxMTLLoader, FoxTextureCubemap, FoxInstanceData, FoxLineMesh, UniformPointLight, FoxAnimationPlayer, `basic.frag/vert`, `flixel_sprite.vert`, `minimal.*`, `sky_panorama.*`, `armature.glsl`, `lighting.glsl`, `material.glsl`, `mesh.glsl`, `shadow.glsl`, `sky.glsl`, `tonemapping.glsl`. If they exist, they are in the **Windows build**, which has not been provided. They must be treated as **unverified** until then.

## 4. Shader catalog (verified interface, inferred cost)

Uniform names and types are read from the `.frag` files. Cost is a rough estimate and is **not measured**.

| Shader | Uniforms | Cost (estimate) |
| --- | --- | --- |
| `adjustColor` | hue, saturation, brightness, contrast (float) | cheap |
| `bloom` | brightness (float), directions (float), quality (int), size (float) | moderate |
| `coloredVignette` | color (vec3), amount, strength (float), transperency (bool) | cheap |
| `darkenbutlights` | strength, threshold (float), tint (vec4), gradientY, gradientHeight (float) | cheap |
| `glitching` | iTime, AMT, SPEED (float) | cheap |
| `heatwave` | time, strength, speed (float), even (bool) | cheap |
| `lineboil` | time, strength, fps (float), scale (vec2), noise (sampler2D) | cheap |
| `saturation` | sat, contrast (float) | cheap |
| `snow` | time, cameraZoom, BRIGHT, DEPTH, WIDTH, SPEED (float); res, cameraPosition (vec2); STARTING_LAYERS, LAYERS (int); flipY, snowMelts, pixely (bool); snowMeltRect (vec4) | expensive |
| `static` | time, strength (float) | cheap |
| `tapenoise` | time, strength (float), res (vec2) | cheap |
| `warp` | distortion (float) | cheap |

The catalog is implemented in `crates/fu-graphics/src/effects.rs` with tests that check these counts.

Effects named in the task but **not** in this ZIP: shadow shaders, sky/environment shaders, tone mapping. They would come from the Foxlite shader modules (unverified, see section 3).

## 5. Porting notes (inferred)

- The shaders cannot run unchanged. They depend on Flixel/OpenFL built-ins and a GLSL 1.00 style. Plan to hand-port to WGSL (or GLSL that naga accepts) and keep the uniform names above so mod scripts keep working. Whether naga's GLSL front end would accept them as-is has not been tested.
- `uniform sampler2D noise` in `lineboil` needs a texture binding supplied by the mod.
- Effect parameters in events are tweened on the beat clock, so the renderer must expose parameter setters, not bake values.

## 6. Verification checklist

Legend: **V** verified from source. **I** inferred from names or structure. **B** needs the Windows build. **R** needs a renderer and a GPU to test.

- [x] V Mod is Codename Engine, API version 3.
- [x] V 12 shaders, their uniform names and types.
- [x] V Effects attach to sprites, cameras and the whole game; gated by a user option.
- [x] V Photosensitivity scaling of bloom (0.25), now carried as `reduce_flashing` in `fu-graphics`.
- [x] V Stage shadows are 2D strip meshes.
- [x] V Two character atlas formats (Sparrow, Adobe Animate).
- [ ] I Cost estimates per effect (replace with measurements).
- [ ] I Hand-port to WGSL is required (naga GLSL acceptance untested).
- [ ] B Foxlite classes and shader modules exist and what they do.
- [ ] B Any glTF/OBJ/cubemap/skeletal assets and how they are loaded.
- [ ] B 3D camera, lighting, shadow-map and tone-mapping behavior.
- [ ] R Any claim about frame time, GPU time, memory or compatibility.
- [ ] R Low/High/Custom behavior on real desktop and mobile GPUs.
- [ ] R Shader failure fallback.
