# Mod package format (v1)

One zip that carries a project, every script and the menu/UI changes (wallpaper included), so a mod can be copied between installs. Produced by Developer Mode, PACKAGE tab.

## Layout

```
fnfmod.json                   manifest: format, kind (mod | launcher-profile), name, createdUtc
launcher_ui.json              menu/UI definition (title, accent, background, entries)
ui/backgrounds/<image>        wallpaper named by launcher_ui.json (png, jpg or webp)
project/<name>.fnfup.json     editor project, copied as-is        (kind = mod)
scripts/<path>                script files, copied as plain files (kind = mod)
```

- **Mod** carries everything. **Launcher profile** carries only `fnfmod.json`, `launcher_ui.json` and the wallpaper.
- Scripts are not interpreted, converted or run. Converting to another mod loader is a separate later step with its own report.

## Menu/UI data

`launcher_ui.json` is plain data. Entries can only reference built-in destinations (`story`, `freeplay`, `mods`, `settings`, `credits`). A package can restyle text, order, accent, background and animation, but it cannot:

- hide or remove **Settings** (it is restored with a warning),
- change an entry's status or "what it needs" text,
- add new commands or scripts to the menu,
- override reduced-motion for players.

Background styles: `stripes` (animated, original), `image` (wallpaper) and `plain`. For `image`, `backgroundImage` is a plain file name and `backgroundDim` (0 to 0.85) darkens it so text stays readable. If the image is missing or invalid the menu falls back to stripes.

Bad values are replaced with defaults and listed as warnings.

## Wallpaper rules

- Plain file name only (no folders), ending in `.png`, `.jpg`, `.jpeg` or `.webp`.
- The file header is checked, so a renamed non-image is refused. Max 8 MB.
- Add images by pasting a full path in the MENU / UI editor. They are copied into `Documents/FNFUniversal/Backgrounds/`, never moved.
- Only use images you have the right to use and share. FNF Universal ships no wallpaper art.
- Applying a package copies its wallpaper into `Backgrounds/` under a new name if that name is taken. Nothing is overwritten.

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
| `Backgrounds/` | wallpaper images for the menu |
| `Exports/` | zips written by EXPORT |
| `Imports/` | drop zips here; imported copies land here too |
| `launcher_ui.json` | your saved menu/UI |

## Known gaps

- Android: Documents is app-private, so there is no way to move zips or images in or out yet. A file picker / share sheet (Storage Access Framework) is needed.
- No conversion to other loaders (V-Slice, Psych, Kade, Codename) yet. Exports are engine-independent.
- Launcher profile is a rebrand of the same app. Standalone APK/EXE builds need a build pipeline.
- The Studio project is exported from its saved file, so press Save in Studio first.
