//! Synthetic Codename-style mod fixtures.
//!
//! Everything here is generated at test time in a temporary directory. No
//! reference mod files, executables or scripts are ever used as inputs.

#![allow(dead_code)]

use fu_import::test_support::TempDir;

/// Directory name used for the fixture mod.
pub const MOD_NAME: &str = "demo";

/// Writes `mods/autoload.txt` plus a complete, internally consistent mod.
pub fn write_mods_directory(temp: &TempDir) {
    temp.write("mods/autoload.txt", &format!("{MOD_NAME}\n"))
        .unwrap();
    write_mod(temp, &format!("mods/{MOD_NAME}"));
}

/// Chart used by the fixture song.
pub fn chart_json() -> &'static str {
    r#"{
  "chartVersion": "1.6.0",
  "codenameChart": true,
  "scrollSpeed": 2.2,
  "stage": "backstage",
  "noteTypes": ["No Animation"],
  "strumLines": [
    {
      "keyCount": 4,
      "position": "dad",
      "characters": ["opponent"],
      "type": 0,
      "visible": true,
      "notes": [
        {"time": 0, "id": 0, "sLen": 0, "type": 0},
        {"time": 200, "id": 1, "sLen": 100, "type": 0}
      ]
    },
    {
      "keyCount": 4,
      "position": "boyfriend",
      "characters": ["bf"],
      "type": 1,
      "visible": true,
      "strumScale": 1,
      "strumSpacing": 1,
      "strumPos": [0, 50],
      "strumLinePos": 0.75,
      "vocalsSuffix": "",
      "notes": [
        {"time": 400, "id": 2, "sLen": 0, "type": 0, "customNoteFlag": "kept"}
      ]
    }
  ],
  "events": [
    {"time": 0, "name": "Camera Bump", "params": [true, 0, 100, "quart", "Out"]}
  ],
  "modchartExtra": {"kept": true}
}"#
}

/// Song metadata used by the fixture song.
pub fn meta_json() -> &'static str {
    r##"{
  "bpm": 120,
  "needsVoices": true,
  "difficulties": ["hard"],
  "displayName": "Bop",
  "icon": "bf",
  "color": "#31B0D1",
  "stepsPerBeat": 4,
  "beatsPerMeasure": 4,
  "opponentModeAllowed": true,
  "coopAllowed": true,
  "customValues": {"difficultyRating": "easy"},
  "unknownMeta": [1, 2, 3]
}"##
}

/// Writes a complete mod at `root`.
pub fn write_mod(temp: &TempDir, root: &str) {
    let at = |relative: &str| format!("{root}/{relative}");

    temp.write(
        &at("data/config/modpack.ini"),
        "[Common] # prefix note\n\
API_VERSION=3\n\
DOWNLOAD_LINK=\"https://example.invalid/mod\"\n\
\n\
[Flags] # no prefix\n\
DISABLE_WARNING_SCREEN=true\n\
VERSION_MESSAGE=\"Demo\"\n\
\n\
[Discord]\n\
CLIENT_ID=\"123\"\n\
\n\
[StateRedirects]\n\
TitleState=\"TitleState\"\n\
\n\
[CustomSection]\n\
keep=this\n",
    )
    .unwrap();
    temp.write(
        &at("data/config/flags.ini"),
        "[Flags]\nDEFAULT_MENU_MUSIC=menu song\n",
    )
    .unwrap();

    temp.write(
        &at("data/characters/bf.xml"),
        "<!DOCTYPE codename-engine-character>\n\
<character x=\"-100\" y=\"-120\" sprite=\"BF\" icon=\"bf\" color=\"#31B0D1\" camx=\"100\" camy=\"100\" mystery=\"kept\">\n\
\t<anim name=\"idle\" anim=\"BF Idle\" x=\"0\" y=\"0\"/>\n\
\t<anim name=\"singLEFT\" anim=\"BF Left\" x=\"-10\" y=\"0\" label=\"true\"/>\n\
\t<anim name=\"singDOWN\" anim=\"BF Down\" x=\"0\" y=\"10\"/>\n\
\t<anim name=\"singUP\" anim=\"BF Up\" x=\"0\" y=\"-10\"/>\n\
\t<anim name=\"singRIGHT\" anim=\"BF Right\" x=\"10\" y=\"0\"/>\n\
</character>\n",
    )
    .unwrap();
    temp.write(
        &at("data/characters/gf.xml"),
        "<!DOCTYPE codename-engine-character>\n\
<character x=\"0\" y=\"0\" sprite=\"GF\" icon=\"gf\"><anim name=\"idle\" anim=\"GF Idle\"/></character>\n",
    )
    .unwrap();
    temp.write(
        &at("data/characters/opponent.xml"),
        "<!DOCTYPE codename-engine-character>\n\
<character x=\"0\" y=\"0\" sprite=\"Opponent\" icon=\"opponent\" flipX=\"true\"><anim name=\"idle\" anim=\"Opponent Idle\"/></character>\n",
    )
    .unwrap();

    temp.write(
        &at("data/stages/backstage.xml"),
        "<!DOCTYPE codename-engine-stage>\n\
<stage name=\"backstage\" folder=\"stages/backstage/\" zoom=\"0.8\" opponentZoom=\"0.7\">\n\
\t<sprite name=\"floor\" sprite=\"floor\" x=\"0\" y=\"0\" scroll=\"1\"/>\n\
\t<sprite name=\"back\" sprite=\"back\" x=\"0\" y=\"0\" scroll=\"0.5\" zoomFactor=\"1.1\"/>\n\
\t<girlfriend x=\"100\" y=\"100\" scrollx=\"1.05\"/>\n\
\t<dad x=\"200\" y=\"200\"/>\n\
\t<boyfriend x=\"300\" y=\"300\" scale=\"0.9\"/>\n\
</stage>\n",
    )
    .unwrap();

    temp.write(
        &at("data/events/Camera Bump.json"),
        r#"{"params": [{"name": "Amount", "type": "Float(0.001, 10, 0.01, 2)", "defaultValue": "0.05"}]}"#,
    )
    .unwrap();
    temp.write(&at("data/events/Camera Bump.hx"), "// camera bump\n")
        .unwrap();
    temp.write(&at("data/scripts/util.hx"), "class Util {}\n")
        .unwrap();
    temp.write(&at("data/states/TitleState.hx"), "class TitleState {}\n")
        .unwrap();

    temp.write_bytes(&at("images/characters/BF.png"), b"\x89PNG\r\n\x1a\n")
        .unwrap();
    temp.write(
        &at("images/characters/BF.xml"),
        "<TextureAtlas imagePath=\"BF.png\"/>\n",
    )
    .unwrap();
    temp.write_bytes(&at("images/characters/GF.png"), b"\x89PNG\r\n\x1a\n")
        .unwrap();
    temp.write(
        &at("images/characters/Opponent/Animation.json"),
        r#"{"ANIMATION": {}}"#,
    )
    .unwrap();
    temp.write_bytes(
        &at("images/characters/Opponent/spritemap1.png"),
        b"\x89PNG\r\n\x1a\n",
    )
    .unwrap();
    temp.write_bytes(&at("images/icons/bf.png"), b"\x89PNG\r\n\x1a\n")
        .unwrap();
    temp.write_bytes(&at("images/icons/gf.png"), b"\x89PNG\r\n\x1a\n")
        .unwrap();
    temp.write_bytes(&at("images/icons/opponent.png"), b"\x89PNG\r\n\x1a\n")
        .unwrap();
    temp.write_bytes(
        &at("images/stages/backstage/floor.png"),
        b"\x89PNG\r\n\x1a\n",
    )
    .unwrap();
    temp.write_bytes(
        &at("images/stages/backstage/back.png"),
        b"\x89PNG\r\n\x1a\n",
    )
    .unwrap();

    temp.write(&at("songs/bop/meta.json"), meta_json()).unwrap();
    temp.write(&at("songs/bop/charts/hard.json"), chart_json())
        .unwrap();
    temp.write(
        &at("songs/bop/events.json"),
        r#"{"events": [{"time": 0, "name": "Camera Bump", "params": [false, 2]}]}"#,
    )
    .unwrap();
    temp.write_bytes(&at("songs/bop/song/Inst.ogg"), b"OggS")
        .unwrap();
    temp.write_bytes(&at("songs/bop/song/Voices.ogg"), b"OggS")
        .unwrap();
    temp.write(&at("songs/bop/scripts/notes.hx"), "// notes\n")
        .unwrap();
}

/// Creates a directory symlink, which may require elevated privileges on Windows.
pub fn link_dir(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, target)
    }
}

/// Creates a file symlink, which may require elevated privileges on Windows.
pub fn link_file(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(source, target)
    }
}

/// Diagnostics that a healthy fixture must never produce.
pub const UNEXPECTED_CODES: [&str; 12] = [
    "missing-asset",
    "case-mismatch",
    "conflicting-state-redirect",
    "missing-character-reference",
    "missing-stage-reference",
    "missing-state-reference",
    "undefined-event",
    "missing-song-file",
    "invalid-chart",
    "malformed-ini",
    "malformed-xml",
    "malformed-json",
];
