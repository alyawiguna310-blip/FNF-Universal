//! Integration tests for mod discovery through `mods/autoload.txt`.

mod common;

use fu_import::test_support::TempDir;
use fu_import::{discover_mods_root, import_mod, import_mods_root};

#[test]
fn imports_the_mod_listed_in_autoload() {
    let temp = TempDir::new("discovery-happy").unwrap();
    common::write_mods_directory(&temp);

    let outcome = import_mods_root(&temp.join("mods"));
    assert!(outcome.discovery.autoload_present);
    assert_eq!(outcome.discovery.mods.len(), 1);
    assert_eq!(outcome.discovery.mods[0].name, common::MOD_NAME);
    assert_eq!(outcome.mods.len(), 1);
    assert!(
        !outcome.mods[0].report.has_errors(),
        "{}",
        outcome.mods[0].report.to_text()
    );
    assert!(!outcome.discovery.report.has_errors());
}

#[test]
fn autoload_supports_comments_blank_lines_and_crlf() {
    let temp = TempDir::new("discovery-format").unwrap();
    common::write_mods_directory(&temp);
    temp.write(
        "mods/autoload.txt",
        "\u{feff}# the demo mod\r\n\r\n; second comment\r\ndemo\r\n",
    )
    .unwrap();

    let discovery = discover_mods_root(&temp.join("mods"));
    assert_eq!(discovery.mods.len(), 1);
    assert!(
        !discovery.report.has_errors(),
        "{}",
        discovery.report.to_text()
    );
}

#[test]
fn missing_autoload_falls_back_to_scanning() {
    let temp = TempDir::new("discovery-scan").unwrap();
    common::write_mods_directory(&temp);
    std::fs::remove_file(temp.join("mods/autoload.txt")).unwrap();

    let discovery = discover_mods_root(&temp.join("mods"));
    assert!(!discovery.autoload_present);
    assert!(discovery.report.contains_code("autoload-missing"));
    assert_eq!(discovery.mods.len(), 1);
    assert_eq!(discovery.mods[0].name, common::MOD_NAME);
}

#[test]
fn missing_autoload_entry_is_reported_but_others_still_import() {
    let temp = TempDir::new("discovery-missing-entry").unwrap();
    common::write_mods_directory(&temp);
    temp.write("mods/autoload.txt", "ghost\ndemo\n").unwrap();

    let outcome = import_mods_root(&temp.join("mods"));
    assert_eq!(outcome.discovery.mods.len(), 1);
    assert_eq!(outcome.discovery.mods[0].name, common::MOD_NAME);
    assert!(outcome
        .discovery
        .report
        .contains_code("autoload-entry-missing"));
    assert!(outcome.discovery.report.has_errors());
}

#[test]
fn unsafe_autoload_entries_are_rejected() {
    let temp = TempDir::new("discovery-unsafe").unwrap();
    common::write_mods_directory(&temp);
    temp.write(
        "mods/autoload.txt",
        "../outside\nC:\\Windows\n/absolute\n..\\up\n",
    )
    .unwrap();

    let discovery = discover_mods_root(&temp.join("mods"));
    assert!(discovery.mods.is_empty());
    assert!(discovery.report.contains_code("autoload-entry-malformed"));
    assert!(discovery.report.has_errors());
}

#[test]
fn duplicate_autoload_entries_are_reported_once() {
    let temp = TempDir::new("discovery-duplicate").unwrap();
    common::write_mods_directory(&temp);
    temp.write("mods/autoload.txt", "demo\ndemo\n").unwrap();

    let discovery = discover_mods_root(&temp.join("mods"));
    assert_eq!(discovery.mods.len(), 1);
    assert!(discovery.report.contains_code("autoload-entry-duplicate"));
}

#[test]
fn importing_a_single_mod_directory_works_without_autoload() {
    let temp = TempDir::new("discovery-direct").unwrap();
    common::write_mods_directory(&temp);

    let outcome = import_mod(&temp.join("mods/demo")).unwrap();
    assert_eq!(outcome.module.name, common::MOD_NAME);
    assert!(!outcome.report.has_errors(), "{}", outcome.report.to_text());
}

#[test]
fn a_missing_mods_directory_is_an_explicit_failure() {
    let temp = TempDir::new("discovery-missing-root").unwrap();
    let discovery = discover_mods_root(&temp.join("not-here"));
    assert!(discovery.mods.is_empty());
    assert!(discovery.report.contains_code("mod-root-missing"));
}

#[test]
fn non_directories_listed_in_autoload_are_malformed() {
    let temp = TempDir::new("discovery-file-entry").unwrap();
    common::write_mods_directory(&temp);
    temp.write("mods/notes.txt", "hello").unwrap();
    temp.write("mods/autoload.txt", "notes.txt\n").unwrap();

    let discovery = discover_mods_root(&temp.join("mods"));
    assert!(discovery.mods.is_empty());
    assert!(discovery.report.contains_code("autoload-entry-malformed"));
}
