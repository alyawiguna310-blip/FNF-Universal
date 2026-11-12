# Platform Capabilities

The SDK should expose capabilities rather than pretending every platform can do everything.

| Capability | Windows/Linux desktop | Android |
|---|---|---|
| Song/chart scripting | Yes, sandboxed | Yes, sandboxed |
| Note visual scripting | Yes | Yes |
| Camera/UI effects | Supported subset | Supported subset |
| Window move/resize | Optional desktop capability | Not available as a normal app capability |
| External mod folder | Normal filesystem permissions | SAF-selected folder / persisted URI permission |
| Keyboard input | Yes | Optional connected keyboard |
| Tile Controller | Not applicable as a default desktop input | Yes |
| V-Slice Style controller | Platform implementation dependent | Yes, once its behavior is specified and tested |

Scripts must check capabilities before using optional APIs. Missing capabilities should produce a clear fallback or a validation warning, not a crash.

Android storage should use the Storage Access Framework. A suggested user-visible folder is `Documents/FNF Universal/Mods`, but access must be granted by the user through the system picker. Do not rely on unrestricted access to `Android/data`.
