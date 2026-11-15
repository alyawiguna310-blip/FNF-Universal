//! End-to-end tests for the V-Slice importer.
//!
//! Fixtures are synthetic and follow the public V-Slice data formats: mod
//! metadata from `_polymod_meta.json`, song metadata `2.2.x`, chart data `2.0.x`
//! and the character/stage JSON shapes. They are not copies of a real mod.

use fu_import::test_support::TempDir;
use fu_import::vslice::{
    import_vslice_mod, import_vslice_mods_root, is_vslice_directory, validate, VsliceMod,
};

const MOD_METADATA: &str = r##"{
  "title": "Demo Mod",
  "description": "A test mod",
  "contributors": [{"name": "Tester", "role": "Developer"}],
  "api_version": "0.5.0",
  "mod_version": "1.0.0",
  "license": "CC BY 4.0",
  "dependencies": {"other": "1.0.0"},
  "customFlag": {"nested": true}
}"##;

const SONG_METADATA: &str = r##"{
  "version": "2.2.3",
  "songName": "Dad Battle",
  "artist": "Kawai Sprite",
  "charter": "Tester",
  "timeFormat": "ms",
  "timeChanges": [{"t": 0, "bpm": 180}, {"t": 60000, "bpm": 200}],
  "offsets": {"instrumental": 0, "vocals": 10},
  "playData": {
    "album": "volume1",
    "previewStart": 0,
    "previewEnd": 15000,
    "ratings": {"easy": 1, "hard": 5},
    "songVariations": [],
    "difficulties": ["easy", "hard"],
    "characters": {"player": "bf", "girlfriend": "gf", "opponent": "dad"},
    "stage": "mainStage",
    "noteStyle": "funkin"
  },
  "generatedBy": "test"
}"##;

const SONG_CHART: &str = r##"{
  "version": "2.0.0",
  "scrollSpeed": {"easy": 1.0, "hard": 1.4},
  "events": [{"t": 500, "e": "Zoom Camera", "v": {"zoom": 1.2}}],
  "notes": {
    "easy": [{"t": 1000, "d": 0, "l": 0, "k": ""}],
    "hard": [
      {"t": 1000, "d": 4, "l": 250},
      {"t": 1500, "d": 1, "k": "alt", "p": [{"n": "flip", "v": true}]}
    ]
  },
  "editorNotes": "keep me"
}"##;

fn write_full_mod(temp: &TempDir, root: &str) {
    temp.write(&format!("{root}/_polymod_meta.json"), MOD_METADATA)
        .unwrap();
    temp.write(
        &format!("{root}/songs/dadbattle/dadbattle-metadata.json"),
        SONG_METADATA,
    )
    .unwrap();
    temp.write(
        &format!("{root}/songs/dadbattle/dadbattle-chart.json"),
        SONG_CHART,
    )
    .unwrap();
    temp.write(&format!("{root}/songs/dadbattle/Inst.ogg"), "audio")
        .unwrap();
    temp.write(
        &format!("{root}/data/characters/bf.json"),
        r##"{
  "name": "Boyfriend",
  "renderType": "sparrow",
  "assetPath": "characters/BOYFRIEND",
  "scale": 1,
  "healthIcon": {"id": "bf", "isPixel": false},
  "animations": [
    {"name": "idle", "prefix": "BF idle", "frameRate": 24, "looped": true},
    {"name": "singLEFT", "prefix": "BF left", "frameRate": 24}
  ]
}"##,
    )
    .unwrap();
    temp.write(
        &format!("{root}/data/stages/mainStage.json"),
        r##"{
  "name": "Main Stage",
  "directory": "week1",
  "cameraZoom": 0.9,
  "props": [
    {"name": "floor", "assetPath": "week1/floor", "position": [0, 0], "scroll": [1, 1]},
    {"name": "backdrop", "assetPath": "week1/backdrop"}
  ],
  "characters": {
    "bf": {"position": [0, 0], "scale": 1},
    "dad": {"position": [300, 0], "cameraOffsets": [100, -100]},
    "gf": {"position": [-200, 0]}
  }
}"##,
    )
    .unwrap();
    temp.write(
        &format!("{root}/weeks/week1.json"),
        r##"{"name": "Week 1", "storyName": "Daddy Dearest", "songs": ["dadbattle"]}"##,
    )
    .unwrap();
    temp.write(&format!("{root}/images/characters/BOYFRIEND.png"), "png")
        .unwrap();
    temp.write(&format!("{root}/images/characters/BOYFRIEND.xml"), "xml")
        .unwrap();
    temp.write(&format!("{root}/images/week1/floor.png"), "png")
        .unwrap();
    temp.write(&format!("{root}/images/week1/backdrop.png"), "png")
        .unwrap();
}

#[test]
fn imports_a_complete_mod() {
    let temp = TempDir::new("vslice-full").unwrap();
    write_full_mod(&temp, "mods/demo");

    let outcome = import_vslice_mod(&temp.join("mods/demo")).expect("import");
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report);

    let module = &outcome.module;
    assert_eq!(
        module.metadata.as_ref().unwrap().title.as_deref(),
        Some("Demo Mod")
    );
    assert_eq!(
        module.metadata.as_ref().unwrap().dependencies.get("other"),
        Some(&"1.0.0".to_string())
    );
    assert_eq!(module.songs.len(), 1);

    let song = module.song("dadbattle").expect("song");
    let metadata = song.metadata.as_ref().expect("song metadata");
    assert_eq!(metadata.time_changes.len(), 2);
    assert_eq!(metadata.offsets.len(), 2);

    let chart = song.chart_for(None).expect("chart");
    assert_eq!(chart.notes["hard"].len(), 2);
    assert_eq!(chart.notes["hard"][1].kind, Some(serde_json::json!("alt")));
    assert_eq!(chart.scroll_speed.get("easy"), Some(&1.0));
    assert_eq!(chart.events.len(), 1);

    assert_eq!(module.characters.len(), 1);
    assert_eq!(module.stages.len(), 1);
    assert_eq!(module.weeks.len(), 1);
    assert!(outcome.support() != fu_import::SupportLevel::Unsupported);
}

#[test]
fn unknown_fields_and_raw_documents_are_preserved() {
    let temp = TempDir::new("vslice-unknown").unwrap();
    write_full_mod(&temp, "mod");

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    let module = &outcome.module;

    let metadata = module.metadata.as_ref().unwrap();
    assert!(metadata.unknown_fields.contains_key("customFlag"));
    assert_eq!(metadata.raw["api_version"], serde_json::json!("0.5.0"));

    let chart = module.song("dadbattle").unwrap().chart_for(None).unwrap();
    assert_eq!(
        chart.unknown_fields.get("editorNotes"),
        Some(&serde_json::json!("keep me"))
    );

    let character = module.character("bf").unwrap();
    assert_eq!(
        character.unknown_fields.get("renderType"),
        None,
        "known fields are not duplicated into unknown_fields"
    );
    assert_eq!(character.raw["renderType"], serde_json::json!("sparrow"));
}

#[test]
fn manifest_only_mods_are_imported() {
    let temp = TempDir::new("vslice-manifest-mod").unwrap();
    temp.write(
        "mod/manifest.json",
        r##"{"id": "demo.mod", "title": "Manifest Mod", "version": "0.6.0"}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/x/x-chart.json",
        r##"{"version": "2.0.0", "notes": {"hard": [{"t": 0, "d": 0}]}}"##,
    )
    .unwrap();

    assert!(is_vslice_directory(&temp.join("mod")));
    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    let metadata = outcome.module.metadata.as_ref().expect("metadata");
    assert_eq!(metadata.id.as_deref(), Some("demo.mod"));
    assert_eq!(metadata.title.as_deref(), Some("Manifest Mod"));
}

#[test]
fn discovers_and_imports_every_mod_in_a_mods_root() {
    let temp = TempDir::new("vslice-root").unwrap();
    write_full_mod(&temp, "mods/alpha");
    temp.write("mods/beta/_polymod_meta.json", r##"{"title": "Beta"}"##)
        .unwrap();
    temp.write("mods/not-a-mod/readme.txt", "hello").unwrap();

    let root = import_vslice_mods_root(&temp.join("mods"));
    let names: Vec<_> = root
        .mods
        .iter()
        .map(|outcome| outcome.module.name.as_str())
        .collect();
    assert_eq!(names, vec!["alpha", "beta"]);
    assert!(root.discovery.report.contains_code("missing-metadata"));
    assert!(!root.discovery.report.has_errors());
}

#[test]
fn scripts_and_binaries_are_inventoried_only() {
    let temp = TempDir::new("vslice-scripts").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Scripts"}"##)
        .unwrap();
    temp.write("mod/scripts/Camera.hx", "class Camera {}")
        .unwrap();
    temp.write("mod/scripts/helper.lua", "print('x')").unwrap();
    temp.write_bytes("mod/libs/native.ndll", &[0x00, 0x01, 0x02])
        .unwrap();
    temp.write_bytes("mod/assets/pack.zip", &[0x50, 0x4b])
        .unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    assert_eq!(outcome.module.scripts.len(), 2);
    assert_eq!(outcome.module.unsupported_files.len(), 2);
    assert!(outcome.report.contains_code("unsupported-script"));
    assert!(outcome.report.contains_code("unsupported-binary"));
    assert!(outcome.report.contains_code("unsupported-archive"));
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report);
}

#[test]
fn song_variations_are_paired_with_their_metadata_and_chart() {
    let temp = TempDir::new("vslice-variations").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Variations"}"##)
        .unwrap();
    temp.write(
        "mod/songs/song/song-metadata.json",
        r##"{
  "version": "2.2.3",
  "timeChanges": [{"t": 0, "bpm": 180}],
  "playData": {"difficulties": ["hard"], "songVariations": ["erect"]}
}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/song/song-chart.json",
        r##"{"version": "2.0.0", "notes": {"hard": [{"t": 0, "d": 0}]}}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/song/song-metadata-erect.json",
        r##"{
  "version": "2.2.3",
  "timeChanges": [{"t": 0, "bpm": 200}],
  "playData": {"difficulties": ["hard"]}
}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/song/song-chart-erect.json",
        r##"{"version": "2.0.0", "notes": {"hard": [{"t": 10, "d": 1}]}}"##,
    )
    .unwrap();
    temp.write("mod/songs/song/Inst.ogg", "audio").unwrap();
    temp.write("mod/songs/song/Inst-erect.ogg", "audio")
        .unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    let song = outcome.module.song("song").expect("song");
    assert!(song.metadata_for(Some("erect")).is_some());
    assert_eq!(
        song.chart_for(Some("erect")).unwrap().notes["hard"].len(),
        1
    );
    assert!(
        !outcome.report.contains_code("missing-song-file"),
        "{:?}",
        outcome.report
    );
}

#[test]
fn malformed_documents_are_reported_and_kept() {
    let temp = TempDir::new("vslice-malformed").unwrap();
    temp.write("mod/_polymod_meta.json", "{ broken").unwrap();
    temp.write("mod/songs/x/x-metadata.json", "[]").unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    assert!(outcome.report.contains_code("malformed-json"));
    assert!(outcome.report.contains_code("malformed-metadata"));

    let paths: Vec<_> = outcome
        .module
        .unparsed
        .iter()
        .map(|document| document.path.as_str())
        .collect();
    assert!(paths.contains(&"_polymod_meta.json"));
    assert!(paths.iter().any(|path| path.ends_with("x-metadata.json")));
    let preserved = outcome
        .module
        .unparsed
        .iter()
        .find(|document| document.path == "_polymod_meta.json")
        .unwrap();
    assert_eq!(preserved.contents, "{ broken");
}

#[test]
fn traversing_asset_references_are_rejected() {
    let temp = TempDir::new("vslice-traversal").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Evil"}"##)
        .unwrap();
    temp.write(
        "mod/data/characters/evil.json",
        r##"{
  "assetPath": "../../../../outside",
  "animations": [{"name": "idle", "prefix": "evil"}]
}"##,
    )
    .unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    assert!(outcome.report.contains_code("unsafe-path"));
    assert!(!outcome.report.contains_code("missing-asset"));
}

#[test]
fn a_charts_folder_is_used_when_the_song_has_no_chart_file() {
    let temp = TempDir::new("vslice-charts-folder").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Charts"}"##)
        .unwrap();
    temp.write(
        "mod/songs/x/metadata.json",
        r##"{"version": "2.2.3", "timeChanges": [{"t": 0, "bpm": 120}], "playData": {"difficulties": ["hard"]}}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/x/charts/hard.json",
        r##"{"version": "2.0.0", "notes": {"hard": [{"t": 5, "d": 2}]}}"##,
    )
    .unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    let song = outcome.module.song("x").expect("song");
    assert_eq!(song.charts.len(), 1);
    assert_eq!(song.charts[0].notes["hard"].len(), 1);
    assert!(outcome.report.contains_code("unknown-chart-field"));
}

#[test]
fn validation_reports_missing_difficulties_and_audio() {
    let temp = TempDir::new("vslice-validate").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Validate"}"##)
        .unwrap();
    temp.write(
        "mod/songs/y/y-metadata.json",
        r##"{
  "version": "2.2.3",
  "timeChanges": [{"t": 0, "bpm": 150}],
  "playData": {"difficulties": ["easy", "hard"]}
}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/y/y-chart.json",
        r##"{"version": "2.0.0", "notes": {"easy": [{"t": 0, "d": 0}]}}"##,
    )
    .unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    assert!(outcome.report.contains_code("missing-difficulty"));
    assert!(outcome.report.contains_code("missing-song-audio"));
    assert!(!outcome.report.has_errors());

    let standalone = validate(&outcome.module);
    assert!(standalone.contains_code("missing-difficulty"));
}

#[test]
fn unsupported_format_versions_are_flagged() {
    let temp = TempDir::new("vslice-versions").unwrap();
    temp.write("mod/_polymod_meta.json", r##"{"title": "Old"}"##)
        .unwrap();
    temp.write(
        "mod/songs/z/z-metadata.json",
        r##"{"version": "2.0.0", "timeFormat": "beats", "timeChanges": [{"t": 100, "bpm": 100}]}"##,
    )
    .unwrap();
    temp.write(
        "mod/songs/z/z-chart.json",
        r##"{"version": "1.0.0", "notes": {}}"##,
    )
    .unwrap();

    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    let versions = outcome
        .report
        .with_code("unsupported-format-version")
        .count();
    assert!(versions >= 3, "{:?}", outcome.report);
    assert!(outcome.report.contains_code("malformed-metadata"));
}

#[test]
fn the_model_round_trips_through_json() {
    let temp = TempDir::new("vslice-round-trip").unwrap();
    write_full_mod(&temp, "mod");
    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");

    let json = serde_json::to_string(&outcome.module).expect("serialize");
    let parsed: VsliceMod = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, outcome.module);
}

#[test]
fn an_empty_directory_is_importable_but_reported() {
    let temp = TempDir::new("vslice-empty-mod").unwrap();
    temp.mkdir("mod").unwrap();
    let outcome = import_vslice_mod(&temp.join("mod")).expect("import");
    assert!(outcome.is_importable());
    assert!(outcome.report.contains_code("missing-metadata"));
    assert!(outcome.module.songs.is_empty());
}

#[test]
fn importing_a_missing_directory_is_an_error() {
    let temp = TempDir::new("vslice-missing-mod").unwrap();
    assert!(import_vslice_mod(&temp.join("nope")).is_err());
}
