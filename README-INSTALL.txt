# FNF Universal Editor v0.2 — persistent project save/load patch

This is an overlay patch for the existing FNF Universal project. It does not include or replace the Rust workspace, platform folders, or other user files.

## Apply
1. Back up your current `apps/launcher/lib/main.dart` and `apps/launcher/test/widget_test.dart`.
2. Extract this ZIP.
3. Copy the included `apps` folder into `D:\fnf-universal-v0.1-foundation`, allowing replacement of those two files.
4. Open PowerShell in `D:\fnf-universal-v0.1-foundation\apps\launcher`.
5. Add the one new dependency:

```powershell
flutter pub add path_provider
```

6. Run:

```powershell
flutter analyze
flutter test
flutter run
```

## New behavior
- Save button writes the current project to the app documents directory:
  `FNFUniversal/Projects/<project-name>.fnfup.json`
- Folder button lists saved `.fnfup.json` projects and loads the selected project.
- Loader validates the project format/version and bounds-checks chart lane/step values.
- Existing JSON preview/copy remains available.

## Platform notes
`path_provider` resolves the app documents directory separately on Android, Windows and Linux. This first persistence pass uses an in-app project list rather than an arbitrary file picker, so it avoids broad filesystem access.

## Limitations
- This patch has not been run through Flutter in the user's local environment. Run the commands above and share any output.
- No migration beyond `formatVersion: 1` yet.
- No asset import, undo/redo, audio playback, stage drag canvas, or engine-specific exports yet.
- The `.fnfup.json` format is FNF Universal's draft project format, not an official format for any FNF engine.
