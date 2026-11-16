//! Semantic validation of an imported V-Slice mod.
//!
//! Validation is data-only: it never opens a script, decodes an image or
//! launches anything. Findings that depend on content the importer cannot see —
//! base-game characters and stages, since a V-Slice mod is an override layer —
//! are reported as warnings rather than treated as definite omissions.

use std::collections::{BTreeMap, BTreeSet};

use crate::codename::model::AssetLookup;
use crate::path::SafeRelativePath;
use crate::report::{DiagnosticKind, ImportReport};
use crate::vslice::model::{Chart, Note, Song, VsliceMod};

/// Song metadata format version family this importer reads.
pub const SUPPORTED_METADATA_VERSION_RULE: &str = "2.2.x";
/// Chart format version family this importer reads.
pub const SUPPORTED_CHART_VERSION_RULE: &str = "2.0.x";

/// Validates a parsed mod and returns a standalone report.
pub fn validate(module: &VsliceMod) -> ImportReport {
    let mut report = ImportReport::new();
    validate_into(module, &mut report);
    report.sort();
    report
}

/// Appends validation findings to an existing report.
pub fn validate_into(module: &VsliceMod, report: &mut ImportReport) {
    validate_project_metadata(module, report);
    validate_duplicate_definitions(module, report);
    validate_songs(module, report);
    validate_characters(module, report);
    validate_stages(module, report);
}

fn validate_project_metadata(module: &VsliceMod, report: &mut ImportReport) {
    let Some(metadata) = module.metadata.as_ref() else {
        return;
    };

    if let Some(api_version) = metadata.api_version.as_deref() {
        if version_parts(api_version).is_none() {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                metadata.source.clone(),
                format!("api_version '{api_version}' is not a semantic version"),
            );
        }
    }

    if let Some(icon) = metadata.icon.as_deref() {
        check_asset_reference(
            report,
            module,
            &metadata.source,
            icon,
            "metadata icon",
            false,
        );
    }
}

fn validate_duplicate_definitions(module: &VsliceMod, report: &mut ImportReport) {
    let mut definitions: Vec<(&str, &str, &str)> = Vec::new();
    for song in &module.songs {
        definitions.push(("song", &song.id, &song.directory));
    }
    for character in &module.characters {
        definitions.push(("character", &character.id, &character.source));
    }
    for stage in &module.stages {
        definitions.push(("stage", &stage.id, &stage.source));
    }
    for week in &module.weeks {
        definitions.push(("week", &week.id, &week.source));
    }

    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for (kind, id, source) in definitions {
        let key = format!("{kind}:{}", id.to_ascii_lowercase());
        match seen.get(&key) {
            Some(existing) => report.add_at(
                DiagnosticKind::DuplicateDefinition,
                source,
                format!("{kind} '{id}' is defined more than once (also in '{existing}')"),
            ),
            None => {
                seen.insert(key, source);
            }
        }
    }
}

fn validate_songs(module: &VsliceMod, report: &mut ImportReport) {
    for song in &module.songs {
        validate_song_metadata_versions(song, report);
        validate_song_charts(song, report);
        validate_song_audio(module, song, report);
        validate_song_references(module, song, report);
    }
}

fn validate_song_metadata_versions(song: &Song, report: &mut ImportReport) {
    let metadata_sets = song
        .metadata
        .iter()
        .chain(song.variation_metadata.iter())
        .collect::<Vec<_>>();

    for metadata in metadata_sets {
        match metadata.version.as_deref() {
            Some(version) => {
                if let Some((major, minor, _)) = version_parts(version) {
                    if (major, minor) != (2, 2) {
                        report.add_at(
                            DiagnosticKind::UnsupportedFormatVersion,
                            metadata.source.clone(),
                            format!(
                                "metadata version {version} is outside the supported \
                                 {SUPPORTED_METADATA_VERSION_RULE} range"
                            ),
                        );
                    }
                } else {
                    report.add_at(
                        DiagnosticKind::MalformedMetadata,
                        metadata.source.clone(),
                        format!("metadata version '{version}' is not a semantic version"),
                    );
                }
            }
            None => report.add_at(
                DiagnosticKind::MalformedMetadata,
                metadata.source.clone(),
                "metadata declares no `version` field",
            ),
        }

        if let Some(time_format) = metadata.time_format.as_deref() {
            if !time_format.eq_ignore_ascii_case("ms") {
                report.add_at(
                    DiagnosticKind::UnsupportedFormatVersion,
                    metadata.source.clone(),
                    format!("time format '{time_format}' is not supported; only 'ms' is defined"),
                );
            }
        }

        if metadata.time_changes.is_empty() {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                metadata.source.clone(),
                "metadata declares no BPM change",
            );
        } else if metadata.time_changes[0].t.unwrap_or(0.0) > 0.0 {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                metadata.source.clone(),
                "the first BPM change must not start after timestamp 0",
            );
        }

        if let Some(play_data) = metadata.play_data.as_ref() {
            if play_data.difficulties.is_empty() {
                report.add_at(
                    DiagnosticKind::MalformedMetadata,
                    metadata.source.clone(),
                    "play data declares no difficulties",
                );
            }
        }
    }
}

fn validate_song_charts(song: &Song, report: &mut ImportReport) {
    let declared: BTreeSet<String> = song
        .difficulties()
        .into_iter()
        .map(|difficulty| difficulty.to_ascii_lowercase())
        .collect();

    for chart in &song.charts {
        match chart.version.as_deref() {
            Some(version) => {
                if let Some((major, minor, _)) = version_parts(version) {
                    if (major, minor) != (2, 0) {
                        report.add_at(
                            DiagnosticKind::UnsupportedFormatVersion,
                            chart.source.clone(),
                            format!(
                                "chart version {version} is outside the supported \
                                 {SUPPORTED_CHART_VERSION_RULE} range"
                            ),
                        );
                    }
                } else {
                    report.add_at(
                        DiagnosticKind::MalformedMetadata,
                        chart.source.clone(),
                        format!("chart version '{version}' is not a semantic version"),
                    );
                }
            }
            None => report.add_at(
                DiagnosticKind::MalformedMetadata,
                chart.source.clone(),
                "chart declares no `version` field",
            ),
        }

        if chart.variation.is_none() {
            let charted: BTreeSet<String> = chart
                .notes
                .iter()
                .filter(|(_, notes)| !notes.is_empty())
                .map(|(difficulty, _)| difficulty.to_ascii_lowercase())
                .collect();

            for difficulty in &declared {
                if !charted.contains(difficulty) {
                    report.add_at(
                        DiagnosticKind::MissingDifficulty,
                        chart.source.clone(),
                        format!("difficulty '{difficulty}' is declared but has no notes"),
                    );
                }
            }
            for difficulty in charted.difference(&declared) {
                if declared.is_empty() {
                    continue;
                }
                report.add_at(
                    DiagnosticKind::UndeclaredDifficulty,
                    chart.source.clone(),
                    format!("difficulty '{difficulty}' has notes but is not declared"),
                );
            }
        }

        validate_chart_content(chart, report);
    }

    if let Some(metadata) = song.metadata.as_ref() {
        if let Some(play_data) = metadata.play_data.as_ref() {
            for variation in &play_data.song_variations {
                let has_metadata = song
                    .variation_metadata
                    .iter()
                    .any(|candidate| candidate.variation.as_deref() == Some(variation.as_str()));
                let has_chart = song
                    .charts
                    .iter()
                    .any(|chart| chart.variation.as_deref() == Some(variation.as_str()));
                if !has_metadata || !has_chart {
                    report.add_at(
                        DiagnosticKind::MissingSongFile,
                        song.directory.clone(),
                        format!(
                            "variation '{variation}' is declared but its {} file is missing",
                            if !has_metadata { "metadata" } else { "chart" }
                        ),
                    );
                }
            }
        }
    }
}

fn validate_chart_content(chart: &Chart, report: &mut ImportReport) {
    for (difficulty, notes) in &chart.notes {
        for note in notes {
            validate_note(chart, difficulty, note, report);
        }
    }

    for (index, event) in chart.events.iter().enumerate() {
        match event.time_ms {
            Some(time) if time.is_finite() && time >= 0.0 => {}
            _ => report.add_at(
                DiagnosticKind::InvalidEvent,
                chart.source.clone(),
                format!("event {index} has no finite, non-negative timestamp"),
            ),
        }
        if event.name.as_deref().unwrap_or_default().trim().is_empty() {
            report.add_at(
                DiagnosticKind::InvalidEvent,
                chart.source.clone(),
                format!("event {index} has no name"),
            );
        }
    }
}

fn validate_note(chart: &Chart, difficulty: &str, note: &Note, report: &mut ImportReport) {
    match note.time_ms {
        Some(time) if time.is_finite() && time >= 0.0 => {}
        _ => report.add_at(
            DiagnosticKind::InvalidNote,
            chart.source.clone(),
            format!(
                "{difficulty} note {} has no finite, non-negative timestamp",
                note.index
            ),
        ),
    }

    match note.direction {
        Some(direction) if direction.is_finite() && (0.0..8.0).contains(&direction) => {
            if direction.fract() != 0.0 {
                report.add_at(
                    DiagnosticKind::InvalidNote,
                    chart.source.clone(),
                    format!(
                        "{difficulty} note {} uses non-integer direction {direction}",
                        note.index
                    ),
                );
            }
        }
        _ => report.add_at(
            DiagnosticKind::InvalidNote,
            chart.source.clone(),
            format!(
                "{difficulty} note {} uses a direction outside 0..=7",
                note.index
            ),
        ),
    }

    if let Some(sustain) = note.sustain_ms {
        if !sustain.is_finite() || sustain < 0.0 {
            report.add_at(
                DiagnosticKind::InvalidNote,
                chart.source.clone(),
                format!(
                    "{difficulty} note {} has an invalid sustain length {sustain}",
                    note.index
                ),
            );
        }
    }
}

fn validate_song_audio(module: &VsliceMod, song: &Song, report: &mut ImportReport) {
    let has_instrumental = song.audio.iter().any(|entry| {
        entry
            .path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("Inst.ogg"))
    });
    if !has_instrumental {
        report.add_at(
            DiagnosticKind::MissingSongAudio,
            song.directory.clone(),
            "no `Inst.ogg` instrumental was found; it may be provided by the base game",
        );
    }

    let declared: BTreeSet<String> = song
        .variation_metadata
        .iter()
        .filter_map(|metadata| metadata.variation.clone())
        .collect();
    for variation in declared {
        let wanted = format!("Inst-{variation}.ogg");
        let found = module
            .assets
            .files_under(&song.directory)
            .any(|entry| entry.path.ends_with(&wanted));
        if !found {
            // Alternative instrumentals are optional, so this is informational.
            report.add_at(
                DiagnosticKind::MissingAsset,
                song.directory.clone(),
                format!("variation '{variation}' has no '{wanted}'"),
            );
        }
    }
}

fn validate_song_references(module: &VsliceMod, song: &Song, report: &mut ImportReport) {
    let metadata_sets = song
        .metadata
        .iter()
        .chain(song.variation_metadata.iter())
        .collect::<Vec<_>>();

    for metadata in metadata_sets {
        let Some(play_data) = metadata.play_data.as_ref() else {
            continue;
        };

        if let Some(characters) = play_data.characters.as_ref() {
            for (role, id) in [
                ("player", characters.player.as_deref()),
                ("girlfriend", characters.girlfriend.as_deref()),
                ("opponent", characters.opponent.as_deref()),
            ] {
                let Some(id) = id.filter(|id| !id.trim().is_empty()) else {
                    continue;
                };
                if module.character(id).is_none() {
                    report.add_at(
                        DiagnosticKind::MissingCharacterReference,
                        metadata.source.clone(),
                        format!(
                            "{role} character '{id}' is not defined by this mod; it may be \
                             provided by the base game"
                        ),
                    );
                }
            }
        }

        if let Some(stage) = play_data
            .stage
            .as_deref()
            .filter(|stage| !stage.trim().is_empty())
        {
            if module.stage(stage).is_none() {
                report.add_at(
                    DiagnosticKind::MissingStageReference,
                    metadata.source.clone(),
                    format!(
                        "stage '{stage}' is not defined by this mod; it may be provided by \
                         the base game"
                    ),
                );
            }
        }
    }
}

fn validate_characters(module: &VsliceMod, report: &mut ImportReport) {
    for character in &module.characters {
        if let Some(asset_path) = character
            .asset_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
        {
            check_asset_reference(
                report,
                module,
                &character.source,
                asset_path,
                "character spritesheet",
                false,
            );
        }
    }
}

fn validate_stages(module: &VsliceMod, report: &mut ImportReport) {
    for stage in &module.stages {
        if stage.layers.is_some() {
            report.add_at(
                DiagnosticKind::LegacyChartFormat,
                stage.source.clone(),
                "stage uses a `layers` array; it is preserved verbatim but only `props` \
                 are interpreted",
            );
        }
        for prop in &stage.props {
            let Some(asset_path) = prop.asset_path.as_deref() else {
                continue;
            };
            if asset_path.starts_with('#') {
                continue;
            }
            check_asset_reference(
                report,
                module,
                &stage.source,
                asset_path,
                "stage prop",
                false,
            );
        }
        if let Some(directory) = stage.directory.as_deref() {
            let wanted = format!("images/{directory}");
            if !module.assets.has_directory(&wanted) {
                report.add_at(
                    DiagnosticKind::MissingAsset,
                    stage.source.clone(),
                    format!(
                        "stage asset directory '{wanted}' is not present in this mod; it may \
                         be provided by the base game"
                    ),
                );
            }
        }
    }
}

/// Checks that a referenced image exists in the inventory.
///
/// Asset paths come from imported JSON, so each candidate is parsed as a
/// [`SafeRelativePath`] before it is compared against the inventory: a
/// traversing or absolute reference is reported and never resolved.
fn check_asset_reference(
    report: &mut ImportReport,
    module: &VsliceMod,
    owner: &str,
    asset: &str,
    label: &str,
    allow_directory: bool,
) {
    let base = asset.trim().trim_end_matches('/');
    let candidates = asset_candidates(base, allow_directory);

    let mut safe: Vec<String> = Vec::new();
    for candidate in candidates {
        if SafeRelativePath::parse(&candidate).is_err() {
            report.add_at(
                DiagnosticKind::UnsafePath,
                owner,
                format!("{label} reference '{asset}' is not a safe relative path"),
            );
            return;
        }
        safe.push(candidate);
    }

    match module.assets.lookup_any(&safe) {
        AssetLookup::Found(_) => {}
        AssetLookup::CaseMismatch { actual } => report.add_at(
            DiagnosticKind::CaseMismatch,
            owner,
            format!(
                "{label} '{asset}' resolves to '{}', which differs by casing",
                actual.path
            ),
        ),
        AssetLookup::Missing => report.add_at(
            DiagnosticKind::MissingAsset,
            owner,
            format!(
                "{label} '{asset}' was not found in this mod; it may be provided by the \
                 base game"
            ),
        ),
    }
}

fn asset_candidates(asset: &str, allow_directory: bool) -> Vec<String> {
    let mut candidates = vec![
        format!("images/{asset}.png"),
        format!("images/{asset}.xml"),
        format!("images/{asset}.json"),
        format!("images/{asset}/Animation.json"),
    ];
    if asset.starts_with("images/") {
        candidates.push(format!("{asset}.png"));
    }
    if allow_directory {
        candidates.push(format!("images/{asset}"));
    }
    candidates
}

fn version_parts(version: &str) -> Option<(u64, u64, u64)> {
    let cleaned = version.trim().trim_start_matches('v');
    let mut parts = cleaned.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .split(['-', '+'])
        .next()
        .unwrap_or("0")
        .parse::<u64>()
        .ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codename::model::AssetInventory;
    use crate::report::Severity;
    use crate::vslice::model::{
        Character, Chart, Note, PlayData, SongCharacters, SongMetadata, Stage, StageProp,
    };

    fn inventory(paths: &[&str]) -> AssetInventory {
        AssetInventory::build(paths.iter().map(|path| (path.to_string(), 1)))
    }

    fn song_with_chart(declared: &[&str], note_difficulties: &[&str]) -> Song {
        let mut chart = Chart {
            source: "songs/song/song-chart.json".to_string(),
            version: Some("2.0.0".to_string()),
            ..Chart::default()
        };
        for difficulty in note_difficulties {
            chart.notes.insert(
                difficulty.to_string(),
                vec![Note {
                    index: 0,
                    time_ms: Some(1000.0),
                    direction: Some(0.0),
                    sustain_ms: Some(0.0),
                    ..Note::default()
                }],
            );
        }
        Song {
            id: "song".to_string(),
            directory: "songs/song".to_string(),
            metadata: Some(SongMetadata {
                source: "songs/song/song-metadata.json".to_string(),
                version: Some("2.2.3".to_string()),
                time_format: Some("ms".to_string()),
                time_changes: vec![crate::vslice::model::TimeChange {
                    t: Some(0.0),
                    bpm: Some(180.0),
                    ..Default::default()
                }],
                play_data: Some(PlayData {
                    difficulties: declared.iter().map(|id| id.to_string()).collect(),
                    characters: Some(SongCharacters {
                        player: Some("bf".to_string()),
                        opponent: Some("dad".to_string()),
                        ..SongCharacters::default()
                    }),
                    stage: Some("stage".to_string()),
                    ..PlayData::default()
                }),
                ..SongMetadata::default()
            }),
            charts: vec![chart],
            ..Song::default()
        }
    }

    #[test]
    fn versions_are_parsed() {
        assert_eq!(version_parts("2.2.3"), Some((2, 2, 3)));
        assert_eq!(version_parts("v2.0.0-beta"), Some((2, 0, 0)));
        assert_eq!(version_parts("nope"), None);
    }

    #[test]
    fn missing_declared_difficulty_is_reported() {
        let module = VsliceMod {
            songs: vec![song_with_chart(&["easy", "hard"], &["easy"])],
            assets: inventory(&["songs/song/Inst.ogg"]),
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("missing-difficulty"));
    }

    #[test]
    fn undeclared_difficulty_is_informational() {
        let module = VsliceMod {
            songs: vec![song_with_chart(&["easy"], &["easy", "nightmare"])],
            assets: inventory(&["songs/song/Inst.ogg"]),
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("undeclared-difficulty"));
        let diagnostic = report.with_code("undeclared-difficulty").next().unwrap();
        assert_eq!(diagnostic.severity, Severity::Info);
    }

    #[test]
    fn invalid_notes_and_events_are_reported() {
        let mut song = song_with_chart(&["easy"], &["easy"]);
        let chart = &mut song.charts[0];
        chart.notes.insert(
            "easy".to_string(),
            vec![Note {
                index: 0,
                time_ms: Some(-5.0),
                direction: Some(12.0),
                sustain_ms: Some(-1.0),
                ..Note::default()
            }],
        );
        chart.events.push(crate::vslice::model::ChartEvent {
            raw: serde_json::json!({}),
            ..Default::default()
        });
        let module = VsliceMod {
            songs: vec![song],
            assets: inventory(&["songs/song/Inst.ogg"]),
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert_eq!(report.with_code("invalid-note").count(), 3);
        assert!(report.contains_code("invalid-event"));
    }

    #[test]
    fn missing_audio_and_references_are_warnings() {
        let module = VsliceMod {
            songs: vec![song_with_chart(&["easy"], &["easy"])],
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("missing-song-audio"));
        assert!(report.contains_code("missing-character-reference"));
        assert!(report.contains_code("missing-stage-reference"));
        assert!(!report.has_errors(), "{:?}", report);
    }

    #[test]
    fn character_and_stage_assets_are_checked() {
        let module = VsliceMod {
            characters: vec![Character {
                id: "bf".to_string(),
                source: "data/characters/bf.json".to_string(),
                asset_path: Some("characters/BOYFRIEND".to_string()),
                ..Character::default()
            }],
            stages: vec![Stage {
                id: "stage".to_string(),
                source: "stages/stage.json".to_string(),
                directory: Some("week1".to_string()),
                props: vec![StageProp {
                    asset_path: Some("week1/floor".to_string()),
                    ..StageProp::default()
                }],
                ..Stage::default()
            }],
            assets: inventory(&["images/characters/BOYFRIEND.png"]),
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("missing-asset"));
        let message = report
            .with_code("missing-asset")
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>()
            .join("|");
        assert!(message.contains("stage prop"));
        assert!(message.contains("week1"));
    }

    #[test]
    fn traversing_asset_references_are_never_resolved() {
        let module = VsliceMod {
            characters: vec![Character {
                id: "evil".to_string(),
                source: "data/characters/evil.json".to_string(),
                asset_path: Some("../../outside".to_string()),
                ..Character::default()
            }],
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("unsafe-path"));
        assert!(!report.contains_code("missing-asset"));
    }

    #[test]
    fn case_mismatched_assets_are_reported() {
        let module = VsliceMod {
            characters: vec![Character {
                id: "bf".to_string(),
                source: "data/characters/bf.json".to_string(),
                asset_path: Some("characters/boyfriend".to_string()),
                ..Character::default()
            }],
            assets: inventory(&["images/characters/BOYFRIEND.png"]),
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("case-mismatch"));
    }

    #[test]
    fn duplicate_definitions_are_reported() {
        let module = VsliceMod {
            stages: vec![
                Stage {
                    id: "stage".to_string(),
                    source: "stages/a.json".to_string(),
                    ..Stage::default()
                },
                Stage {
                    id: "Stage".to_string(),
                    source: "stages/b.json".to_string(),
                    ..Stage::default()
                },
            ],
            ..VsliceMod::default()
        };
        let report = validate(&module);
        assert!(report.contains_code("duplicate-definition"));
    }
}
