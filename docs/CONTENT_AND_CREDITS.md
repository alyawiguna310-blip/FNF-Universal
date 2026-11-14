# Content policy, Story/Freeplay and credits

## What FNF Universal does and does not ship

FNF Universal ships **no** Friday Night Funkin' music, art, charts or other game assets, and no mods. Per public sources, the original game's source code is open (Apache 2.0) but its assets are under a separate, closed license, and adding a credit does not change that. It also matches this project's rule of original branding and assets.

Instead, Story and Freeplay read the weeks and songs from a folder **the player points at**: their own copy of the game, or mod folders they have the right to use. Point it at one mod, a `mods` folder holding many, or the game folder.

## What the scanner reads

Only small text fields: names, artist, BPM and difficulty names. Never read, copied or run: audio, images, chart contents, scripts (`.hx`, `.hxc`, `.lua`), executables.

### V-Slice (Funkin' Modding Docs, CC BY-SA 4.0)

```
data/levels/<level>.json                 name, songs[], visible
data/songs/<id>/<id>-metadata.json       songName, artist, playData.difficulties[], timeChanges[0].bpm
```

### Codename Engine (Codename Engine wiki)

```
data/weeks/weeks/<week>.xml              <week name=".."> with <song>id</song> nodes
data/weeks/weeks.txt                     optional order of the weeks (file names)
songs/<id>/meta.json                     displayName / name, bpm, difficulties[]
songs/<id>/charts/<difficulty>.json      only the file names are used, when meta.json lists no difficulties
```

- A folder counts as a Codename mod if it has `data/weeks` or a `songs/<id>/meta.json`. Song folders with neither a `meta.json` nor charts are ignored.
- Songs in no week still appear in Freeplay (like in Codename). `data/config/freeplaySonglist.txt` is not read yet.
- Week XML is matched with a few small patterns and never parsed as full XML. Comments are removed, only `&lt; &gt; &quot; &apos; &amp;` are decoded, and DOCTYPE/external entities are never expanded.
- Mods must be **unzipped folders**. `.zip` addons are not opened yet.

### Not supported yet

Psych Engine (`weeks/*.json`), Kade and other layouts.

## Safety caps

1 MB per file, 20 000 folder entries, 7 levels deep, 2000 song folders per mod, symlinks not followed, ids limited to letters, digits, space, `-`, `_` and `.` (no slashes, colons or `..`). Images/sounds/music/videos/fonts/shaders folders are skipped. Level files from different mods with the same name are kept apart, and each entry shows which mod it came from (FROM).

Not yet verified against real builds (see TESTING.md).

## Status

| Entry | State |
| --- | --- |
| Story, Freeplay | Browse only. Playing needs the engine adapter launch runner (not built). |
| Credits | Ready. Credits The Funkin' Crew, the CodenameCrew, the Modding Docs and the Codename wiki, and states that no game content is bundled. |

## Where the folder is remembered

`Documents/FNFUniversal/content_source.json` (just the path). On Android Documents is app-private and there is no folder picker yet, so this is Windows-first.
