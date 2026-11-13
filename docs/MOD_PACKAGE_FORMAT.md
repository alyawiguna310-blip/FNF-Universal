# Mod package format (v1)

One zip that carries a project, every script and the menu/UI changes, so a mod can be copied between installs. Produced by Developer Mode, PACKAGE tab.

## Layout

```
fnfmod.json            manifest: format, kind (mod | launcher-profile), name, createdUtc
launcher_ui.json       menu/UI definition (title, accent, background, entries)
project/<name>.fnfup.json   editor project, copied as-is        (kind = mod)
scripts/<path>              script files, copied as plain files (kind = mod)
```

- **Mod** carries all four. **Launcher profile** carries only `fnfmod.json` and `launcher_ui.json`.
- Scripts are not interpreted, converted or run. Converting to another mod loader is a separate later step with its own report.

## Menu/UI data

`launcher_ui.json` is plain data. Entries can only reference built-in destinations (`story`, `freeplay`, `mods`, `settings`, `credits`). A package can restyle text, order, accent, background and animation, but it cannot:

- hide or remove **Settings** (it is restored with a warning),
- change an entry's status or "what it needs" text,
- add new commands or scripts to the menu,
- override reduced-motion for players.

Bad values are replaced with defaults and listed as warnings.

## Import safety

- Inspect is read-only. Nothing is extracted, executed or converted.
- Import writes a **new** folder `Imports/<name>-<timestamp>/`. It never overwrites.
- Paths with `..`, absolute paths, drive letters, reserved Windows names or odd characters are skipped (zip-slip protection).
- Executables and native libraries (`.exe .dll .bat .cmd .ps1 .sh .apk .so .jar ...`) are never exported or imported.
- Limits: 2000 entries, 32 MB per entry, 256 MB total. Zip64 and encrypted zips are refused. Sizes and CRC are verified.
- "Apply menu/UI" is an explicit button. It changes the live menu and is only saved if you press Save.

## Folders (under Documents/FNFUniversal)

| Folder | Use |
| --- | --- |
| `Projects/` | Studio saves (`*.fnfup.json`) |
| `Scripts/<project name>/` | put a mod's script files here before exporting |
| `Exports/` | zips written by EXPORT |
| `Imports/` | drop zips here; imported copies land here too |
| `launcher_ui.json` | your saved menu/UI |

## Known gaps

- Android: Documents is app-private, so there is no way to move zips in or out yet. A file picker / share sheet (Storage Access Framework) is needed.
- No conversion to other loaders (V-Slice, Psych, Kade, Codename) yet. Exports are engine-independent.
- Launcher profile is a rebrand of the same app. Standalone APK/EXE builds need a build pipeline.
- The Studio project is exported from its saved file, so press Save in Studio first.
