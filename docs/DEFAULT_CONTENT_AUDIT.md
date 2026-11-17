# Codename default content audit

What Codename Engine's own default content looks like on disk, what FNF-Universal
can load of it today, and what may be redistributed. Based on the files in the
reference repository `Codename-and-idk` (the `assets/` folder of a Codename
install). Statements marked **verified** were read from those files. Statements
marked **not verified** are recollection or inference and need checking.

## Licensing and redistribution

* The reference repository has **no licence file**, and its `README.md` is 42
  bytes. Nothing in it grants redistribution rights.
* `assets/data/specialThanks.txt` credits the people behind the base game's art,
  music and code. It is a credit list, not a licence.
* The default content is the base game's Weeks 1 to 7 plus Tutorial: songs,
  character art, stages, icons and UI. That art and music is the original game's,
  not Codename's. **Not verified:** to my recollection the original game's
  assets are licensed non-commercial and no-derivatives (CC BY-NC-ND 4.0) while
  its code is Apache-2.0, and Codename's own code is MIT. The upstream `LICENSE`
  files of both projects need to be read before any decision.
* The same repository also contains third-party material with its own owners:
  `VS Var Overhaul [Demo].exe` (69 MB), `mods/Vs Var Overhaul [Demo]`,
  `lime.ndll`, `libvlc.dll`, `libvlccore.dll`, and Python helper scripts.

**Decision for this project:** do not copy any Codename asset into the
FNF-Universal repository. The engine reads default content from a Codename
install the user already has (`--codename <assets dir>`), mounted read-only
through `fu-vfs`. Tests use small synthetic fixtures written in the test itself.
If you want to bundle assets later, that needs the upstream licences first.

## Layout (verified)

```
assets/
  data/        characters/*.xml (+ .hx)   stages/*.xml (+ .hx)   weeks/weeks/*.xml
               weeks/weeks.txt            notes/  splashes/  states/  scripts/
               titlescreen/  dialogue/  config/  editors/  alphabet.xml + alphabet/
  images/      characters/  stages/  icons/  game/  menus/ ... (PNG + XML or folders)
  songs/       <id>/{meta.json, events.json, charts/{easy,normal,hard}.json,
               song/{Inst.ogg, Voices.ogg}}
  music/ sounds/ fonts/ shaders/ videos/ languages/
```

Songs present: tutorial, bopeebo, fresh, dadbattle, spookeez, south, monster,
pico, philly nice, blammed, satin panties, high, milf, cocoa, eggnog,
winter horrorland, senpai, roses, thorns, guns, stress, ugh, test.

## How content is referenced (verified)

* **Weeks.** `data/weeks/weeks/week1.xml` is
  `<week name="DADDY DEAREST" chars="dad,bf,gf" sprite="week1">` with one
  `<song>` per song. `weeks.txt` only orders them and says it is optional.
* **Songs.** `meta.json` carries `displayName`, `bpm`, `icon`, `color`,
  `coopAllowed`, `opponentModeAllowed`. Charts are one JSON file per difficulty.
  Audio is `song/Inst.ogg` and `song/Voices.ogg`.
* **Characters.** `data/characters/<id>.xml` names a `sprite`, an `icon`, a
  `color`, optional `flipX`, `isPlayer`, `holdTime`, and `<anim>` children with
  `name`, `anim` (a frame-name prefix, with spaces and mixed case), `x`, `y`,
  `fps`, `loop` and `indices`.
* **Character art** is `images/characters/<sprite>.png` plus `.xml` (Sparrow),
  or a folder `images/characters/<sprite>/` holding `Animation.json`,
  `spritemap1.json` and `spritemap1.png` (Animate), or, for `spirit`, a `.txt`
  atlas (not XML).
* **Stages.** `data/stages/<id>.xml` names a `folder` under `images/` and lists
  `<sprite>` elements plus `<girlfriend/>`, `<dad/>`, `<boyfriend/>` anchors in
  draw order. `stage.xml` (Week 1) has no script; `limo`, `mall-evil`, `philly`,
  `school`, `school-evil`, `spooky` and `tank` each have a `.hx` script.
* **Icons** are `images/icons/<name>/icon.png`. **Notes, receptors and
  judgements** are `images/game/notes/default.{png,xml}` and
  `images/game/score/*.png`. **Countdown** is `images/game/{ready,set,go}.png`.

## How content is mounted

`fu_play::codename_content::mount_codename(assets, mod_root)` builds one `Vfs`:

| Mount id | Source | Mount point | Priority | Policy | Case |
| --- | --- | --- | --- | --- | --- |
| `codename-assets` | the user's `--codename <assets dir>` | `/` | 10 | read-only | insensitive |
| `codename-mod` | the selected mod directory | `/` | 20 | read-only | insensitive |

A virtual path such as `/data/characters/dad.xml` resolves in the mod first and
falls back to the installation. A directory that is missing or is not a
directory is reported in `CodenameContent::warnings` and that layer is left out;
nothing else on disk is consulted in its place. Writes are refused by the mount
policy, so the installation is never modified. `..`, absolute paths, drive
prefixes and backslashes in XML and metadata are refused before a lookup.

## What can be loaded today

| Need | Format | Status |
| --- | --- | --- |
| Note skin, receptors, sustains | Sparrow | Loaded through the VFS, except fractional `frameX` reads as 0 |
| Judgement images, countdown | PNG | Loaded through the VFS |
| Health icons | PNG | Loaded through the VFS in the base (`icons/<name>/icon.png`) and mod (`icons/<name>.png`) layouts; the sheet is drawn whole into a square (frame layout not inspected) |
| Dad, Bopeebo opponent, `bf`, `mom`, `pico`, `spooky`, `monster`, `tankman`, `senpai` | Sparrow + XML | Loaded through the VFS. `indices` honoured; `-loop` follow-up animations, `holdTime`, `isPlayer` and Animate `pivotX/pivotY` are not used |
| **`gf`, `gf-car`, `gf-christmas`, `gf-tankmen`** | Animate folder | **Not drawn.** `AnimateAtlas::parse` reads `Animation.json` only, uses placeholder UVs and a 512x512 sheet, and never reads `spritemap1.json` |
| `spirit` | `.txt` atlas | Not supported |
| Week 1 stage (`stage.xml`) | XML + PNG | Loaded through the VFS; `startCamPosX/Y` ignored |
| Stages with `.hx` scripts | scripts | Layers load; scripts are never run |
| Songs, charts, audio | JSON + OGG | Loaded by `load_codename_song` through `fu-vfs` |
| Weeks | `week*.xml` | Not parsed |
| Fonts, `alphabet.xml` | bitmap font XML | Not parsed; the HUD uses a built-in bitmap font |

The one remaining host-path lookup is `song_meta_path`, which returns a `PathBuf`
for callers that read the file themselves.

## Week 1 needs (the smallest playable slice)

Songs `bopeebo`, `fresh`, `dadbattle`; characters `dad`, `bf`, `gf`; stage
`stage`; the note skin, judgements and icons for `dad`, `bf`, `gf`.

Everything except **`gf`** is loadable. `gf` is an Animate atlas, so Week 1 cannot
show its spectator until the Animate renderer path exists.

## Character animation facts (verified from `dad.xml` and its atlas)

* `indices` entries are numbers or inclusive `a..b` ranges, and each selects the
  frame named `<prefix><number, 4 digits>`. The atlas has idle frames
  `0000`-`0012` and `Dad Sing note UP0000`-`0059`, which match the upper bounds
  of `2..12` and `56..59`.
* Prefix matching is case sensitive: `Dad Sing note UP`, `Dad Sing Note LEFT` and
  `dad sing note right` name three different sets. `singLEFT` plays
  `dad sing note right`, and `singRIGHT` plays `Dad Sing Note LEFT`.
* Frames omit `frameX`, `frameY`, `frameWidth` and `frameHeight` when untrimmed,
  and carry `pivotX`/`pivotY` on a character's first frame.
* Each pose has a `-loop` variant (`singUP-loop`, `idle-loop`) that plays its
  tail frames in a loop. Nothing switches to it yet.

## Next milestones

1. ~~Mount the Codename `assets` directory and the mod as `fu-vfs` layers and make
   `CodenameContent` read through it.~~ Done; see "How content is mounted".
2. Animate atlas rendering (`spritemap1.json` rectangles, symbol timelines) so
   `gf` shows.
3. `-loop` follow-up animations and `holdTime`.
4. Parse `weeks/*.xml` for the song list, shown through FNF-Universal's own menu.
5. Fractional `frameX`/`frameY`.
