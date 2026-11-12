# Editor ↔ Engine Contract

## Shared project model
The editor must not maintain a private format that the engine cannot load. Every editor action writes to the same project model consumed by the validator and runtime.

A project contains metadata and target compatibility; assets and characters/stages; songs and chart data; note visual definitions and movement behaviors; gameplay mechanics and enemy definitions; creator-exposed mod settings; scripts and declared capabilities; and player-independent defaults.

The editor should offer Beginner/Visual Mode and Advanced/Script Mode. Both modes edit the same underlying data. Switching modes must not discard information that the other mode can represent.

## Suggested editor modules
| Editor module | Primary data |
|---|---|
| Project Hub | project metadata, engine compatibility |
| Chart Editor | lanes, notes, sustains, events |
| Note Editor | sprites, receptors, animation, visual paths |
| Character Editor | character metadata, animations |
| Stage Editor | stage objects, layers, camera |
| Enemy/Boss Editor | phases, attacks, health rules |
| Cutscene Editor | timeline, video, dialogue, audio |
| Script Editor | scripts, API references, permissions |
| Mod Settings Editor | creator-defined options and defaults |
| Preview/Debugger | live preview, logs, validation |

## Note system boundaries
**Input Manager**
- Reads keyboard and the selected mobile controller mode.
- Owns Tile Controller and V-Slice Style selection.
- Owns player controller layout, opacity, size and placement.
- Mods cannot silently change the selected controller mode.

**Note Renderer**
- Owns note textures, colors, size, rotation, animation, particles and visual position.
- Can evaluate a mod-defined visual path each frame.
- Does not decide whether a note was hit.

**Gameplay/Judgment**
- Owns note lane identity, timing window, sustain duration, hit/miss judgment and score.
- Uses a documented gameplay hitbox policy.
- Does not infer timing from a sprite's screen position.

The default is visual-only movement: moving a sprite does not move its judgment timing or hitbox. A mod may request a supported gameplay-path mechanic explicitly, but that must be declared, validated and previewed.

## Creator settings vs player preferences
Creator-defined settings are registered by a mod and shown in that mod's settings page. Examples: enemy attacks enabled/disabled, enemy health drain, mod-specific assist option, note visual intensity, creator-defined difficulty preset.

Player preferences are engine-owned: chosen controller mode, controller placement and opacity, accessibility options, and global visual comfort options.

A mod may expose an option that changes its own mechanics, but cannot secretly overwrite protected player preferences. The engine should display the effective setting and reject invalid overrides.

## Preview behavior
The editor preview should use the same Rust gameplay and rendering logic as runtime wherever possible. If preview uses a simplified implementation, it must label differences and never claim exact runtime behavior.

Every script-driven feature should support schema validation, capability validation, preview with deterministic seed, clear runtime errors, and reset to safe defaults.
