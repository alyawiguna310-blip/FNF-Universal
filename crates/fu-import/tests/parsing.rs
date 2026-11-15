//! Integration tests for the parsed representation of a mod.

mod common;

use fu_import::codename::{AnchorRole, AssetKind, ChartFormat, ScriptLanguage, StageElement};
use fu_import::import_mod;
use fu_import::test_support::TempDir;

fn import_fixture(label: &str) -> (TempDir, fu_import::ImportOutcome) {
    let temp = TempDir::new(label).unwrap();
    common::write_mods_directory(&temp);
    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    (temp, outcome)
}

#[test]
fn imports_a_complete_mod() {
    let (_temp, outcome) = import_fixture("parse-complete");
    let module = &outcome.module;

    assert_eq!(module.name, common::MOD_NAME);
    assert!(!outcome.report.has_errors(), "{}", outcome.report.to_text());

    assert_eq!(module.characters.len(), 3);
    let bf = module.character("bf").expect("bf character");
    assert_eq!(bf.sprite.as_deref(), Some("BF"));
    assert_eq!(bf.icon.as_deref(), Some("bf"));
    assert_eq!(bf.animations.len(), 5);
    assert_eq!(bf.animations[1].name, "singLEFT");
    assert_eq!(bf.animations[1].animation.as_deref(), Some("BF Left"));
    assert_eq!(bf.animations[1].label.as_deref(), Some("true"));
    assert_eq!(bf.animations[1].x, Some(-10.0));

    assert_eq!(module.stages.len(), 1);
    let stage = module.stage("backstage").expect("stage");
    assert_eq!(stage.folder.as_deref(), Some("stages/backstage/"));
    assert_eq!(stage.sprites().count(), 2);
    let anchors: Vec<_> = stage.anchors().collect();
    assert_eq!(anchors.len(), 3);
    assert_eq!(anchors[0].tag, "girlfriend");
    assert_eq!(anchors[0].role, AnchorRole::Girlfriend);
    assert_eq!(anchors[2].role, AnchorRole::Player);
    assert_eq!(anchors[2].scale, Some(0.9));

    assert_eq!(module.events.len(), 1);
    let event = module.event_definition("Camera Bump").expect("event");
    assert_eq!(event.params.len(), 1);
    assert_eq!(event.params[0].name.as_deref(), Some("Amount"));
    assert!(
        module.defines_event("Camera Bump"),
        "the .hx companion counts"
    );

    assert_eq!(module.songs.len(), 1);
    let song = &module.songs[0];
    assert_eq!(song.name, "bop");
    assert_eq!(song.directory, "songs/bop");
    assert_eq!(song.audio.len(), 2, "instrumental and voices inventoried");
    assert_eq!(song.scripts.len(), 1);

    let metadata = song.metadata.as_ref().expect("metadata");
    assert_eq!(metadata.bpm, Some(120.0));
    assert_eq!(metadata.display_name.as_deref(), Some("Bop"));
    assert_eq!(metadata.needs_voices, Some(true));
    assert_eq!(metadata.difficulties, vec!["hard"]);
    assert_eq!(metadata.steps_per_beat, Some(4.0));
    assert!(metadata.gameover.is_none());
    assert_eq!(
        metadata
            .custom_values
            .as_ref()
            .and_then(|value| value.get("difficultyRating")),
        Some(&serde_json::Value::String("easy".to_string()))
    );

    assert_eq!(song.charts.len(), 1);
    let chart = &song.charts[0];
    assert_eq!(chart.difficulty, "hard");
    assert_eq!(chart.format, ChartFormat::Codename);
    assert_eq!(chart.chart_version.as_deref(), Some("1.6.0"));
    assert_eq!(chart.codename_chart, Some(true));
    assert_eq!(chart.scroll_speed, Some(2.2));
    assert_eq!(chart.stage.as_deref(), Some("backstage"));
    assert_eq!(chart.note_types.len(), 1);
    assert_eq!(chart.note_types[0].name, "No Animation");
    assert_eq!(chart.strum_lines.len(), 2);
    assert_eq!(chart.note_count(), 3);

    let opponent_line = &chart.strum_lines[0];
    assert_eq!(opponent_line.key_count, Some(4));
    assert_eq!(opponent_line.position.as_deref(), Some("dad"));
    assert_eq!(opponent_line.characters, vec!["opponent"]);
    assert_eq!(opponent_line.notes[1].time, Some(200.0));
    assert_eq!(opponent_line.notes[1].sustain_length, Some(100.0));

    let player_line = &chart.strum_lines[1];
    assert_eq!(player_line.strum_scale, Some(1.0));
    assert_eq!(player_line.strum_spacing, Some(1.0));
    assert_eq!(player_line.strum_line_pos, Some(0.75));
    assert_eq!(player_line.vocals_suffix.as_deref(), Some(""));
    assert_eq!(
        player_line.strum_pos.map(|pos| (pos.x, pos.y)),
        Some((0.0, 50.0))
    );
    assert_eq!(player_line.line_type, Some(serde_json::Value::from(1)));

    assert_eq!(chart.events.len(), 1);
    assert_eq!(chart.events[0].params.len(), 5);
    assert_eq!(chart.events[0].params[3], serde_json::Value::from("quart"));

    let song_events = song.events.as_ref().expect("song events");
    assert_eq!(song_events.events.len(), 1);

    assert_eq!(module.config.api_version.as_deref(), Some("3"));
    assert_eq!(
        module.config.flags.get("DISABLE_WARNING_SCREEN"),
        Some(&"true".to_string())
    );
    assert_eq!(
        module.config.flags.get("DEFAULT_MENU_MUSIC"),
        Some(&"menu song".to_string()),
        "flags.ini merges into the same flag map"
    );
    assert_eq!(
        module.config.discord.get("CLIENT_ID"),
        Some(&"123".to_string())
    );
    assert_eq!(module.config.state_redirects.len(), 1);
    assert_eq!(module.config.state_redirects[0].target, "TitleState");
    assert_eq!(module.config.unknown_sections, vec!["CustomSection"]);
    assert_eq!(module.config.sources.len(), 2);
}

#[test]
fn unknown_content_is_preserved_everywhere() {
    let (_temp, outcome) = import_fixture("parse-preserved");
    let module = &outcome.module;

    let bf = module.character("bf").unwrap();
    assert!(bf
        .attributes
        .iter()
        .any(|attribute| attribute.name == "mystery" && attribute.value == "kept"));

    let chart = &module.songs[0].charts[0];
    assert_eq!(
        chart.unknown_fields.get("modchartExtra"),
        Some(&serde_json::json!({"kept": true}))
    );
    assert!(chart.raw.get("modchartExtra").is_some());
    assert_eq!(
        chart.strum_lines[1].notes[0].extra.get("customNoteFlag"),
        Some(&serde_json::Value::from("kept"))
    );
    assert!(chart.strum_lines[1].raw.get("strumLinePos").is_some());

    let metadata = module.songs[0].metadata.as_ref().unwrap();
    assert_eq!(
        metadata.unknown_fields.get("unknownMeta"),
        Some(&serde_json::json!([1, 2, 3]))
    );

    let custom = module
        .config
        .sources
        .iter()
        .find(|source| source.path.ends_with("modpack.ini"))
        .unwrap()
        .document
        .section("CustomSection")
        .expect("unknown section is still present");
    assert_eq!(custom.get("keep"), Some("this"));

    assert!(outcome.report.contains_code("unknown-chart-field"));
    assert!(outcome.report.contains_code("unknown-metadata"));
}

#[test]
fn malformed_ini_is_reported_without_aborting_the_import() {
    let temp = TempDir::new("parse-bad-ini").unwrap();
    common::write_mods_directory(&temp);
    temp.write(
        "mods/demo/data/config/modpack.ini",
        "orphan=1\n[Broken\n[Flags]\nno_equals_here\nGOOD=yes\n",
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert!(outcome.report.contains_code("malformed-ini"));
    assert_eq!(
        outcome.module.config.flags.get("GOOD"),
        Some(&"yes".to_string())
    );
    assert_eq!(
        outcome.module.characters.len(),
        3,
        "rest of the mod still imports"
    );
}

#[test]
fn malformed_character_xml_is_reported() {
    let temp = TempDir::new("parse-bad-xml").unwrap();
    common::write_mods_directory(&temp);
    temp.write(
        "mods/demo/data/characters/broken.xml",
        "<character sprite=\"BF\"><anim name=\"idle\"</character>",
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert!(outcome.report.contains_code("malformed-xml"));
    assert!(
        outcome.module.character("broken").is_some(),
        "partial data is kept"
    );
    assert!(!outcome.report.has_errors(), "XML problems are recoverable");
}

#[test]
fn malformed_chart_json_is_an_error_but_other_songs_survive() {
    let temp = TempDir::new("parse-bad-json").unwrap();
    common::write_mods_directory(&temp);
    temp.write("mods/demo/songs/bop/charts/easy.json", "{ not json ")
        .unwrap();
    temp.write(
        "mods/demo/songs/bop/charts/extra.json",
        r#"{"strumLines": []}"#,
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert!(outcome.report.contains_code("malformed-json"));
    assert!(outcome.report.has_errors());
    assert_eq!(
        outcome.module.songs[0].charts.len(),
        2,
        "bad file skipped, good kept"
    );
    assert!(outcome.module.song("bop").is_some());
}

#[test]
fn case_mismatched_metadata_is_read_and_reported() {
    let temp = TempDir::new("parse-case").unwrap();
    common::write_mods_directory(&temp);
    std::fs::rename(
        temp.join("mods/demo/songs/bop/meta.json"),
        temp.join("mods/demo/songs/bop/Meta.json"),
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert!(outcome.report.contains_code("case-mismatch"));
    assert_eq!(
        outcome.module.songs[0].metadata.as_ref().unwrap().bpm,
        Some(120.0)
    );
}

#[test]
fn legacy_chart_layout_is_preserved_but_not_converted() {
    let temp = TempDir::new("parse-legacy").unwrap();
    common::write_mods_directory(&temp);
    temp.write(
        "mods/demo/songs/bop/charts/legacy.json",
        r#"{"song": {"song": "Bop", "bpm": 120, "speed": 2, "notes": [[0, 1, 2, 3]]}}"#,
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    let chart = outcome.module.songs[0]
        .chart("legacy")
        .expect("legacy chart imported");
    assert_eq!(chart.format, ChartFormat::LegacyNotes);
    assert!(chart.strum_lines.is_empty(), "nothing is invented");
    assert!(chart.raw.get("song").is_some(), "source data is preserved");
    assert!(outcome.report.contains_code("legacy-chart-format"));
    assert!(
        !outcome.report.has_errors(),
        "it is a warning, not a failure"
    );
}

#[test]
fn song_audio_and_scripts_are_inventoried_by_kind() {
    let (_temp, outcome) = import_fixture("parse-inventory");
    let song = &outcome.module.songs[0];
    assert!(song
        .audio
        .iter()
        .all(|entry| entry.kind == AssetKind::Audio));
    assert!(song
        .scripts
        .iter()
        .all(|script| script.language == ScriptLanguage::Haxe));
    assert_eq!(song.scripts[0].path, "songs/bop/scripts/notes.hx");
}

#[test]
fn atlases_are_recognized_without_decoding() {
    let (_temp, outcome) = import_fixture("parse-atlas");
    let inventory = &outcome.module.assets;
    assert!(inventory.atlases.iter().any(|atlas| atlas.kind
        == fu_import::codename::AtlasKind::Sparrow
        && atlas.image.as_deref() == Some("images/characters/BF.png")));
    assert!(inventory.atlases.iter().any(|atlas| atlas.kind
        == fu_import::codename::AtlasKind::AnimateFolder
        && atlas.folder.as_deref() == Some("images/characters/Opponent")));
    assert!(outcome.report.contains_code("partial-atlas"));
}

#[test]
fn stage_elements_keep_their_draw_order() {
    let (_temp, outcome) = import_fixture("parse-order");
    let stage = &outcome.module.stages[0];
    let kinds: Vec<&str> = stage
        .elements
        .iter()
        .map(|element| match element {
            StageElement::Sprite(_) => "sprite",
            StageElement::Anchor(_) => "anchor",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["sprite", "sprite", "anchor", "anchor", "anchor"]
    );
}
