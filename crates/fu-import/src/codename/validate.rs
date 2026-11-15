//! Semantic validation of an imported Codename Engine mod.
//!
//! Validation is read-only and only uses what the parser already inventoried,
//! so it never opens a script, decodes an image or launches anything. Findings
//! that depend on content the importer cannot see — base-game characters,
//! built-in Codename events, engine-provided states — are classified as
//! [`SupportLevel::Unverified`] rather than reported as definite omissions.

use std::collections::{BTreeMap, BTreeSet};

use crate::codename::model::{
    AnchorRole, AssetEntry, AssetInventory, AssetLookup, AtlasKind, Chart, CodenameMod,
    StageAnchor, StageElement, StageSprite,
};
use crate::path::SafeRelativePath;
use crate::report::{Diagnostic, DiagnosticKind, ImportReport, Severity, SupportLevel};

/// Character slots a chart may address.
const KNOWN_POSITIONS: [&str; 5] = ["dad", "boyfriend", "girlfriend", "player", "opponent"];

/// Validates a parsed mod and returns a standalone report.
pub fn validate(module: &CodenameMod) -> ImportReport {
    let mut report = ImportReport::new();
    validate_into(module, &mut report);
    report.sort();
    report
}

/// Appends validation findings to an existing report.
pub fn validate_into(module: &CodenameMod, report: &mut ImportReport) {
    validate_inventory(module, report);
    validate_config(module, report);
    validate_characters(module, report);
    validate_stages(module, report);
    validate_songs(module, report);
    validate_event_definitions(module, report);
}

fn validate_inventory(module: &CodenameMod, report: &mut ImportReport) {
    let mut seen: BTreeMap<String, &AssetEntry> = BTreeMap::new();
    for entry in &module.assets.entries {
        let key = entry.path.to_ascii_lowercase();
        match seen.get(&key) {
            Some(existing) if existing.path != entry.path => report.push(
                Diagnostic::new(
                    DiagnosticKind::DuplicateCaseName,
                    format!(
                        "'{}' and '{}' differ only by casing, which is ambiguous on case-sensitive filesystems",
                        existing.path, entry.path
                    ),
                )
                .at(entry.path.clone()),
            ),
            Some(_) => {}
            None => {
                seen.insert(key, entry);
            }
        }
    }

    for atlas in &module.assets.atlases {
        let (location, label, kind) = match atlas.kind {
            AtlasKind::Sparrow => (
                atlas
                    .image
                    .clone()
                    .or_else(|| atlas.folder.clone())
                    .unwrap_or_default(),
                atlas.image.clone().unwrap_or_default(),
                "Sparrow atlas",
            ),
            AtlasKind::AnimateFolder => {
                let folder = atlas.folder.clone().unwrap_or_default();
                (folder.clone(), folder, "Animate texture atlas folder")
            }
        };
        report.push(
            Diagnostic::new(
                DiagnosticKind::PartialAtlas,
                format!("{kind} '{label}' recognized; frames are not decoded by the importer"),
            )
            .at(location),
        );
    }
}

fn validate_config(module: &CodenameMod, report: &mut ImportReport) {
    let config = &module.config;
    if config.is_empty() {
        return;
    }

    if let Some(api_version) = &config.api_version {
        if api_version.trim().parse::<u32>().is_err() {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                "data/config/modpack.ini",
                format!("API_VERSION is not a number: {api_version}"),
            );
        }
    }

    let mut by_state: BTreeMap<String, Vec<&crate::codename::model::StateRedirect>> =
        BTreeMap::new();
    for redirect in &config.state_redirects {
        by_state
            .entry(redirect.state.to_ascii_lowercase())
            .or_default()
            .push(redirect);
    }

    for (state, redirects) in by_state {
        let ordinary_targets: BTreeSet<&str> = redirects
            .iter()
            .filter(|redirect| !redirect.force)
            .map(|redirect| redirect.target.as_str())
            .collect();
        if ordinary_targets.len() > 1 {
            report.add_at(
                DiagnosticKind::ConflictingStateRedirect,
                redirects[0].source.clone(),
                format!(
                    "'{}' is redirected to several targets: {}",
                    redirects[0].state,
                    ordinary_targets
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }

        let forced_targets: BTreeSet<&str> = redirects
            .iter()
            .filter(|redirect| redirect.force)
            .map(|redirect| redirect.target.as_str())
            .collect();
        if forced_targets.len() > 1 {
            report.add_at(
                DiagnosticKind::ConflictingStateRedirect,
                redirects[0].source.clone(),
                format!(
                    "'{state}' is force redirected to several targets: {}",
                    forced_targets
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }

        if redirects.len() > 1 && ordinary_targets.len() <= 1 && forced_targets.len() <= 1 {
            report.add_at(
                DiagnosticKind::DuplicateStateRedirect,
                redirects[0].source.clone(),
                format!("'{state}' is redirected more than once to the same target"),
            );
        }
    }

    for redirect in &config.state_redirects {
        let target = redirect.target.trim();
        if target.is_empty() {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                redirect.source.clone(),
                format!("'{}' has an empty redirect target", redirect.state),
            );
            continue;
        }

        if target.contains(['/', '\\', ':']) || target.contains("..") {
            report.add_at(
                DiagnosticKind::UnsafePath,
                redirect.source.clone(),
                format!("redirect target '{target}' is not a plain state name"),
            );
            continue;
        }

        let candidates = state_candidates(target);
        match module.assets.lookup_any(&candidates) {
            AssetLookup::Found(_) => {}
            AssetLookup::CaseMismatch { actual } => report.push(
                Diagnostic::new(
                    DiagnosticKind::CaseMismatch,
                    format!(
                        "state '{target}' is stored as '{}' with different casing",
                        actual.path
                    ),
                )
                .at(actual.path.clone()),
            ),
            AssetLookup::Missing => report.push(
                Diagnostic::new(
                    DiagnosticKind::MissingStateReference,
                    format!(
                        "state '{target}' is not defined by this mod; a base game or another mod may provide it"
                    ),
                )
                .with_support(SupportLevel::Unverified)
                .at(redirect.source.clone()),
            ),
        }
    }
}

fn state_candidates(target: &str) -> Vec<String> {
    vec![
        format!("data/states/{target}.hx"),
        format!("data/states/{target}.lua"),
    ]
}

fn validate_characters(module: &CodenameMod, report: &mut ImportReport) {
    let mut seen_ids: BTreeMap<String, String> = BTreeMap::new();
    for character in &module.characters {
        let key = character.id.to_ascii_lowercase();
        if let Some(existing) = seen_ids.get(&key) {
            report.add_at(
                DiagnosticKind::DuplicateDefinition,
                character.source.clone(),
                format!(
                    "character id '{}' is also defined by '{existing}'",
                    character.id
                ),
            );
        } else {
            seen_ids.insert(key, character.source.clone());
        }

        match &character.sprite {
            Some(sprite) => {
                let candidates = sprite_candidates(sprite);
                check_reference(
                    &module.assets,
                    &candidates,
                    sprite,
                    &format!("character '{}'", character.id),
                    report,
                );
            }
            None => report.add_at(
                DiagnosticKind::InvalidAnimation,
                character.source.clone(),
                format!("character '{}' has no 'sprite' attribute", character.id),
            ),
        }

        if let Some(icon) = &character.icon {
            let candidates = icon_candidates(icon);
            check_reference(
                &module.assets,
                &candidates,
                icon,
                &format!("character '{}' icon", character.id),
                report,
            );
        }

        let mut seen_animations: BTreeMap<String, &str> = BTreeMap::new();
        for animation in &character.animations {
            if animation.animation.is_none() {
                report.add_at(
                    DiagnosticKind::InvalidAnimation,
                    character.source.clone(),
                    format!(
                        "animation '{}' of character '{}' has no 'anim' identifier",
                        animation.name, character.id
                    ),
                );
            }
            if animation.name.is_empty() {
                continue;
            }
            let key = animation.name.to_ascii_lowercase();
            if let Some(existing) = seen_animations.get(&key) {
                report.add_at(
                    DiagnosticKind::InvalidAnimation,
                    character.source.clone(),
                    format!(
                        "animation '{existing}' is defined more than once for character '{}'",
                        character.id
                    ),
                );
            } else {
                seen_animations.insert(key, animation.name.as_str());
            }
        }

        if !character.extra_elements.is_empty() {
            let names: Vec<_> = character
                .extra_elements
                .iter()
                .map(|element| element.name.as_str())
                .collect();
            report.add_at(
                DiagnosticKind::UnsupportedFeature,
                character.source.clone(),
                format!(
                    "unrecognized character elements preserved but not interpreted: {}",
                    names.join(", ")
                ),
            );
        }
    }
}

fn validate_stages(module: &CodenameMod, report: &mut ImportReport) {
    let mut seen_ids: BTreeMap<String, String> = BTreeMap::new();
    for stage in &module.stages {
        let key = stage.id.to_ascii_lowercase();
        if let Some(existing) = seen_ids.get(&key) {
            report.add_at(
                DiagnosticKind::DuplicateDefinition,
                stage.source.clone(),
                format!("stage id '{}' is also defined by '{existing}'", stage.id),
            );
        } else {
            seen_ids.insert(key, stage.source.clone());
        }

        let folder = stage.folder.as_deref().map(str::trim);
        match folder {
            Some(folder) if !folder.is_empty() => {
                let normalized = folder.trim_matches(['/', '\\']).replace('\\', "/");
                if !normalized.is_empty() && SafeRelativePath::parse(&normalized).is_err() {
                    report.add_at(
                        DiagnosticKind::UnsafePath,
                        stage.source.clone(),
                        format!("stage folder '{folder}' is not a safe relative path"),
                    );
                } else if !normalized.is_empty()
                    && !module.assets.has_directory(&format!("images/{normalized}"))
                {
                    report.add_at(
                        DiagnosticKind::MissingAsset,
                        stage.source.clone(),
                        format!("stage folder '{folder}' has no files under images/"),
                    );
                }
            }
            _ => report.add_at(
                DiagnosticKind::MissingMetadata,
                stage.source.clone(),
                format!("stage '{}' has no 'folder' attribute", stage.id),
            ),
        }

        let mut seen_sprites: BTreeMap<String, ()> = BTreeMap::new();
        let mut seen_roles: BTreeMap<&str, usize> = BTreeMap::new();
        for element in &stage.elements {
            match element {
                StageElement::Sprite(sprite) => {
                    validate_stage_sprite(
                        &module.assets,
                        stage,
                        folder,
                        sprite,
                        &mut seen_sprites,
                        report,
                    );
                }
                StageElement::Anchor(anchor) => {
                    validate_stage_anchor(stage, anchor, &mut seen_roles, report);
                }
            }
        }

        if !stage.extra_elements.is_empty() {
            let names: Vec<_> = stage
                .extra_elements
                .iter()
                .map(|element| element.name.as_str())
                .collect();
            report.add_at(
                DiagnosticKind::UnsupportedFeature,
                stage.source.clone(),
                format!(
                    "unrecognized stage elements preserved but not interpreted: {}",
                    names.join(", ")
                ),
            );
        }
    }
}

fn validate_stage_sprite(
    inventory: &AssetInventory,
    stage: &crate::codename::model::Stage,
    folder: Option<&str>,
    sprite: &StageSprite,
    seen: &mut BTreeMap<String, ()>,
    report: &mut ImportReport,
) {
    if let Some(name) = &sprite.name {
        let key = name.to_ascii_lowercase();
        if seen.insert(key, ()).is_some() {
            report.add_at(
                DiagnosticKind::DuplicateDefinition,
                stage.source.clone(),
                format!(
                    "stage '{}' defines sprite '{name}' more than once",
                    stage.id
                ),
            );
        }
    }

    match &sprite.sprite {
        Some(reference) => {
            let candidates = stage_sprite_candidates(folder, reference);
            check_reference(
                inventory,
                &candidates,
                reference,
                &format!("stage '{}'", stage.id),
                report,
            );
        }
        None => report.add_at(
            DiagnosticKind::MalformedMetadata,
            stage.source.clone(),
            format!(
                "stage '{}' has a <sprite> without a 'sprite' attribute",
                stage.id
            ),
        ),
    }
}

fn validate_stage_anchor(
    stage: &crate::codename::model::Stage,
    anchor: &StageAnchor,
    seen_roles: &mut BTreeMap<&'static str, usize>,
    report: &mut ImportReport,
) {
    let role: &'static str = match anchor.role {
        AnchorRole::Opponent => "opponent",
        AnchorRole::Player => "player",
        AnchorRole::Girlfriend => "girlfriend",
    };
    let count = seen_roles.entry(role).or_insert(0);
    *count += 1;
    if *count > 1 {
        report.add_at(
            DiagnosticKind::DuplicateDefinition,
            stage.source.clone(),
            format!(
                "stage '{}' has more than one {role} anchor (<{}>)",
                stage.id, anchor.tag
            ),
        );
    }
}

fn validate_songs(module: &CodenameMod, report: &mut ImportReport) {
    let mut undefined: BTreeSet<String> = BTreeSet::new();

    for song in &module.songs {
        let directory = &song.directory;

        if let Some(metadata) = &song.metadata {
            if let Some(bpm) = metadata.bpm {
                if !bpm.is_finite() || bpm <= 0.0 {
                    report.add_at(
                        DiagnosticKind::MalformedMetadata,
                        format!("{directory}/meta.json"),
                        format!("bpm must be a positive number, found {bpm}"),
                    );
                }
            }
            if metadata.difficulties.is_empty() {
                report.add_at(
                    DiagnosticKind::MalformedMetadata,
                    format!("{directory}/meta.json"),
                    "no difficulties are declared",
                );
            }
        }

        let declared: Vec<String> = song
            .metadata
            .as_ref()
            .map(|metadata| metadata.difficulties.clone())
            .unwrap_or_default();

        let mut seen_difficulties: BTreeMap<String, &str> = BTreeMap::new();
        for chart in &song.charts {
            let key = chart.difficulty.to_ascii_lowercase();
            if let Some(existing) = seen_difficulties.get(&key) {
                report.add_at(
                    DiagnosticKind::DuplicateChartDifficulty,
                    chart.source.clone(),
                    format!(
                        "difficulty '{}' is also provided by chart '{existing}'",
                        chart.difficulty
                    ),
                );
            } else {
                seen_difficulties.insert(key, chart.difficulty.as_str());
            }

            if !declared
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&chart.difficulty))
            {
                report.add_at(
                    DiagnosticKind::UnknownMetadata,
                    chart.source.clone(),
                    format!(
                        "difficulty '{}' is not listed in meta.json difficulties",
                        chart.difficulty
                    ),
                );
            }
        }

        for difficulty in &declared {
            if song.chart(difficulty).is_none() {
                report.add_at(
                    DiagnosticKind::MissingSongFile,
                    format!("{directory}/charts"),
                    format!("declared difficulty '{difficulty}' has no chart file"),
                );
            }
        }

        validate_song_audio(module, song, directory, report);

        for chart in &song.charts {
            validate_chart(module, chart, report, &mut undefined);
        }

        if let Some(events) = &song.events {
            for event in &events.events {
                check_event(
                    module,
                    event.name.as_deref(),
                    &events_source(directory),
                    report,
                    &mut undefined,
                );
            }
        }
    }
}

fn events_source(directory: &str) -> String {
    format!("{directory}/events.json")
}

fn validate_song_audio(
    module: &CodenameMod,
    song: &crate::codename::model::Song,
    directory: &str,
    report: &mut ImportReport,
) {
    let instrumental = audio_candidates(directory, "Inst");
    match module.assets.lookup_any(&instrumental) {
        AssetLookup::Found(_) => {}
        AssetLookup::CaseMismatch { actual } => report.push(
            Diagnostic::new(
                DiagnosticKind::CaseMismatch,
                "instrumental track is stored with different casing".to_string(),
            )
            .at(actual.path.clone()),
        ),
        AssetLookup::Missing => report.push(
            Diagnostic::new(
                DiagnosticKind::MissingSongFile,
                "instrumental track (Inst.ogg, Inst.mp3 or Inst.wav) is missing".to_string(),
            )
            .at(format!("{directory}/song")),
        ),
    }

    match song
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.needs_voices)
    {
        Some(true) => {
            let voices = audio_candidates(directory, "Voices");
            match module.assets.lookup_any(&voices) {
                AssetLookup::Found(_) => {}
                AssetLookup::CaseMismatch { actual } => report.push(
                    Diagnostic::new(
                        DiagnosticKind::CaseMismatch,
                        "voices track is stored with different casing".to_string(),
                    )
                    .at(actual.path.clone()),
                ),
                AssetLookup::Missing => report.push(
                    Diagnostic::new(
                        DiagnosticKind::MissingSongFile,
                        "meta.json requires voices but no Voices track exists (resolved suffix track names are not checked)".to_string(),
                    )
                    .with_severity(Severity::Warning)
                    .at(format!("{directory}/song")),
                ),
            }
        }
        Some(false) => {}
        None => report.push(
            Diagnostic::new(
                DiagnosticKind::UnknownMetadata,
                "metadata does not declare needsVoices; voices presence was not verified"
                    .to_string(),
            )
            .with_support(SupportLevel::Unverified)
            .at(format!("{directory}/meta.json")),
        ),
    }
}

fn audio_candidates(directory: &str, stem: &str) -> Vec<String> {
    ["ogg", "mp3", "wav"]
        .into_iter()
        .map(|extension| format!("{directory}/song/{stem}.{extension}"))
        .collect()
}

fn validate_chart(
    module: &CodenameMod,
    chart: &Chart,
    report: &mut ImportReport,
    undefined: &mut BTreeSet<String>,
) {
    if chart.format == crate::codename::model::ChartFormat::Codename
        && chart.codename_chart.is_none()
    {
        report.add_at(
            DiagnosticKind::UnknownMetadata,
            chart.source.clone(),
            "chart does not declare 'codenameChart'; the layout was inferred from 'strumLines'",
        );
    }

    if chart.strum_lines.is_empty() && chart.format == crate::codename::model::ChartFormat::Codename
    {
        report.add_at(
            DiagnosticKind::InvalidChart,
            chart.source.clone(),
            "chart has an empty 'strumLines' array",
        );
    }

    if let Some(stage) = &chart.stage {
        let stage_name = stage.trim_end_matches(['/', '\\']);
        let stage_name = stage_name.strip_suffix(".xml").unwrap_or(stage_name);
        if module.stage(stage_name).is_none() {
            report.push(
                Diagnostic::new(
                    DiagnosticKind::MissingStageReference,
                    format!(
                        "chart uses stage '{stage_name}', which this mod does not define; a base game or another mod may provide it"
                    ),
                )
                .with_support(SupportLevel::Unverified)
                .at(chart.source.clone()),
            );
        }
    }

    for event in &chart.events {
        check_event(
            module,
            event.name.as_deref(),
            &chart.source,
            report,
            undefined,
        );
    }

    let mut seen_lines: BTreeMap<(String, String), usize> = BTreeMap::new();
    for line in &chart.strum_lines {
        match line.key_count {
            Some(0) => report.add_at(
                DiagnosticKind::InvalidChart,
                chart.source.clone(),
                "strum line declares zero keys",
            ),
            None => report.push(
                Diagnostic::new(
                    DiagnosticKind::InvalidChart,
                    "strum line has no keyCount".to_string(),
                )
                .with_severity(Severity::Warning)
                .at(chart.source.clone()),
            ),
            Some(_) => {}
        }

        if line.characters.is_empty() {
            report.add_at(
                DiagnosticKind::MissingCharacterReference,
                chart.source.clone(),
                "strum line has no characters".to_string(),
            );
        }

        for character in &line.characters {
            if module.character(character).is_none() {
                report.push(
                    Diagnostic::new(
                        DiagnosticKind::MissingCharacterReference,
                        format!(
                            "chart uses character '{character}', which this mod does not define; a base game or another mod may provide it"
                        ),
                    )
                    .with_support(SupportLevel::Unverified)
                    .at(chart.source.clone()),
                );
            }
        }

        if let Some(position) = &line.position {
            if !KNOWN_POSITIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(position))
            {
                report.add_at(
                    DiagnosticKind::UnknownCharacterRole,
                    chart.source.clone(),
                    format!("unrecognized strum line position '{position}'"),
                );
            }
        }

        let key = (
            line.position
                .clone()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            line.characters.join(",").to_ascii_lowercase(),
        );
        if let Some(count) = seen_lines.get(&key) {
            report.add_at(
                DiagnosticKind::DuplicateDefinition,
                chart.source.clone(),
                format!("strum lines are duplicated for the same slot and characters ({count})"),
            );
        }
        *seen_lines.entry(key).or_insert(0) += 1;

        validate_notes(chart, line, report);
    }
}

fn validate_notes(
    chart: &Chart,
    line: &crate::codename::model::StrumLine,
    report: &mut ImportReport,
) {
    for note in &line.notes {
        if let Some(time) = note.time {
            if !time.is_finite() || time < 0.0 {
                report.push(
                    Diagnostic::new(
                        DiagnosticKind::InvalidChart,
                        format!("note time {time} is not a non-negative number"),
                    )
                    .with_severity(Severity::Warning)
                    .at(chart.source.clone()),
                );
            }
        }

        if let (Some(id), Some(key_count)) = (note.id, line.key_count) {
            if id >= key_count {
                report.push(
                    Diagnostic::new(
                        DiagnosticKind::InvalidChart,
                        format!("note lane {id} is outside the {key_count} declared keys"),
                    )
                    .at(chart.source.clone()),
                );
            }
        }

        if let Some(sustain) = note.sustain_length {
            if sustain < 0.0 {
                report.push(
                    Diagnostic::new(
                        DiagnosticKind::InvalidChart,
                        format!("note sustain length {sustain} is negative"),
                    )
                    .with_severity(Severity::Warning)
                    .at(chart.source.clone()),
                );
            }
        }
    }
}

fn check_event(
    module: &CodenameMod,
    name: Option<&str>,
    source: &str,
    report: &mut ImportReport,
    undefined: &mut BTreeSet<String>,
) {
    let Some(name) = name else {
        return;
    };
    if name.trim().is_empty() {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            source.to_string(),
            "event has an empty name",
        );
        return;
    }
    if module.defines_event(name) {
        return;
    }
    let key = name.to_ascii_lowercase();
    if !undefined.insert(key) {
        return;
    }
    report.push(
        Diagnostic::new(
            DiagnosticKind::UndefinedEvent,
            format!(
                "event '{name}' is not defined by this mod; it may be provided by Codename Engine or another mod"
            ),
        )
        .with_support(SupportLevel::Unverified)
        .at(source.to_string()),
    );
}

fn validate_event_definitions(module: &CodenameMod, report: &mut ImportReport) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for definition in &module.events {
        let key = definition.name.to_ascii_lowercase();
        if let Some(existing) = seen.get(&key) {
            report.add_at(
                DiagnosticKind::DuplicateDefinition,
                definition.source.clone(),
                format!(
                    "event '{}' is also defined by '{existing}'",
                    definition.name
                ),
            );
        } else {
            seen.insert(key, definition.source.clone());
        }

        if !definition.unknown_fields.is_empty() {
            report.add_at(
                DiagnosticKind::UnknownMetadata,
                definition.source.clone(),
                format!(
                    "event definition properties preserved without interpretation: {}",
                    definition
                        .unknown_fields
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }
}

fn sprite_candidates(sprite: &str) -> Vec<String> {
    let trimmed = sprite.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Vec::new();
    }
    let normalized = trimmed.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if lower.ends_with(".png") || lower.ends_with(".xml") || lower.ends_with(".json") {
        return vec![
            format!("images/{normalized}"),
            format!("images/characters/{normalized}"),
        ];
    }
    let base = if normalized.contains('/') {
        format!("images/{normalized}")
    } else {
        format!("images/characters/{normalized}")
    };
    vec![
        format!("{base}.png"),
        format!("{base}.xml"),
        format!("{base}/Animation.json"),
        format!("images/{normalized}.png"),
    ]
}

fn icon_candidates(icon: &str) -> Vec<String> {
    let trimmed = icon.trim().trim_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Vec::new();
    }
    vec![
        format!("images/icons/{trimmed}.png"),
        format!("images/icons/{trimmed}.xml"),
        format!("images/{trimmed}.png"),
    ]
}

fn stage_sprite_candidates(folder: Option<&str>, sprite: &str) -> Vec<String> {
    let normalized = sprite.trim().replace('\\', "/");
    if normalized.is_empty() {
        return Vec::new();
    }
    let base = match folder {
        Some(folder) if !folder.trim().is_empty() => {
            format!(
                "images/{}/{}",
                folder.trim_matches(['/', '\\']).replace('\\', "/"),
                normalized
            )
        }
        _ => format!("images/{normalized}"),
    };
    vec![
        format!("{base}.png"),
        format!("{base}.xml"),
        format!("{base}/Animation.json"),
    ]
}

fn check_reference(
    inventory: &AssetInventory,
    candidates: &[String],
    reference: &str,
    owner: &str,
    report: &mut ImportReport,
) {
    if candidates.is_empty() {
        report.push(
            Diagnostic::new(
                DiagnosticKind::MissingAsset,
                format!("{owner} has an empty asset reference"),
            )
            .with_support(SupportLevel::Partial),
        );
        return;
    }

    match inventory.lookup_any(candidates) {
        AssetLookup::Found(_) => {}
        AssetLookup::CaseMismatch { actual } => report.push(
            Diagnostic::new(
                DiagnosticKind::CaseMismatch,
                format!(
                    "'{reference}' referenced by {owner} is stored as '{}' with different casing",
                    actual.path
                ),
            )
            .at(actual.path.clone()),
        ),
        AssetLookup::Missing => report.push(
            Diagnostic::new(
                DiagnosticKind::MissingAsset,
                format!(
                    "'{reference}' referenced by {owner} was not found (tried {})",
                    candidates.join(", ")
                ),
            )
            .at(candidates[0].clone()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codename::model::{
        AssetKind, Character, CharacterAnimation, ChartEvent, ChartFormat, Note, Song,
        SongMetadata, Stage, StateRedirect, StrumLine,
    };

    fn inventory(files: &[&str]) -> AssetInventory {
        AssetInventory::build(files.iter().map(|path| (path.to_string(), 1u64)))
    }

    fn character(id: &str, sprite: &str) -> Character {
        Character {
            id: id.to_string(),
            source: format!("data/characters/{id}.xml"),
            sprite: Some(sprite.to_string()),
            ..Character::default()
        }
    }

    #[test]
    fn reports_missing_and_miscased_character_sprites() {
        let mut module = CodenameMod {
            assets: inventory(&[
                "images/characters/Var.png",
                "images/characters/Var.xml",
                "images/characters/newBF/Animation.json",
            ]),
            ..CodenameMod::default()
        };
        module.characters.push(character("var", "Var"));
        module.characters.push(character("bf", "newBF"));
        module.characters.push(character("gone", "MissingSprite"));
        module.characters.push(character("cased", "var"));

        let report = validate(&module);
        let missing: Vec<_> = report.with_code("missing-asset").collect();
        assert_eq!(missing.len(), 1, "{:?}", report);
        assert!(missing[0].message.contains("MissingSprite"));
        assert_eq!(report.with_code("case-mismatch").count(), 1);
    }

    #[test]
    fn reports_character_animation_problems() {
        let mut module = CodenameMod {
            assets: inventory(&["images/characters/Var.png"]),
            ..CodenameMod::default()
        };
        let mut entry = character("var", "Var");
        entry.animations = vec![
            CharacterAnimation {
                name: "idle".to_string(),
                animation: Some("Idle".to_string()),
                ..CharacterAnimation::default()
            },
            CharacterAnimation {
                name: "IDLE".to_string(),
                animation: Some("Idle2".to_string()),
                ..CharacterAnimation::default()
            },
            CharacterAnimation {
                name: "singUP".to_string(),
                animation: None,
                ..CharacterAnimation::default()
            },
        ];
        module.characters.push(entry);

        let report = validate(&module);
        let animation_issues: Vec<_> = report.with_code("invalid-animation").collect();
        assert_eq!(animation_issues.len(), 2, "{:?}", report);
    }

    #[test]
    fn reports_stage_folder_and_sprite_references() {
        let mut module = CodenameMod {
            assets: inventory(&["images/stages/castle/floor.png"]),
            ..CodenameMod::default()
        };
        module.stages.push(Stage {
            id: "castle".to_string(),
            source: "data/stages/castle.xml".to_string(),
            folder: Some("stages/castle/".to_string()),
            elements: vec![
                StageElement::Sprite(StageSprite {
                    name: Some("floor".to_string()),
                    sprite: Some("floor".to_string()),
                    ..StageSprite::default()
                }),
                StageElement::Sprite(StageSprite {
                    name: Some("roof".to_string()),
                    sprite: Some("roof".to_string()),
                    ..StageSprite::default()
                }),
                StageElement::Anchor(StageAnchor {
                    role: AnchorRole::Girlfriend,
                    tag: "gf".to_string(),
                    ..StageAnchor::default()
                }),
                StageElement::Anchor(StageAnchor {
                    role: AnchorRole::Girlfriend,
                    tag: "girlfriend".to_string(),
                    ..StageAnchor::default()
                }),
            ],
            ..Stage::default()
        });
        module.stages.push(Stage {
            id: "empty".to_string(),
            source: "data/stages/empty.xml".to_string(),
            folder: None,
            ..Stage::default()
        });

        let report = validate(&module);
        assert_eq!(report.with_code("missing-asset").count(), 1);
        assert_eq!(report.with_code("missing-metadata").count(), 1);
        assert_eq!(report.with_code("duplicate-definition").count(), 1);
    }

    #[test]
    fn reports_conflicting_and_duplicate_state_redirects() {
        let mut module = CodenameMod {
            assets: inventory(&["data/states/newMenu.hx"]),
            ..CodenameMod::default()
        };
        module.config.state_redirects = vec![
            StateRedirect {
                state: "TitleState".to_string(),
                target: "newTitle".to_string(),
                force: false,
                source: "data/config/modpack.ini".to_string(),
            },
            StateRedirect {
                state: "TitleState".to_string(),
                target: "otherTitle".to_string(),
                force: false,
                source: "data/config/flags.ini".to_string(),
            },
            StateRedirect {
                state: "MainMenuState".to_string(),
                target: "newMenu".to_string(),
                force: false,
                source: "data/config/modpack.ini".to_string(),
            },
            StateRedirect {
                state: "MainMenuState".to_string(),
                target: "newMenu".to_string(),
                force: false,
                source: "data/config/flags.ini".to_string(),
            },
        ];

        let report = validate(&module);
        assert_eq!(report.with_code("conflicting-state-redirect").count(), 1);
        assert_eq!(report.with_code("duplicate-state-redirect").count(), 1);
        assert_eq!(
            report.with_code("missing-state-reference").count(),
            2,
            "newTitle and otherTitle are not provided by the mod"
        );
    }

    fn song_with(chart: Chart, audio: &[&str]) -> (Song, AssetInventory) {
        let mut module_assets = vec![
            "images/characters/var.png".to_string(),
            "images/characters/newbf.png".to_string(),
        ];
        module_assets.extend(audio.iter().map(|path| path.to_string()));
        let song = Song {
            name: "greetings".to_string(),
            directory: "songs/greetings".to_string(),
            metadata: Some(SongMetadata {
                bpm: Some(100.0),
                difficulties: vec!["hard".to_string()],
                needs_voices: Some(true),
                ..SongMetadata::default()
            }),
            charts: vec![chart],
            ..Song::default()
        };
        (
            song,
            AssetInventory::build(module_assets.into_iter().map(|p| (p, 1u64))),
        )
    }

    fn base_chart() -> Chart {
        Chart {
            source: "songs/greetings/charts/hard.json".to_string(),
            difficulty: "hard".to_string(),
            format: ChartFormat::Codename,
            codename_chart: Some(true),
            stage: Some("castle".to_string()),
            strum_lines: vec![StrumLine {
                key_count: Some(4),
                position: Some("dad".to_string()),
                characters: vec!["var".to_string()],
                notes: vec![Note {
                    time: Some(0.0),
                    id: Some(0),
                    ..Note::default()
                }],
                ..StrumLine::default()
            }],
            ..Chart::default()
        }
    }

    #[test]
    fn reports_missing_song_files_and_references() {
        let (song, assets) = song_with(
            base_chart(),
            &[
                "songs/greetings/song/Inst.ogg",
                "songs/greetings/song/Voices.ogg",
            ],
        );
        let mut module = CodenameMod {
            assets,
            ..CodenameMod::default()
        };
        module.songs.push(song);
        module.characters.push(character("var", "var"));
        let report = validate(&module);
        assert!(!report.has_errors(), "{report}");
        assert_eq!(report.with_code("missing-stage-reference").count(), 1);
        assert_eq!(report.with_code("missing-asset").count(), 0);
    }

    #[test]
    fn reports_missing_instrumental_and_voices() {
        let (song, assets) = song_with(base_chart(), &[]);
        let mut module = CodenameMod {
            assets,
            ..CodenameMod::default()
        };
        module.songs.push(song);
        let report = validate(&module);
        let missing: Vec<_> = report.with_code("missing-song-file").collect();
        assert_eq!(missing.len(), 2, "{report}");
        assert!(missing
            .iter()
            .any(|entry| entry.message.contains("instrumental")));
        assert!(report.has_errors());
    }

    #[test]
    fn reports_invalid_notes_and_unknown_characters() {
        let (mut song, assets) = song_with(
            base_chart(),
            &[
                "songs/greetings/song/Inst.ogg",
                "songs/greetings/song/Voices.ogg",
            ],
        );
        song.charts[0].strum_lines[0].characters = vec!["unknown".to_string()];
        song.charts[0].strum_lines[0].notes = vec![
            Note {
                time: Some(-5.0),
                id: Some(9),
                sustain_length: Some(-1.0),
                ..Note::default()
            },
            Note {
                time: None,
                id: None,
                ..Note::default()
            },
        ];
        let mut module = CodenameMod {
            assets,
            ..CodenameMod::default()
        };
        module.songs.push(song);
        let report = validate(&module);
        assert_eq!(report.with_code("missing-character-reference").count(), 1);
        assert!(report.with_code("invalid-chart").count() >= 3, "{report}");
    }

    #[test]
    fn reports_undefined_events_once() {
        let (mut song, assets) = song_with(
            base_chart(),
            &[
                "songs/greetings/song/Inst.ogg",
                "songs/greetings/song/Voices.ogg",
            ],
        );
        let event = ChartEvent {
            time: Some(0.0),
            name: Some("Mystery Event".to_string()),
            ..ChartEvent::default()
        };
        song.charts[0].events = vec![event.clone()];
        song.events = Some(crate::codename::model::SongEvents {
            events: vec![event],
            ..crate::codename::model::SongEvents::default()
        });
        let mut module = CodenameMod {
            assets,
            ..CodenameMod::default()
        };
        module.songs.push(song);
        let report = validate(&module);
        let undefined: Vec<_> = report.with_code("undefined-event").collect();
        assert_eq!(undefined.len(), 1, "reported once per event name");
        assert_eq!(undefined[0].support, SupportLevel::Unverified);
        assert_eq!(undefined[0].severity, Severity::Warning);
    }

    #[test]
    fn defined_events_are_accepted() {
        let (mut song, assets) = song_with(
            base_chart(),
            &[
                "songs/greetings/song/Inst.ogg",
                "songs/greetings/song/Voices.ogg",
            ],
        );
        song.charts[0].events = vec![ChartEvent {
            time: Some(0.0),
            name: Some("Camera Bump".to_string()),
            ..ChartEvent::default()
        }];
        let mut module = CodenameMod {
            assets,
            ..CodenameMod::default()
        };
        module.songs.push(song);
        module.events.push(crate::codename::model::EventDefinition {
            name: "Camera Bump".to_string(),
            source: "data/events/Camera Bump.json".to_string(),
            ..crate::codename::model::EventDefinition::default()
        });
        let report = validate(&module);
        assert_eq!(report.with_code("undefined-event").count(), 0);
    }

    #[test]
    fn reports_duplicate_casing_in_inventory() {
        let mut module = CodenameMod {
            assets: inventory(&["images/icons/var.png", "images/icons/Var.png"]),
            ..CodenameMod::default()
        };
        module.characters.push(character("var", "var"));
        let report = validate(&module);
        assert_eq!(report.with_code("duplicate-case-name").count(), 1);
    }

    #[test]
    fn every_asset_kind_is_recognized() {
        assert_eq!(
            AssetKind::classify("a.hx"),
            AssetKind::Script(crate::codename::model::ScriptLanguage::Haxe)
        );
        assert_eq!(AssetKind::classify("a.png"), AssetKind::Image);
        assert_eq!(AssetKind::classify("a.unknown"), AssetKind::Other);
    }
}
