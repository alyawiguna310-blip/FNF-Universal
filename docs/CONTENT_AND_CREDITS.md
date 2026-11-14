# Content policy, Story/Freeplay and credits

## What FNF Universal does and does not ship

FNF Universal ships **no** Friday Night Funkin' music, art, charts or other game assets. Per public sources, the original game's source code is open (Apache 2.0) but its assets are under a separate, closed license, and adding a credit does not change that. It also matches this project's rule of original branding and assets.

Instead, Story and Freeplay read the weeks and songs from a folder **the player points at**: their own copy of the game, or a mod folder they have the right to use.

## What the scanner reads

Layout follows the Funkin' Modding Docs (CC BY-SA 4.0):

```
data/levels/<level>.json                 name, songs[], visible
data/songs/<id>/<id>-metadata.json       songName, artist, playData.difficulties[], timeChanges[0].bpm
```

- Only those small JSON fields are read: names, artist, BPM, difficulties.
- Never read, copied or run: audio, images, charts, scripts (`.hxc`, `.lua`), executables.
- Safety caps: 1 MB per JSON file, 20 000 folder entries, 7 levels deep, symlinks not followed, ids limited to letters, digits, `-` and `_`. Images/sounds/music folders are skipped.
- Hidden levels (`visible: false`) are left out of Story but their songs still show in Freeplay. Songs that are in no level appear last in Freeplay.
- This is **not yet verified against real builds** (see TESTING.md).

## Status

| Entry | State |
| --- | --- |
| Story, Freeplay | Browse only. Playing needs the engine adapter launch runner (not built). |
| Credits | Ready. Credits The Funkin' Crew and the Modding Docs, and states that no game content is bundled. |

## Where the folder is remembered

`Documents/FNFUniversal/content_source.json` (just the path). On Android Documents is app-private and there is no folder picker yet, so this is Windows-first.
