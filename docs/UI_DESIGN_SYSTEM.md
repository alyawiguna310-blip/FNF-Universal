# UI design system (phase 1)

Status: main menu, design tokens and Developer Mode gate are in. Editors are not redesigned yet.

## Where things live

| File | Purpose |
| --- | --- |
| `apps/launcher/lib/ui/fnf_tokens.dart` | Colors, spacing, motion, type styles, `buildFnfTheme()` |
| `apps/launcher/lib/ui/main_menu.dart` | Main menu = launcher (keyboard, controller, touch) |
| `apps/launcher/lib/ui/menu_entry.dart` | Menu entries and their honest status |
| `apps/launcher/lib/ui/feature_status_screen.dart` | What an unbuilt entry still needs |
| `apps/launcher/lib/ui/developer_mode.dart` | Session-only Developer Mode switch |
| `apps/launcher/lib/ui/fnf_app.dart` | App shell; hosts the existing `EditorShell` under Developer Mode |
| `apps/launcher/lib/menu_main.dart` | Phase 1 entry point |

`lib/main.dart` and its tests are untouched.

## Run it

```
cd apps/launcher
flutter run -t lib/menu_main.dart
```

## Input map

| Action | Keyboard | Controller | Touch |
| --- | --- | --- | --- |
| Move | Up / Down | D-pad | Tap entry, or on-screen up/down |
| Confirm | Enter | A | Tap entry, or on-screen confirm |
| Back | Escape | B | Back button on sub-screens |

The on-screen pad shows on Android and iOS by default (`showTouchControls` overrides).

## Visual rules

- Dark base, one accent (`FnfColors.accent`), hard edges, no card rounding.
- Every element earns its place: the status tag says whether an entry works, the right panel explains the selection, the footer shows the controls.
- Entries that are not built say so on a status screen. No silent buttons.

## Known gaps

- Developer Mode unlock is a temporary hidden gesture (7 quick taps on the version label). The spec puts it inside Controller Editor, which does not exist yet. Call `DeveloperModeController.unlock()` from there when it is built, then delete the gesture.
- Developer Mode is not persisted between launches.
- No bundled display font yet. Add an open-licensed one, list it in `LICENSES.md`, then set it in `FnfType`.
- Only the existing Chart, Character and Stage editors are reachable. Script Editor, Mod Creator, Mod Porting, Enemy/Boss, Cutscene, Mod Settings, Validator, SDK docs and the 3D Stage Editor are not built.
- F7, F8 and F9 are not wired yet.
- `kAppVersionLabel` is hardcoded; keep it in sync with `pubspec.yaml`.
