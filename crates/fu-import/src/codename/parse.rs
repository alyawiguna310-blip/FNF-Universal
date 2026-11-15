//! Parsing of a discovered mod directory into the Codename model.
//!
//! Every read goes through [`crate::path::read_text_within`], so the parser
//! cannot escape the mod root, follow a symlink out of it, or read an oversized
//! file. Scripts are inventoried by extension only: their contents are never
//! read, compiled or executed.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::codename::discovery::{scan_mod_tree, DiscoveredMod, ModTree};
use crate::codename::model::{
    attribute_ci, attribute_f64, AnchorRole, AssetEntry, AssetInventory, AssetKind, Attribute,
    Character, CharacterAnimation, Chart, ChartEvent, ChartFormat, CodenameMod, ConfigSource,
    EventDefinition, EventParameter, ModpackConfig, Note, NoteTypeDefinition, RawElement,
    ScriptAsset, Song, SongEvents, SongMetadata, Stage, StageAnchor, StageElement, StageSprite,
    StateRedirect, StrumLine, StrumPos, UnsupportedFile, UnsupportedReason,
};
use crate::ini::IniDocument;
use crate::path::{self, PathError, SafeRelativePath};
use crate::report::{Diagnostic, DiagnosticKind, ImportReport, Severity};
use crate::xml::XmlDocument;

/// Directory holding Codename configuration files.
pub const CONFIG_DIRECTORY: &str = "data/config";
/// Main mod metadata file.
pub const MODPACK_INI: &str = "data/config/modpack.ini";
/// Secondary configuration file, merged after `modpack.ini`.
pub const FLAGS_INI: &str = "data/config/flags.ini";
/// Character definitions.
pub const CHARACTERS_DIRECTORY: &str = "data/characters";
/// Stage definitions.
pub const STAGES_DIRECTORY: &str = "data/stages";
/// Event definitions and event scripts.
pub const EVENTS_DIRECTORY: &str = "data/events";
/// State scripts referenced by state redirects.
pub const STATES_DIRECTORY: &str = "data/states";
/// Song directories.
pub const SONGS_DIRECTORY: &str = "songs";

/// Limits applied while parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Maximum directory depth scanned inside the mod.
    pub max_depth: usize,
    /// Maximum size of a single text file, in bytes.
    pub max_file_bytes: u64,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_depth: crate::codename::discovery::DEFAULT_MAX_DEPTH,
            max_file_bytes: crate::path::MAX_IMPORT_FILE_BYTES,
        }
    }
}

/// A parsed mod together with everything that was noticed on the way.
#[derive(Debug, Clone)]
pub struct ParsedMod {
    /// The imported mod.
    pub module: CodenameMod,
    /// Findings from scanning and parsing. Validation adds to this report.
    pub report: ImportReport,
}

/// Parses a discovered mod directory with default limits.
pub fn parse_mod(discovered: &DiscoveredMod) -> ParsedMod {
    parse_mod_with(discovered, ParseOptions::default())
}

/// Parses a discovered mod directory.
pub fn parse_mod_with(discovered: &DiscoveredMod, options: ParseOptions) -> ParsedMod {
    let tree = scan_mod_tree(&discovered.root, options.max_depth);
    let mut report = ImportReport::with_target(discovered.name.clone());
    report.extend(tree.report.clone());

    let assets = AssetInventory::build(
        tree.files
            .iter()
            .map(|file| (file.relative.to_string(), file.size)),
    );

    let module = CodenameMod {
        name: discovered.name.clone(),
        root: discovered.root.to_string_lossy().to_string(),
        assets,
        ..CodenameMod::default()
    };

    let mut parser = Parser {
        root: discovered.root.clone(),
        tree,
        report,
        module,
        options,
    };
    parser.run();
    ParsedMod {
        module: parser.module,
        report: parser.report,
    }
}

struct Parser {
    root: std::path::PathBuf,
    tree: ModTree,
    report: ImportReport,
    module: CodenameMod,
    options: ParseOptions,
}

impl Parser {
    fn run(&mut self) {
        self.collect_scripts_and_binaries();
        self.parse_config();
        self.parse_characters();
        self.parse_stages();
        self.parse_events();
        self.parse_songs();
        self.report.sort();
    }

    /// Resolves `wanted` to an inventoried path, reporting case mismatches.
    fn resolve_file(&mut self, wanted: &str) -> Option<String> {
        if let Some(file) = self.tree.file(wanted) {
            return Some(file.relative.to_string());
        }
        if let Some(file) = self.tree.file_case_insensitive(wanted) {
            let actual = file.relative.to_string();
            self.report.push(
                Diagnostic::new(
                    DiagnosticKind::CaseMismatch,
                    format!("file is named '{actual}' but the importer expected '{wanted}'"),
                )
                .at(actual.clone()),
            );
            return Some(actual);
        }
        None
    }

    fn read_text(&mut self, relative: &str) -> Option<String> {
        let parsed = match SafeRelativePath::parse(relative) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.report
                    .add_at(DiagnosticKind::UnsafePath, relative, error.to_string());
                return None;
            }
        };
        match path::read_text_within(&self.root, &parsed, self.options.max_file_bytes) {
            Ok(text) => Some(text),
            Err(error) => {
                let kind = match &error {
                    PathError::TooLarge { .. } | PathError::NotUtf8(_) => {
                        DiagnosticKind::MalformedMetadata
                    }
                    PathError::Io(_) => DiagnosticKind::ScanIoError,
                    _ => DiagnosticKind::UnsafePath,
                };
                self.report.add_at(kind, relative, error.to_string());
                None
            }
        }
    }

    fn collect_scripts_and_binaries(&mut self) {
        let entries = self.module.assets.entries.clone();
        let mut script_count = 0usize;
        for entry in entries {
            match &entry.kind {
                AssetKind::Script(language) => {
                    script_count += 1;
                    self.module.scripts.push(ScriptAsset {
                        in_event_directory: entry.path.starts_with(&format!("{EVENTS_DIRECTORY}/")),
                        path: entry.path,
                        language: language.clone(),
                        size: entry.size,
                    });
                }
                AssetKind::Binary => {
                    let reason = classify_binary(&entry.path);
                    self.report.push(
                        Diagnostic::new(
                            DiagnosticKind::UnsupportedBinary,
                            format!(
                                "{} was recorded and will never be loaded",
                                describe(&reason)
                            ),
                        )
                        .at(entry.path.clone()),
                    );
                    self.module.unsupported_files.push(UnsupportedFile {
                        path: entry.path,
                        reason,
                        size: entry.size,
                    });
                }
                AssetKind::Archive => {
                    self.report.push(
                        Diagnostic::new(
                            DiagnosticKind::UnsupportedArchive,
                            "archive is recorded but is not expanded",
                        )
                        .at(entry.path.clone()),
                    );
                    self.module.unsupported_files.push(UnsupportedFile {
                        path: entry.path,
                        reason: UnsupportedReason::Archive,
                        size: entry.size,
                    });
                }
                _ => {}
            }
        }

        if script_count > 0 {
            self.report.push(Diagnostic::new(
                DiagnosticKind::UnsupportedScript,
                format!(
                    "{script_count} script file(s) were inventoried as text metadata only; nothing is compiled or executed"
                ),
            ));
        }
    }

    fn parse_config(&mut self) {
        let mut config = ModpackConfig::default();

        for wanted in [MODPACK_INI, FLAGS_INI] {
            let Some(actual) = self.resolve_file(wanted) else {
                if wanted == MODPACK_INI {
                    self.report.add_at(
                        DiagnosticKind::MissingMetadata,
                        wanted,
                        "modpack.ini is missing; mod metadata and state redirects are unavailable",
                    );
                }
                continue;
            };
            let Some(text) = self.read_text(&actual) else {
                continue;
            };
            let document = IniDocument::parse(&text);
            for issue in &document.issues {
                self.report.add_at(
                    DiagnosticKind::MalformedIni,
                    actual.clone(),
                    format!("line {}: {}", issue.line, issue.message),
                );
            }
            interpret_config(&document, &actual, &mut config, &mut self.report);
            config.sources.push(ConfigSource {
                path: actual,
                document,
            });
        }

        if config.sources.is_empty() {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                CONFIG_DIRECTORY,
                "no configuration files were found",
            );
        }

        self.module.config = config;
    }

    fn parse_characters(&mut self) {
        let mut paths: Vec<String> = self
            .tree
            .files
            .iter()
            .filter(|file| {
                file.relative.parent() == Some(CHARACTERS_DIRECTORY)
                    && file.relative.extension().as_deref() == Some("xml")
            })
            .map(|file| file.relative.to_string())
            .collect();
        paths.sort();

        if paths.is_empty() {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                CHARACTERS_DIRECTORY,
                "no character definitions were found",
            );
            return;
        }

        for path in paths {
            let Some(text) = self.read_text(&path) else {
                continue;
            };
            let document = XmlDocument::parse(&text);
            let character = parse_character_document(&path, document, &mut self.report);
            self.module.characters.push(character);
        }
    }

    fn parse_stages(&mut self) {
        let mut paths: Vec<String> = self
            .tree
            .files
            .iter()
            .filter(|file| {
                file.relative.parent() == Some(STAGES_DIRECTORY)
                    && file.relative.extension().as_deref() == Some("xml")
            })
            .map(|file| file.relative.to_string())
            .collect();
        paths.sort();

        if paths.is_empty() {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                STAGES_DIRECTORY,
                "no stage definitions were found",
            );
            return;
        }

        for path in paths {
            let Some(text) = self.read_text(&path) else {
                continue;
            };
            let document = XmlDocument::parse(&text);
            let stage = parse_stage_document(&path, document, &mut self.report);
            self.module.stages.push(stage);
        }
    }

    fn parse_events(&mut self) {
        let mut paths: Vec<String> = self
            .tree
            .files
            .iter()
            .filter(|file| {
                file.relative.parent() == Some(EVENTS_DIRECTORY)
                    && file.relative.extension().as_deref() == Some("json")
            })
            .map(|file| file.relative.to_string())
            .collect();
        paths.sort();

        for path in paths {
            let Some(text) = self.read_text(&path) else {
                continue;
            };
            let value = match serde_json::from_str::<Value>(&text) {
                Ok(value) => value,
                Err(error) => {
                    self.report.add_at(
                        DiagnosticKind::MalformedJson,
                        path.clone(),
                        format!("line {}: {error}", error.line()),
                    );
                    continue;
                }
            };
            let definition = parse_event_definition(&path, value, &mut self.report);
            self.module.events.push(definition);
        }
    }

    fn parse_songs(&mut self) {
        let mut directories: Vec<String> = self
            .tree
            .directories
            .iter()
            .filter(|directory| directory.starts_with(&format!("{SONGS_DIRECTORY}/")))
            .filter(|directory| directory.matches('/').count() == 1)
            .cloned()
            .collect();
        directories.sort();

        if directories.is_empty() {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                SONGS_DIRECTORY,
                "no song directories were found",
            );
            return;
        }

        for directory in directories {
            let song = self.parse_song(&directory);
            self.module.songs.push(song);
        }
    }

    fn parse_song(&mut self, directory: &str) -> Song {
        let name = directory
            .rsplit('/')
            .next()
            .unwrap_or(directory)
            .to_string();
        let files: Vec<AssetEntry> = self.module.assets.files_under(directory).cloned().collect();

        let mut song = Song {
            name,
            directory: directory.to_string(),
            ..Song::default()
        };

        if let Some(meta_path) = self.resolve_file(&format!("{directory}/meta.json")) {
            if let Some(text) = self.read_text(&meta_path) {
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) => {
                        song.metadata =
                            Some(parse_song_metadata(&meta_path, value, &mut self.report));
                    }
                    Err(error) => self.report.add_at(
                        DiagnosticKind::MalformedJson,
                        meta_path,
                        format!("line {}: {error}", error.line()),
                    ),
                }
            }
        } else {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                format!("{directory}/meta.json"),
                "song metadata is missing",
            );
        }

        if let Some(events_path) = self.resolve_file(&format!("{directory}/events.json")) {
            if let Some(text) = self.read_text(&events_path) {
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) => {
                        song.events =
                            Some(parse_song_events(&events_path, value, &mut self.report));
                    }
                    Err(error) => self.report.add_at(
                        DiagnosticKind::MalformedJson,
                        events_path,
                        format!("line {}: {error}", error.line()),
                    ),
                }
            }
        }

        let mut charts: Vec<String> = self
            .tree
            .files
            .iter()
            .filter(|file| {
                file.relative.parent() == Some(&format!("{directory}/charts"))
                    && file.relative.extension().as_deref() == Some("json")
            })
            .map(|file| file.relative.to_string())
            .collect();
        charts.sort();

        if charts.is_empty() {
            self.report.add_at(
                DiagnosticKind::InvalidChart,
                format!("{directory}/charts"),
                "song has no chart files",
            );
        }

        for chart_path in charts {
            let Some(text) = self.read_text(&chart_path) else {
                continue;
            };
            let difficulty = chart_difficulty(&chart_path);
            let value = match serde_json::from_str::<Value>(&text) {
                Ok(value) => value,
                Err(error) => {
                    self.report.add_at(
                        DiagnosticKind::MalformedJson,
                        chart_path,
                        format!("line {}: {error}", error.line()),
                    );
                    continue;
                }
            };
            let chart = parse_chart_document(&chart_path, &difficulty, value, &mut self.report);
            song.charts.push(chart);
        }

        for file in files {
            let audio_prefix = format!("{directory}/song/");
            if file.path.starts_with(&audio_prefix) && file.kind == AssetKind::Audio {
                song.audio.push(file);
                continue;
            }
            if file.path.starts_with(&format!("{directory}/scripts/"))
                && matches!(file.kind, AssetKind::Script(_))
            {
                if let AssetKind::Script(language) = &file.kind {
                    song.scripts.push(ScriptAsset {
                        path: file.path.clone(),
                        language: language.clone(),
                        size: file.size,
                        in_event_directory: false,
                    });
                }
                continue;
            }
            if file.path.ends_with("/meta.json")
                || file.path.ends_with("/events.json")
                || file.path.starts_with(&format!("{directory}/charts/"))
            {
                continue;
            }
            song.other_files.push(file);
        }

        song
    }
}

fn classify_binary(path: &str) -> UnsupportedReason {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "dll" | "ndll" | "so" | "dylib" => UnsupportedReason::NativeLibrary,
        "exe" | "msi" | "bat" | "cmd" | "sh" => UnsupportedReason::Executable,
        _ => UnsupportedReason::Unknown,
    }
}

fn describe(reason: &UnsupportedReason) -> &'static str {
    match reason {
        UnsupportedReason::NativeLibrary => "a native library",
        UnsupportedReason::Executable => "an executable",
        UnsupportedReason::Archive => "an archive",
        UnsupportedReason::Unknown => "an opaque binary",
    }
}

/// Interprets known sections of a configuration document, preserving the rest.
fn interpret_config(
    document: &IniDocument,
    source: &str,
    config: &mut ModpackConfig,
    report: &mut ImportReport,
) {
    for section in &document.sections {
        let name = section.name.as_str();
        if name.eq_ignore_ascii_case("Common") {
            for entry in &section.entries {
                if entry.key.eq_ignore_ascii_case("API_VERSION") {
                    if config.api_version.is_none() {
                        config.api_version = Some(entry.value.clone());
                    }
                } else {
                    report.push(
                        Diagnostic::new(
                            DiagnosticKind::UnknownMetadata,
                            format!("[Common] {} is preserved but not interpreted", entry.key),
                        )
                        .at(source.to_string()),
                    );
                }
            }
        } else if name.eq_ignore_ascii_case("Flags") {
            for entry in &section.entries {
                config.flags.insert(entry.key.clone(), entry.value.clone());
            }
        } else if name.eq_ignore_ascii_case("Discord") {
            for entry in &section.entries {
                config
                    .discord
                    .insert(entry.key.clone(), entry.value.clone());
            }
        } else if name.eq_ignore_ascii_case("StateRedirects")
            || name.eq_ignore_ascii_case("StateRedirects.force")
        {
            let force = name.eq_ignore_ascii_case("StateRedirects.force");
            for entry in &section.entries {
                config.state_redirects.push(StateRedirect {
                    state: entry.key.clone(),
                    target: entry.value.clone(),
                    force,
                    source: source.to_string(),
                });
            }
        } else {
            if !config.unknown_sections.iter().any(|known| known == name) {
                config.unknown_sections.push(name.to_string());
            }
            report.push(
                Diagnostic::new(
                    DiagnosticKind::UnknownMetadata,
                    format!("section [{name}] is preserved but not interpreted"),
                )
                .at(source.to_string()),
            );
        }
    }
}

fn chart_difficulty(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index > 0 => name[..index].to_string(),
        _ => name.to_string(),
    }
}

fn raw_element(element: &crate::xml::XmlElement) -> RawElement {
    RawElement {
        name: element.name.clone(),
        attributes: element
            .attributes
            .iter()
            .map(|attribute| Attribute {
                name: attribute.name.clone(),
                value: attribute.value.clone(),
            })
            .collect(),
        text: element.trimmed_text().to_string(),
    }
}

fn attributes_of(element: &crate::xml::XmlElement) -> Vec<Attribute> {
    element
        .attributes
        .iter()
        .map(|attribute| Attribute {
            name: attribute.name.clone(),
            value: attribute.value.clone(),
        })
        .collect()
}

fn is_tag(element: &crate::xml::XmlElement, name: &str) -> bool {
    element.name.eq_ignore_ascii_case(name)
}

/// Parses one `data/characters/*.xml` document.
pub(crate) fn parse_character_document(
    path: &str,
    document: XmlDocument,
    report: &mut ImportReport,
) -> Character {
    for issue in &document.issues {
        report.add_at(
            DiagnosticKind::MalformedXml,
            path.to_string(),
            format!("line {}: {}", issue.line, issue.message),
        );
    }

    let mut character = Character {
        id: SafeRelativePath::parse(path)
            .map(|parsed| parsed.file_stem().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        source: path.to_string(),
        ..Character::default()
    };

    let Some(root) = document.root else {
        report.add_at(
            DiagnosticKind::MalformedXml,
            path.to_string(),
            "document has no root element",
        );
        return character;
    };

    if !is_tag(&root, "character") {
        report.add_at(
            DiagnosticKind::MalformedXml,
            path.to_string(),
            format!("expected a <character> root, found <{}>", root.name),
        );
    }

    character.attributes = attributes_of(&root);
    character.sprite = attribute_ci(&character.attributes, "sprite").map(str::to_string);
    character.icon = attribute_ci(&character.attributes, "icon").map(str::to_string);

    for child in &root.children {
        if is_tag(child, "anim") {
            let attributes = attributes_of(child);
            let name = attribute_ci(&attributes, "name")
                .unwrap_or_default()
                .to_string();
            if name.is_empty() {
                report.add_at(
                    DiagnosticKind::InvalidAnimation,
                    path.to_string(),
                    "an <anim> element has no name",
                );
            }
            character.animations.push(CharacterAnimation {
                name,
                animation: attribute_ci(&attributes, "anim").map(str::to_string),
                label: attribute_ci(&attributes, "label").map(str::to_string),
                x: attribute_f64(&attributes, "x"),
                y: attribute_f64(&attributes, "y"),
                attributes,
            });
        } else {
            character.extra_elements.push(raw_element(child));
        }
    }

    character
}

/// Parses one `data/stages/*.xml` document.
pub(crate) fn parse_stage_document(
    path: &str,
    document: XmlDocument,
    report: &mut ImportReport,
) -> Stage {
    for issue in &document.issues {
        report.add_at(
            DiagnosticKind::MalformedXml,
            path.to_string(),
            format!("line {}: {}", issue.line, issue.message),
        );
    }

    let mut stage = Stage {
        id: SafeRelativePath::parse(path)
            .map(|parsed| parsed.file_stem().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        source: path.to_string(),
        ..Stage::default()
    };

    let Some(root) = document.root else {
        report.add_at(
            DiagnosticKind::MalformedXml,
            path.to_string(),
            "document has no root element",
        );
        return stage;
    };

    if !is_tag(&root, "stage") {
        report.add_at(
            DiagnosticKind::MalformedXml,
            path.to_string(),
            format!("expected a <stage> root, found <{}>", root.name),
        );
    }

    stage.attributes = attributes_of(&root);
    stage.name = attribute_ci(&stage.attributes, "name").map(str::to_string);
    stage.folder = attribute_ci(&stage.attributes, "folder").map(str::to_string);

    for child in &root.children {
        let attributes = attributes_of(child);
        let role = if is_tag(child, "dad") {
            Some(AnchorRole::Opponent)
        } else if is_tag(child, "boyfriend") {
            Some(AnchorRole::Player)
        } else if is_tag(child, "gf") || is_tag(child, "girlfriend") {
            Some(AnchorRole::Girlfriend)
        } else {
            None
        };

        if is_tag(child, "sprite") {
            stage.elements.push(StageElement::Sprite(StageSprite {
                name: attribute_ci(&attributes, "name").map(str::to_string),
                sprite: attribute_ci(&attributes, "sprite").map(str::to_string),
                x: attribute_f64(&attributes, "x"),
                y: attribute_f64(&attributes, "y"),
                scale: attribute_f64(&attributes, "scale"),
                scroll_x: attribute_f64(&attributes, "scrollx")
                    .or_else(|| attribute_f64(&attributes, "scroll")),
                scroll_y: attribute_f64(&attributes, "scrolly")
                    .or_else(|| attribute_f64(&attributes, "scroll")),
                zoom_factor: attribute_f64(&attributes, "zoomFactor"),
                alpha: attribute_f64(&attributes, "alpha"),
                attributes,
            }));
        } else if let Some(role) = role {
            stage.elements.push(StageElement::Anchor(StageAnchor {
                role,
                tag: child.name.clone(),
                x: attribute_f64(&attributes, "x"),
                y: attribute_f64(&attributes, "y"),
                scroll_x: attribute_f64(&attributes, "scrollx")
                    .or_else(|| attribute_f64(&attributes, "scroll")),
                scroll_y: attribute_f64(&attributes, "scrolly")
                    .or_else(|| attribute_f64(&attributes, "scroll")),
                scale: attribute_f64(&attributes, "scale"),
                attributes,
            }));
        } else {
            stage.extra_elements.push(raw_element(child));
        }
    }

    stage
}

/// Parses a `data/events/*.json` definition.
pub(crate) fn parse_event_definition(
    path: &str,
    value: Value,
    report: &mut ImportReport,
) -> EventDefinition {
    let mut definition = EventDefinition {
        name: SafeRelativePath::parse(path)
            .map(|parsed| parsed.file_stem().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        source: path.to_string(),
        raw: value.clone(),
        ..EventDefinition::default()
    };

    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            path.to_string(),
            "event definition is not a JSON object",
        );
        definition.raw = value;
        return definition;
    };

    if let Some(Value::Array(params)) = object.remove("params") {
        for param in params {
            let Value::Object(mut param_object) = param else {
                report.add_at(
                    DiagnosticKind::MalformedMetadata,
                    path.to_string(),
                    "event parameter is not a JSON object",
                );
                continue;
            };
            definition.params.push(EventParameter {
                name: take_string(&mut param_object, "name"),
                value_type: take_string(&mut param_object, "type"),
                default_value: param_object.remove("defaultValue"),
                unknown_fields: param_object.into_iter().collect(),
            });
        }
    } else if object.contains_key("params") {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            path.to_string(),
            "event 'params' is not an array",
        );
    }

    definition.unknown_fields = object.into_iter().collect();
    definition
}

/// Parses a song `meta.json` document.
pub(crate) fn parse_song_metadata(
    path: &str,
    value: Value,
    report: &mut ImportReport,
) -> SongMetadata {
    let mut metadata = SongMetadata {
        raw: value.clone(),
        ..SongMetadata::default()
    };

    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            path.to_string(),
            "song metadata is not a JSON object",
        );
        metadata.raw = value;
        return metadata;
    };

    metadata.display_name = take_string(&mut object, "displayName");
    metadata.icon = take_string(&mut object, "icon");
    metadata.color = take_string(&mut object, "color");
    metadata.bpm = take_f64(&mut object, "bpm");
    metadata.needs_voices = take_bool(&mut object, "needsVoices");
    metadata.steps_per_beat = take_f64(&mut object, "stepsPerBeat");
    metadata.beats_per_measure = take_f64(&mut object, "beatsPerMeasure");
    metadata.opponent_mode_allowed = take_bool(&mut object, "opponentModeAllowed");
    metadata.coop_allowed = take_bool(&mut object, "coopAllowed");
    metadata.gameover = object.remove("gameover");
    metadata.custom_values = object.remove("customValues");

    match object.remove("difficulties") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                match entry {
                    Value::String(name) => metadata.difficulties.push(name),
                    other => {
                        report.add_at(
                            DiagnosticKind::MalformedMetadata,
                            path.to_string(),
                            format!("difficulty entry is not a name: {other}"),
                        );
                    }
                }
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'difficulties' is not an array",
            );
            object.insert("difficulties".to_string(), other);
        }
        None => {}
    }

    metadata.unknown_fields = object.into_iter().collect();
    metadata
}

/// Parses a song `events.json` document.
pub(crate) fn parse_song_events(path: &str, value: Value, report: &mut ImportReport) -> SongEvents {
    let mut events = SongEvents {
        raw: value.clone(),
        ..SongEvents::default()
    };

    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            path.to_string(),
            "song events file is not a JSON object",
        );
        events.raw = value;
        return events;
    };

    match object.remove("events") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                events.events.push(parse_chart_event(entry, path, report));
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'events' is not an array",
            );
            object.insert("events".to_string(), other);
        }
        None => {}
    }

    events.unknown_fields = object.into_iter().collect();
    events
}

/// Parses one chart document.
pub(crate) fn parse_chart_document(
    path: &str,
    difficulty: &str,
    value: Value,
    report: &mut ImportReport,
) -> Chart {
    let mut chart = Chart {
        source: path.to_string(),
        difficulty: difficulty.to_string(),
        raw: value.clone(),
        ..Chart::default()
    };

    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::MalformedJson,
            path.to_string(),
            "chart is not a JSON object",
        );
        return chart;
    };

    // Some exports wrap the chart in a `song` object; Codename itself writes it
    // flat, so the wrapper is unwrapped and reported.
    let mut wrapper: Option<String> = None;
    if !object.contains_key("strumLines") {
        let nested = object
            .get("song")
            .and_then(Value::as_object)
            .is_some_and(|inner| inner.contains_key("strumLines") || inner.contains_key("notes"));
        if nested {
            wrapper = Some("song".to_string());
            if let Some(Value::Object(inner)) = object.remove("song") {
                for (key, value) in inner {
                    object.entry(key).or_insert(value);
                }
            }
        }
    }

    if let Some(wrapper) = wrapper {
        report.push(
            Diagnostic::new(
                DiagnosticKind::UnknownMetadata,
                format!("chart data was wrapped in '{wrapper}' and was unwrapped"),
            )
            .with_severity(Severity::Info)
            .at(path.to_string()),
        );
    }

    let has_strum_lines = object.contains_key("strumLines");
    let has_legacy_notes = object.contains_key("notes");

    chart.format = if has_strum_lines {
        ChartFormat::Codename
    } else if has_legacy_notes {
        report.add_at(
            DiagnosticKind::LegacyChartFormat,
            path.to_string(),
            "legacy note-array chart is preserved verbatim but not converted",
        );
        ChartFormat::LegacyNotes
    } else {
        report.add_at(
            DiagnosticKind::InvalidChart,
            path.to_string(),
            "chart has neither 'strumLines' nor 'notes'",
        );
        ChartFormat::Unknown
    };

    match object.remove("chartVersion") {
        Some(Value::String(version)) => chart.chart_version = Some(version),
        Some(Value::Number(number)) => chart.chart_version = Some(number.to_string()),
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'chartVersion' is neither a string nor a number",
            );
            object.insert("chartVersion".to_string(), other);
        }
        None => {}
    }
    chart.codename_chart = take_bool(&mut object, "codenameChart");
    chart.scroll_speed = take_f64(&mut object, "scrollSpeed");
    chart.stage = take_string(&mut object, "stage");
    chart.players = object.remove("players");

    match object.remove("noteTypes") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                chart.note_types.push(note_type_definition(entry));
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'noteTypes' is not an array",
            );
            object.insert("noteTypes".to_string(), other);
        }
        None => {}
    }

    match object.remove("strumLines") {
        Some(Value::Array(lines)) => {
            for line in lines {
                chart.strum_lines.push(parse_strum_line(line, path, report));
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'strumLines' is not an array",
            );
            object.insert("strumLines".to_string(), other);
        }
        None => {}
    }

    match object.remove("events") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                chart.events.push(parse_chart_event(entry, path, report));
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'events' is not an array",
            );
            object.insert("events".to_string(), other);
        }
        None => {}
    }

    // Aggregated so a mod with many extra properties still produces one clear
    // note instead of a diagnostic per property.
    let mut unrecognized: BTreeSet<String> = object.keys().cloned().collect();
    for line in &chart.strum_lines {
        unrecognized.extend(line.unknown_fields.keys().cloned());
        for note in &line.notes {
            unrecognized.extend(note.extra.keys().cloned());
        }
    }
    for event in &chart.events {
        unrecognized.extend(event.extra.keys().cloned());
    }
    if !unrecognized.is_empty() {
        report.push(
            Diagnostic::new(
                DiagnosticKind::UnknownChartField,
                format!(
                    "properties preserved without interpretation: {}",
                    unrecognized.into_iter().collect::<Vec<_>>().join(", ")
                ),
            )
            .at(path.to_string()),
        );
    }

    chart.unknown_fields = object.into_iter().collect();
    chart
}

fn note_type_definition(value: Value) -> NoteTypeDefinition {
    let name = match &value {
        Value::String(text) => text.clone(),
        Value::Object(object) => object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    };
    NoteTypeDefinition { name, raw: value }
}

fn parse_strum_line(value: Value, path: &str, report: &mut ImportReport) -> StrumLine {
    let mut line = StrumLine {
        raw: value.clone(),
        ..StrumLine::default()
    };

    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::InvalidChart,
            path.to_string(),
            "strum line is not a JSON object",
        );
        return line;
    };

    line.key_count = take_u32(&mut object, "keyCount");
    line.position = take_string(&mut object, "position");
    line.visible = take_bool(&mut object, "visible");
    line.strum_scale = take_f64(&mut object, "strumScale");
    line.strum_spacing = take_f64(&mut object, "strumSpacing");
    line.strum_line_pos = take_f64(&mut object, "strumLinePos");
    line.vocals_suffix = take_string(&mut object, "vocalsSuffix");
    line.line_type = object.remove("type");

    match object.remove("strumPos") {
        Some(Value::Array(values)) => {
            let numbers: Vec<f64> = values
                .iter()
                .filter_map(|value| match value {
                    Value::Number(number) => number.as_f64(),
                    _ => None,
                })
                .collect();
            if numbers.len() == values.len() && numbers.len() == 2 {
                line.strum_pos = Some(StrumPos {
                    x: numbers[0],
                    y: numbers[1],
                });
            } else {
                report.add_at(
                    DiagnosticKind::MalformedMetadata,
                    path.to_string(),
                    "'strumPos' is not a two-number array",
                );
                object.insert("strumPos".to_string(), Value::Array(values));
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'strumPos' is not an array",
            );
            object.insert("strumPos".to_string(), other);
        }
        None => {}
    }

    match object.remove("characters") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                match entry {
                    Value::String(name) => line.characters.push(name),
                    other => {
                        report.add_at(
                            DiagnosticKind::MalformedMetadata,
                            path.to_string(),
                            format!("strum line character is not a name: {other}"),
                        );
                    }
                }
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "'characters' is not an array",
            );
            object.insert("characters".to_string(), other);
        }
        None => {}
    }

    match object.remove("notes") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                line.notes.push(parse_note(entry, path, report));
            }
        }
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "strum line 'notes' is not an array",
            );
            object.insert("notes".to_string(), other);
        }
        None => {}
    }

    line.unknown_fields = object.into_iter().collect();
    line
}

fn parse_note(value: Value, path: &str, report: &mut ImportReport) -> Note {
    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::InvalidChart,
            path.to_string(),
            "note is not a JSON object",
        );
        return Note::default();
    };

    let time = take_f64(&mut object, "time");
    let id = take_u32(&mut object, "id");
    let sustain_length = take_f64(&mut object, "sLen");
    let note_type = object.remove("type");

    if time.is_none() {
        report.add_at(
            DiagnosticKind::InvalidChart,
            path.to_string(),
            "note has no numeric 'time'",
        );
    }
    if id.is_none() {
        report.add_at(
            DiagnosticKind::InvalidChart,
            path.to_string(),
            "note has no integer 'id'",
        );
    }

    Note {
        time,
        id,
        sustain_length,
        note_type,
        extra: object.into_iter().collect(),
    }
}

fn parse_chart_event(value: Value, path: &str, report: &mut ImportReport) -> ChartEvent {
    let mut event = ChartEvent::default();

    let Value::Object(mut object) = value else {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            path.to_string(),
            "event is not a JSON object",
        );
        return event;
    };

    event.time = take_f64(&mut object, "time");
    event.name = take_string(&mut object, "name");

    match object.remove("params") {
        Some(Value::Array(params)) => event.params = params,
        Some(other) => {
            report.add_at(
                DiagnosticKind::MalformedMetadata,
                path.to_string(),
                "event 'params' is not an array",
            );
            object.insert("params".to_string(), other);
        }
        None => {}
    }

    if event.name.is_none() {
        report.add_at(
            DiagnosticKind::MalformedMetadata,
            path.to_string(),
            "event has no 'name'",
        );
    }

    event.extra = object.into_iter().collect();
    event
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Option<String> {
    match map.remove(key) {
        Some(Value::String(text)) => Some(text),
        Some(other) => {
            map.insert(key.to_string(), other);
            None
        }
        None => None,
    }
}

fn take_f64(map: &mut Map<String, Value>, key: &str) -> Option<f64> {
    match map.remove(key) {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => match text.trim().parse::<f64>() {
            Ok(number) => Some(number),
            Err(_) => {
                map.insert(key.to_string(), Value::String(text));
                None
            }
        },
        Some(other) => {
            map.insert(key.to_string(), other);
            None
        }
        None => None,
    }
}

fn take_u32(map: &mut Map<String, Value>, key: &str) -> Option<u32> {
    match map.remove(key) {
        Some(Value::Number(number)) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Some(Value::String(text)) => match text.trim().parse::<u32>() {
            Ok(number) => Some(number),
            Err(_) => {
                map.insert(key.to_string(), Value::String(text));
                None
            }
        },
        Some(other) => {
            map.insert(key.to_string(), other);
            None
        }
        None => None,
    }
}

fn take_bool(map: &mut Map<String, Value>, key: &str) -> Option<bool> {
    match map.remove(key) {
        Some(Value::Bool(value)) => Some(value),
        Some(Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => {
                map.insert(key.to_string(), Value::String(text));
                None
            }
        },
        Some(Value::Number(number)) => match number.as_u64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => {
                map.insert(key.to_string(), Value::Number(number));
                None
            }
        },
        Some(other) => {
            map.insert(key.to_string(), other);
            None
        }
        None => None,
    }
}

/// True when `path` looks like a Codename state script.
pub fn is_state_script(path: &str) -> bool {
    path.starts_with(&format!("{STATES_DIRECTORY}/"))
        && matches!(AssetKind::classify(path), AssetKind::Script(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn parse_json(source: &str) -> Value {
        serde_json::from_str(source).expect("test fixture is valid JSON")
    }

    #[test]
    fn parses_codename_chart_with_full_strum_line_data() {
        let value = parse_json(
            r#"{
                "chartVersion": "1.6.0",
                "codenameChart": true,
                "scrollSpeed": 2.2,
                "stage": "castle",
                "noteTypes": ["No Animation", {"name": "Alt", "anim": "alt"}],
                "strumLines": [
                    {
                        "keyCount": 4,
                        "position": "dad",
                        "characters": ["var"],
                        "type": 0,
                        "visible": true,
                        "strumScale": 1,
                        "strumSpacing": 1,
                        "strumPos": [0, 50],
                        "strumLinePos": 0.75,
                        "vocalsSuffix": "",
                        "notes": [{"time": 9600, "id": 2, "sLen": 525, "type": 0}],
                        "customFlag": true
                    }
                ],
                "events": [{"time": 0, "name": "Cinematic Bars", "params": [true, 0, 100, "quart", "Out"]}],
                "oddTopLevel": {"nested": [1, 2, 3]}
            }"#,
        );
        let mut report = ImportReport::new();
        let chart = parse_chart_document("songs/x/charts/hard.json", "hard", value, &mut report);

        assert_eq!(chart.format, ChartFormat::Codename);
        assert_eq!(chart.chart_version.as_deref(), Some("1.6.0"));
        assert_eq!(chart.codename_chart, Some(true));
        assert_eq!(chart.scroll_speed, Some(2.2));
        assert_eq!(chart.stage.as_deref(), Some("castle"));
        assert_eq!(chart.note_types.len(), 2);
        assert_eq!(chart.note_types[0].name, "No Animation");
        assert_eq!(chart.note_types[1].name, "Alt");
        assert_eq!(chart.note_types[1].raw["anim"], "alt");

        let line = &chart.strum_lines[0];
        assert_eq!(line.key_count, Some(4));
        assert_eq!(line.position.as_deref(), Some("dad"));
        assert_eq!(line.characters, vec!["var"]);
        assert_eq!(line.visible, Some(true));
        assert_eq!(line.strum_scale, Some(1.0));
        assert_eq!(line.strum_spacing, Some(1.0));
        assert_eq!(line.strum_pos, Some(StrumPos { x: 0.0, y: 50.0 }));
        assert_eq!(line.strum_line_pos, Some(0.75));
        assert_eq!(line.vocals_suffix.as_deref(), Some(""));
        assert_eq!(line.line_type, Some(Value::from(0)));
        assert_eq!(line.notes.len(), 1);
        assert_eq!(
            line.unknown_fields.get("customFlag"),
            Some(&Value::Bool(true))
        );

        let note = &line.notes[0];
        assert_eq!(note.time, Some(9600.0));
        assert_eq!(note.id, Some(2));
        assert_eq!(note.sustain_length, Some(525.0));
        assert_eq!(note.note_type, Some(Value::from(0)));

        assert_eq!(chart.events.len(), 1);
        assert_eq!(chart.events[0].name.as_deref(), Some("Cinematic Bars"));
        assert_eq!(chart.events[0].params.len(), 5);
        assert!(chart.unknown_fields.contains_key("oddTopLevel"));
        assert!(chart.raw.get("oddTopLevel").is_some(), "raw is retained");
        assert!(report.contains_code("unknown-chart-field"));
        assert!(!report.has_errors(), "{:?}", report);
    }

    #[test]
    fn keeps_heterogeneous_event_parameters() {
        let value = parse_json(
            r#"{"strumLines": [], "events": [{"time": 1, "name": "E", "params": [true, 0, "x", 1.5, {"k": 1}, null]}]}"#,
        );
        let mut report = ImportReport::new();
        let chart = parse_chart_document("a/charts/hard.json", "hard", value, &mut report);
        let params = &chart.events[0].params;
        assert_eq!(params.len(), 6);
        assert_eq!(params[0], Value::Bool(true));
        assert_eq!(params[5], Value::Null);
    }

    #[test]
    fn reports_legacy_and_missing_layouts() {
        let mut report = ImportReport::new();
        let legacy = parse_chart_document(
            "a/charts/hard.json",
            "hard",
            parse_json(r#"{"song": {"notes": [[1, 2]], "bpm": 150}}"#),
            &mut report,
        );
        assert_eq!(legacy.format, ChartFormat::LegacyNotes);
        assert!(report.contains_code("legacy-chart-format"));
        assert!(legacy.raw.get("song").is_some());

        let mut report = ImportReport::new();
        let broken =
            parse_chart_document("a/charts/hard.json", "hard", parse_json("{}"), &mut report);
        assert_eq!(broken.format, ChartFormat::Unknown);
        assert!(report.contains_code("invalid-chart"));
        assert!(report.has_errors());
    }

    #[test]
    fn parses_song_metadata_and_unknown_fields() {
        let mut report = ImportReport::new();
        let metadata = parse_song_metadata(
            "songs/x/meta.json",
            parse_json(
                r#"{"bpm": 125, "needsVoices": true, "difficulties": ["hard"], "displayName": "x",
                    "gameover": {"quote": "q"}, "customValues": {}, "extraThing": 1}"#,
            ),
            &mut report,
        );
        assert_eq!(metadata.bpm, Some(125.0));
        assert_eq!(metadata.needs_voices, Some(true));
        assert_eq!(metadata.difficulties, vec!["hard"]);
        assert!(metadata.gameover.is_some());
        assert!(metadata.unknown_fields.contains_key("extraThing"));
        assert!(metadata.raw.get("extraThing").is_some());
    }

    #[test]
    fn parses_character_and_stage_documents() {
        let mut report = ImportReport::new();
        let character = parse_character_document(
            "data/characters/var.xml",
            XmlDocument::parse(
                r#"<character sprite="Var" icon="var" mystery="1"><anim name="idle" anim="Var_Idle" x="-10" y="-65"/><anim anim="nameless"/><unknown/></character>"#,
            ),
            &mut report,
        );
        assert_eq!(character.id, "var");
        assert_eq!(character.sprite.as_deref(), Some("Var"));
        assert_eq!(character.animations.len(), 2);
        assert_eq!(
            character.animations[0].animation.as_deref(),
            Some("Var_Idle")
        );
        assert_eq!(character.animations[0].x, Some(-10.0));
        assert_eq!(character.extra_elements.len(), 1);
        assert_eq!(character.attributes.len(), 3);
        assert!(report.contains_code("invalid-animation"));

        let mut report = ImportReport::new();
        let stage = parse_stage_document(
            "data/stages/castle.xml",
            XmlDocument::parse(
                r#"<stage name="castle" folder="stages/castle/"><sprite name="floor" sprite="floor" scroll="1" zoomFactor="1.2"/><girlfriend x="830" y="410"/><gf x="1"/><dad x="2"/><boyfriend x="3"/><weird/></stage>"#,
            ),
            &mut report,
        );
        assert_eq!(stage.id, "castle");
        assert_eq!(stage.folder.as_deref(), Some("stages/castle/"));
        assert_eq!(stage.sprites().count(), 1);
        let anchors: Vec<_> = stage.anchors().collect();
        assert_eq!(anchors.len(), 4);
        assert_eq!(anchors[0].role, AnchorRole::Girlfriend);
        assert_eq!(anchors[0].tag, "girlfriend");
        assert_eq!(anchors[1].tag, "gf");
        assert_eq!(anchors[2].role, AnchorRole::Opponent);
        assert_eq!(anchors[3].role, AnchorRole::Player);
        assert_eq!(stage.extra_elements.len(), 1);
        let sprite = stage.sprites().next().unwrap();
        assert_eq!(sprite.scroll_x, Some(1.0));
        assert_eq!(sprite.scroll_y, Some(1.0));
        assert_eq!(sprite.zoom_factor, Some(1.2));
    }

    #[test]
    fn parses_ini_config_with_unknown_sections() {
        let temp = TempDir::new("config").unwrap();
        temp.write(
            "mod/data/config/modpack.ini",
            "[Common] # note\nAPI_VERSION=3\nDOWNLOAD_LINK=\"https://example\"\n[Flags]\nA=true\n[StateRedirects]\nTitleState=newTitle\n[StateRedirects.force]\nTitleState=forced\n[Custom]\nkey=value\n",
        )
        .unwrap();
        let discovered = crate::codename::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_mod(&discovered);

        assert_eq!(parsed.module.config.api_version.as_deref(), Some("3"));
        assert_eq!(
            parsed.module.config.flags.get("A"),
            Some(&"true".to_string())
        );
        assert_eq!(parsed.module.config.state_redirects.len(), 2);
        assert!(!parsed.module.config.state_redirects[0].force);
        assert!(parsed.module.config.state_redirects[1].force);
        assert_eq!(parsed.module.config.unknown_sections, vec!["Custom"]);
        assert_eq!(parsed.module.config.sources.len(), 1);
        assert!(parsed.report.contains_code("unknown-metadata"));
    }

    #[test]
    fn inventories_scripts_and_binaries_without_reading_them() {
        let temp = TempDir::new("scripts").unwrap();
        temp.write("mod/data/scripts/boom.hx", "trace('hi');")
            .unwrap();
        temp.write("mod/data/events/Camera Bump.hx", "// event")
            .unwrap();
        temp.write("mod/data/events/Camera Bump.json", r#"{"params": []}"#)
            .unwrap();
        temp.write("mod/plugins/evil.dll", "MZ").unwrap();
        temp.write("mod/backup.rar", "RAR").unwrap();
        let discovered = crate::codename::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_mod(&discovered);

        assert_eq!(parsed.module.scripts.len(), 2);
        assert!(parsed
            .module
            .scripts
            .iter()
            .any(|script| script.in_event_directory));
        assert_eq!(parsed.module.unsupported_files.len(), 2);
        assert!(parsed
            .module
            .unsupported_files
            .iter()
            .any(|file| file.reason == UnsupportedReason::NativeLibrary));
        assert!(parsed
            .module
            .unsupported_files
            .iter()
            .any(|file| file.reason == UnsupportedReason::Archive));
        assert!(parsed.report.contains_code("unsupported-script"));
        assert!(parsed.report.contains_code("unsupported-binary"));
        assert!(parsed.report.contains_code("unsupported-archive"));
    }

    #[test]
    fn song_parsing_collects_audio_charts_and_scripts() {
        let temp = TempDir::new("song").unwrap();
        temp.write(
            "mod/songs/black-humor/meta.json",
            r#"{"bpm": 125, "difficulties": ["hard"], "needsVoices": true}"#,
        )
        .unwrap();
        temp.write(
            "mod/songs/black-humor/charts/hard.json",
            r#"{"codenameChart": true, "strumLines": [{"keyCount": 4, "notes": [{"time": 0, "id": 0}]}]}"#,
        )
        .unwrap();
        temp.write_bytes("mod/songs/black-humor/song/Inst.ogg", b"OggS")
            .unwrap();
        temp.write_bytes("mod/songs/black-humor/song/Voices.ogg", b"OggS")
            .unwrap();
        temp.write("mod/songs/black-humor/scripts/notes.hx", "")
            .unwrap();
        temp.write("mod/songs/black-humor/events.json", r#"{"events": []}"#)
            .unwrap();

        let discovered = crate::codename::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_mod(&discovered);
        let song = &parsed.module.songs[0];
        assert_eq!(song.name, "black-humor");
        assert_eq!(song.charts.len(), 1);
        assert_eq!(song.charts[0].difficulty, "hard");
        assert_eq!(song.audio.len(), 2);
        assert_eq!(song.scripts.len(), 1);
        assert!(song.metadata.is_some());
        assert!(song.events.is_some());
        assert!(song.other_files.is_empty());
    }

    #[test]
    fn missing_song_metadata_is_reported() {
        let temp = TempDir::new("song-missing").unwrap();
        temp.write("mod/songs/x/charts/hard.json", r#"{"strumLines": []}"#)
            .unwrap();
        let discovered = crate::codename::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_mod(&discovered);
        assert!(parsed.report.contains_code("missing-metadata"));
    }

    #[test]
    fn case_mismatched_config_file_is_read_and_reported() {
        let temp = TempDir::new("case").unwrap();
        temp.write("mod/data/config/Modpack.ini", "[Flags]\nA=true\n")
            .unwrap();
        let discovered = crate::codename::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_mod(&discovered);
        assert!(parsed.report.contains_code("case-mismatch"));
        assert_eq!(
            parsed.module.config.flags.get("A"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn state_script_helper_matches_classification() {
        assert!(is_state_script("data/states/newTitle.hx"));
        assert!(!is_state_script("data/scripts/newTitle.hx"));
        assert!(!is_state_script("data/states/newTitle.xml"));
    }
}
