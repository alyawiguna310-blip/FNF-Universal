# In-Game Developer Tools and 3D Stage Foundation

## Player-facing controls

- **F7:** toggle Editor Mode while in a supported gameplay/editor session.
- **F8:** show/hide the read-only diagnostic log console.
- **F9:** toggle the diagnostic overlay (FPS, frame time, renderer and memory counters).
- Function-key bindings must be remappable and must not be the only way to access tools on Android.
- Mobile builds expose equivalent touch controls in the developer overlay/menu.

F8 is deliberately not a terminal. It has no command input, command parser, script execution, or gameplay mutation. It displays engine, asset, mod, audio, renderer and script log entries with level filters, auto-scroll, clear and copy actions.

## Editor Mode

Editor Mode is an in-game editing surface, not a separate game process. It may pause or use a safe preview session while editing. Chart edits are staged in an edit transaction and only become project changes after an explicit save/apply action. Runtime gameplay judgment/input state remains separate from visual editor selection.

## 3D stage content

The project model supports 2D, 3D and hybrid stages. Hybrid stages can combine 2D sprites/characters with 3D backgrounds, props, lights and cameras.

Initial model import targets:
- glTF 2.0 JSON (.gltf)
- GLB binary (.glb)

The Rust project model in `crates/fu-project/src/stage3d.rs` defines serializable scene metadata and structural validation. It does **not** parse model files or render scenes yet.

## Runtime/rendering plan

- Keep stage data renderer-agnostic; the same project should be usable on Android, Windows and Linux.
- Use capability detection rather than device-name allowlists.
- Offer Auto, Low, Balanced, High and Ultra quality tiers.
- Clamp texture dimensions, shadow quality, post-processing, light count and render scale according to actual GPU/device capabilities.
- Provide graceful fallbacks: unsupported effects are disabled with a visible compatibility warning, not a crash.
- Keep 2D gameplay timing, note judgment and input independent from 3D visual transforms.
- Use asset budgets and asynchronous loading; avoid blocking the audio/gameplay thread.
- 3D renderer integration, glTF decoding, animation playback, editor viewport and device profiling remain future implementation work.

## Safety and compatibility

Imported models are data, not executable code. Validate file sizes, references, dimensions and supported glTF extensions before loading. Do not execute scripts during import. Scripted stage events must use the existing sandbox/capability model.
