//! Integration tests for the validation diagnostics.

mod common;

use fu_import::test_support::TempDir;
use fu_import::{import_mod, Severity, SupportLevel};

fn import_modified(label: &str, change: impl FnOnce(&TempDir)) -> fu_import::ImportOutcome {
    let temp = TempDir::new(label).unwrap();
    common::write_mods_directory(&temp);
    change(&temp);
    import_mod(&temp.join("mods/demo")).unwrap()
}

#[test]
fn healthy_fixture_has_no_unexpected_diagnostics() {
    let outcome = import_modified("validate-healthy", |_| {});
    for code in common::UNEXPECTED_CODES {
        assert!(
            !outcome.report.contains_code(code),
            "unexpected {code} in:\n{}",
            outcome.report.to_text()
        );
    }
    assert!(!outcome.report.has_errors());
    assert!(outcome.is_importable());
}

#[test]
fn missing_character_sprite_and_icon_are_reported() {
    let outcome = import_modified("validate-missing-sprite", |temp| {
        std::fs::remove_file(temp.join("mods/demo/images/characters/GF.png")).unwrap();
        std::fs::remove_file(temp.join("mods/demo/images/icons/gf.png")).unwrap();
    });

    let missing: Vec<_> = outcome.report.with_code("missing-asset").collect();
    assert!(missing.len() >= 2, "{}", outcome.report.to_text());
    assert!(missing
        .iter()
        .any(|diagnostic| diagnostic.message.contains("GF")));
    assert!(missing
        .iter()
        .any(|diagnostic| diagnostic.message.contains("icon")));
    assert!(missing
        .iter()
        .all(|diagnostic| diagnostic.severity == Severity::Warning));
}

#[test]
fn stage_sprite_with_wrong_casing_is_reported() {
    let outcome = import_modified("validate-case", |temp| {
        let folder = temp.join("mods/demo/images/stages/backstage");
        std::fs::remove_file(folder.join("floor.png")).unwrap();
        std::fs::write(folder.join("Floor.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    });

    let mismatches: Vec<_> = outcome.report.with_code("case-mismatch").collect();
    assert_eq!(mismatches.len(), 1, "{}", outcome.report.to_text());
    assert!(mismatches[0]
        .path
        .as_deref()
        .unwrap()
        .ends_with("Floor.png"));
    assert_eq!(mismatches[0].support, SupportLevel::Partial);
}

#[test]
fn duplicate_and_conflicting_state_redirects_are_distinguished() {
    let outcome = import_modified("validate-redirects", |temp| {
        temp.write("mods/demo/data/states/newMenu.hx", "class NewMenu {}")
            .unwrap();
        temp.write(
            "mods/demo/data/config/flags.ini",
            "[StateRedirects]\nTitleState=OtherTitle\nMainMenuState=newMenu\n",
        )
        .unwrap();
        temp.write(
            "mods/demo/data/config/modpack.ini",
            "[StateRedirects]\nTitleState=TitleState\nMainMenuState=newMenu\n",
        )
        .unwrap();
    });

    assert!(outcome.report.contains_code("conflicting-state-redirect"));
    assert!(outcome.report.contains_code("duplicate-state-redirect"));
    assert!(outcome.report.contains_code("missing-state-reference"));
    assert!(
        !outcome.report.has_errors(),
        "redirect problems are not fatal"
    );
}

#[test]
fn undefined_events_are_unverified_warnings() {
    let outcome = import_modified("validate-events", |temp| {
        temp.write(
            "mods/demo/songs/bop/charts/hard.json",
            r#"{"codenameChart": true, "stage": "backstage",
                "strumLines": [{"keyCount": 4, "position": "dad", "characters": ["opponent"],
                                "notes": [{"time": 0, "id": 0}]}],
                "events": [{"time": 0, "name": "Mystery Event", "params": [1]},
                           {"time": 1, "name": "Mystery Event", "params": [2]},
                           {"time": 2, "name": "Camera Bump", "params": [true]}]}"#,
        )
        .unwrap();
    });

    let undefined: Vec<_> = outcome.report.with_code("undefined-event").collect();
    assert_eq!(undefined.len(), 1, "reported once per event name");
    assert!(undefined[0].message.contains("Mystery Event"));
    assert_eq!(undefined[0].severity, Severity::Warning);
    assert_eq!(undefined[0].support, SupportLevel::Unverified);
    assert!(
        !outcome.report.has_errors(),
        "an unknown event is never treated as definitely missing"
    );
}

#[test]
fn missing_character_and_stage_references_are_unverified() {
    let outcome = import_modified("validate-references", |temp| {
        temp.write(
            "mods/demo/songs/bop/charts/hard.json",
            r#"{"codenameChart": true, "stage": "nowhere",
                "strumLines": [{"keyCount": 4, "position": "dad", "characters": ["ghost"],
                                "notes": []}]}"#,
        )
        .unwrap();
    });

    let characters: Vec<_> = outcome
        .report
        .with_code("missing-character-reference")
        .collect();
    assert_eq!(characters.len(), 1);
    assert_eq!(characters[0].support, SupportLevel::Unverified);
    let stages: Vec<_> = outcome
        .report
        .with_code("missing-stage-reference")
        .collect();
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].support, SupportLevel::Unverified);
}

#[test]
fn missing_song_files_are_errors() {
    let outcome = import_modified("validate-song-files", |temp| {
        std::fs::remove_file(temp.join("mods/demo/songs/bop/song/Inst.ogg")).unwrap();
        std::fs::remove_file(temp.join("mods/demo/songs/bop/song/Voices.ogg")).unwrap();
        std::fs::remove_file(temp.join("mods/demo/songs/bop/charts/hard.json")).unwrap();
    });

    let missing: Vec<_> = outcome.report.with_code("missing-song-file").collect();
    assert!(missing
        .iter()
        .any(|diagnostic| diagnostic.message.contains("instrumental")));
    assert!(missing
        .iter()
        .any(|diagnostic| diagnostic.message.contains("declared difficulty")));
    assert!(outcome.report.has_errors());
    assert!(!outcome.is_importable());
}

#[test]
fn missing_voices_is_a_warning_not_an_error() {
    let outcome = import_modified("validate-voices", |temp| {
        std::fs::remove_file(temp.join("mods/demo/songs/bop/song/Voices.ogg")).unwrap();
    });

    let voices: Vec<_> = outcome
        .report
        .with_code("missing-song-file")
        .filter(|diagnostic| diagnostic.message.contains("voices"))
        .collect();
    assert_eq!(voices.len(), 1);
    assert_eq!(voices[0].severity, Severity::Warning);
    assert!(!outcome.report.has_errors());
}

#[test]
fn unsupported_scripts_native_libraries_and_archives_are_recorded() {
    let outcome = import_modified("validate-unsupported", |temp| {
        temp.write("mods/demo/plugins/evil.dll", "MZ").unwrap();
        temp.write("mods/demo/tools/run.exe", "MZ").unwrap();
        temp.write("mods/demo/backup.rar", "RAR").unwrap();
        temp.write(
            "mods/demo/data/events/Custom Event.json",
            r#"{"params": []}"#,
        )
        .unwrap();
        temp.write("mods/demo/data/events/Custom Event.lua", "-- lua")
            .unwrap();
    });

    assert!(outcome.report.contains_code("unsupported-script"));
    assert!(outcome.report.contains_code("unsupported-binary"));
    assert!(outcome.report.contains_code("unsupported-archive"));

    let reasons: Vec<_> = outcome
        .module
        .unsupported_files
        .iter()
        .map(|file| file.reason.clone())
        .collect();
    assert!(reasons.contains(&fu_import::codename::UnsupportedReason::NativeLibrary));
    assert!(reasons.contains(&fu_import::codename::UnsupportedReason::Executable));
    assert!(reasons.contains(&fu_import::codename::UnsupportedReason::Archive));

    assert!(outcome
        .module
        .scripts
        .iter()
        .any(|script| script.language == fu_import::codename::ScriptLanguage::Lua));
}

#[test]
fn invalid_chart_data_is_reported() {
    let outcome = import_modified("validate-chart-data", |temp| {
        temp.write(
            "mods/demo/songs/bop/charts/hard.json",
            r#"{"codenameChart": true,
                "strumLines": [{"keyCount": 4, "position": "dad", "characters": ["opponent"],
                                "notes": [{"time": 0, "id": 9}, {"id": 0}]}]}"#,
        )
        .unwrap();
    });

    let invalid: Vec<_> = outcome.report.with_code("invalid-chart").collect();
    assert!(
        invalid
            .iter()
            .any(|diagnostic| diagnostic.message.contains("lane 9")),
        "{}",
        outcome.report.to_text()
    );
    assert!(
        invalid
            .iter()
            .any(|diagnostic| diagnostic.message.contains("'time'")),
        "{}",
        outcome.report.to_text()
    );
}

#[test]
fn report_summary_and_text_are_usable() {
    let outcome = import_modified("validate-summary", |temp| {
        std::fs::remove_file(temp.join("mods/demo/images/characters/GF.png")).unwrap();
    });

    let summary = outcome.report.summary();
    assert_eq!(summary.errors, 0);
    assert!(summary.warnings >= 1);
    assert_eq!(summary.support, SupportLevel::Partial);
    let text = outcome.report.to_text();
    assert!(text.contains("import report: demo"));
    assert!(text.contains("support: partial"));
}

#[test]
fn duplicate_strum_lines_are_reported() {
    let outcome = import_modified("validate-duplicates", |temp| {
        temp.write(
            "mods/demo/songs/bop/charts/hard.json",
            r#"{"codenameChart": true, "stage": "backstage",
                "strumLines": [
                  {"keyCount": 4, "position": "dad", "characters": ["opponent"], "notes": []},
                  {"keyCount": 4, "position": "dad", "characters": ["opponent"], "notes": []}
                ]}"#,
        )
        .unwrap();
    });

    let duplicates: Vec<_> = outcome.report.with_code("duplicate-definition").collect();
    assert_eq!(duplicates.len(), 1, "{}", outcome.report.to_text());
}

#[test]
fn duplicate_animation_names_are_reported() {
    let outcome = import_modified("validate-animations", |temp| {
        temp.write(
            "mods/demo/data/characters/bf.xml",
            "<character sprite=\"BF\" icon=\"bf\">\
<anim name=\"idle\" anim=\"BF Idle\"/>\
<anim name=\"IDLE\" anim=\"BF Idle 2\"/>\
</character>",
        )
        .unwrap();
    });

    let issues: Vec<_> = outcome.report.with_code("invalid-animation").collect();
    assert_eq!(issues.len(), 1, "{}", outcome.report.to_text());
}
