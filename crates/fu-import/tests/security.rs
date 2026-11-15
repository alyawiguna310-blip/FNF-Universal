//! Integration tests for the importer's security boundaries.
//!
//! These tests only assert that nothing escapes, executes or is embedded. The
//! symlink tests need permission to create links; when the platform refuses,
//! the test reports that it was skipped instead of silently passing a link
//! assertion.

mod common;

use fu_import::path::SafeRelativePath;
use fu_import::test_support::TempDir;
use fu_import::{import_mod, import_mod_with, ParseOptions};

fn serialized(outcome: &fu_import::ImportOutcome) -> String {
    serde_json::to_string(&outcome.module).expect("module serializes")
}

#[test]
fn traversal_and_absolute_paths_are_rejected() {
    let rejected = [
        "../secret",
        "a/../../secret",
        r"..\secret",
        "/etc/passwd",
        r"C:\Windows\System32\drivers\etc\hosts",
        "C:relative",
        r"\\server\share",
        "images/\0.png",
        "images/CON.png",
        "images/notes.png:stream",
    ];
    for input in rejected {
        assert!(
            SafeRelativePath::parse(input).is_err(),
            "expected rejection for {input:?}"
        );
    }

    assert_eq!(
        SafeRelativePath::parse("images/characters/BF.png")
            .unwrap()
            .as_str(),
        "images/characters/BF.png"
    );
}

#[test]
fn autoload_cannot_point_outside_the_mods_directory() {
    let temp = TempDir::new("security-autoload").unwrap();
    common::write_mods_directory(&temp);
    temp.write("outside/secret.txt", "OUTSIDE_MARKER").unwrap();
    temp.write("mods/autoload.txt", "../outside\n..\\outside\ndemo\n")
        .unwrap();

    let discovery = fu_import::discover_mods_root(&temp.join("mods"));
    assert_eq!(discovery.mods.len(), 1);
    assert_eq!(discovery.mods[0].name, "demo");
    assert!(discovery.report.contains_code("autoload-entry-malformed"));
}

#[test]
fn symlinked_mod_directory_outside_the_root_is_rejected() {
    let temp = TempDir::new("security-symlink-mod").unwrap();
    common::write_mods_directory(&temp);
    temp.write("outside/mod/data/config/modpack.ini", "[Flags]\nA=b\n")
        .unwrap();

    let link = temp.join("mods/linked");
    if common::link_dir(&temp.join("outside/mod"), &link).is_err() {
        eprintln!("skipping: this environment does not permit creating directory symlinks");
        return;
    }
    temp.write("mods/autoload.txt", "linked\n").unwrap();

    let discovery = fu_import::discover_mods_root(&temp.join("mods"));
    assert!(
        discovery.report.contains_code("unsafe-path"),
        "{}",
        discovery.report.to_text()
    );
    assert!(
        discovery.mods.iter().all(|module| module.name != "linked"),
        "a link outside the mods directory must not be imported"
    );
}

#[test]
fn symlinked_files_inside_a_mod_are_not_read() {
    let temp = TempDir::new("security-symlink-file").unwrap();
    common::write_mods_directory(&temp);
    temp.write("outside/secret.txt", "OUTSIDE_SECRET_MARKER")
        .unwrap();

    let link = temp.join("mods/demo/leak.txt");
    if common::link_file(&temp.join("outside/secret.txt"), &link).is_err() {
        eprintln!("skipping: this environment does not permit creating file symlinks");
        return;
    }

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert!(
        outcome.report.contains_code("scan-symlink-skipped"),
        "{}",
        outcome.report.to_text()
    );
    assert!(
        outcome.module.assets.get("leak.txt").is_none(),
        "a symlinked file must not be inventoried"
    );
    assert!(
        !serialized(&outcome).contains("OUTSIDE_SECRET_MARKER"),
        "symlinked content must never be imported"
    );
}

#[test]
fn scripts_are_inventoried_but_never_read_or_executed() {
    let temp = TempDir::new("security-scripts").unwrap();
    common::write_mods_directory(&temp);
    temp.write(
        "mods/demo/data/scripts/danger.hx",
        "// DANGEROUS_SCRIPT_MARKER\nclass Danger { static function go() Sys.command(\"echo pwned\"); }\n",
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    let script = outcome
        .module
        .scripts
        .iter()
        .find(|script| script.path.ends_with("danger.hx"))
        .expect("script is inventoried");
    assert!(script.size > 0, "size is recorded without reading content");
    assert!(outcome.report.contains_code("unsupported-script"));
    assert!(
        !serialized(&outcome).contains("DANGEROUS_SCRIPT_MARKER"),
        "script source must never be embedded in the model"
    );
    assert!(!temp.exists("pwned"));
    assert!(!temp.exists("mods/demo/pwned"));
}

#[test]
fn native_libraries_and_executables_are_never_loaded() {
    let temp = TempDir::new("security-binaries").unwrap();
    common::write_mods_directory(&temp);
    temp.write("mods/demo/lib/native.ndll", "NATIVE_LIBRARY_MARKER")
        .unwrap();
    temp.write("mods/demo/tools/patch.exe", "EXECUTABLE_MARKER")
        .unwrap();
    temp.write("mods/demo/lib/plugin.so", "SO_MARKER").unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert_eq!(
        outcome
            .module
            .unsupported_files
            .iter()
            .filter(|file| file.reason == fu_import::codename::UnsupportedReason::NativeLibrary)
            .count(),
        2
    );
    assert!(outcome
        .module
        .unsupported_files
        .iter()
        .any(|file| file.reason == fu_import::codename::UnsupportedReason::Executable));

    let model = serialized(&outcome);
    for marker in ["NATIVE_LIBRARY_MARKER", "EXECUTABLE_MARKER", "SO_MARKER"] {
        assert!(!model.contains(marker), "{marker} must not be embedded");
    }
}

#[test]
fn xml_external_entities_are_never_resolved() {
    let temp = TempDir::new("security-xxe").unwrap();
    common::write_mods_directory(&temp);
    temp.write("outside/secret.txt", "XXE_SECRET_MARKER")
        .unwrap();
    let secret = temp.join("outside/secret.txt");
    let uri = format!("file:///{}", secret.to_string_lossy().replace('\\', "/"));

    temp.write(
        "mods/demo/data/characters/xxe.xml",
        &format!(
            "<!DOCTYPE character [<!ENTITY xxe SYSTEM \"{uri}\">]>\n\
<character sprite=\"&xxe;\" icon=\"&xxe;\"><anim name=\"idle\" anim=\"&xxe;\"/></character>\n"
        ),
    )
    .unwrap();

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    let character = outcome.module.character("xxe").expect("character parsed");
    assert_eq!(
        character.sprite.as_deref(),
        Some("&xxe;"),
        "the entity reference stays literal"
    );
    assert!(
        !serialized(&outcome).contains("XXE_SECRET_MARKER"),
        "external entities must never be expanded"
    );
}

#[test]
fn oversized_files_are_refused_instead_of_read() {
    let temp = TempDir::new("security-size").unwrap();
    common::write_mods_directory(&temp);

    let outcome = import_mod_with(
        &temp.join("mods/demo"),
        ParseOptions {
            max_depth: 32,
            max_file_bytes: 8,
        },
    )
    .unwrap();

    assert!(
        outcome.report.contains_code("malformed-metadata"),
        "{}",
        outcome.report.to_text()
    );
    assert!(
        outcome.module.songs[0].metadata.is_none(),
        "the oversized metadata file was not parsed"
    );
}

#[test]
fn depth_limit_stops_recursive_discovery() {
    let temp = TempDir::new("security-depth").unwrap();
    common::write_mods_directory(&temp);

    let outcome = import_mod_with(
        &temp.join("mods/demo"),
        ParseOptions {
            max_depth: 1,
            max_file_bytes: ParseOptions::default().max_file_bytes,
        },
    )
    .unwrap();

    assert!(outcome.report.contains_code("scan-depth-limit"));
    assert!(outcome.module.songs.is_empty(), "songs are below the limit");
    assert!(outcome.module.characters.is_empty());
}
