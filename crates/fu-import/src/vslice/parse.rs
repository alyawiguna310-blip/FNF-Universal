//! Parsing of a discovered V-Slice mod into the typed model.
//!
//! Every read goes through [`crate::path::read_text_within`], so the parser
//! cannot escape the mod root, follow a symlink out of it or read an oversized
//! file. Scripts are inventoried by extension only and are never read, compiled
//! or executed.
//!
//! Decoding is deliberately two-stage. A document is first parsed as JSON and
//! kept verbatim; then a typed view is attempted. When the typed view fails the
//! raw text is stored in [`VsliceMod::unparsed`] and a diagnostic is reported,
//! so malformed content is visible rather than silently dropped.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::codename::model::{
    AssetInventory, AssetKind, ScriptAsset, UnsupportedFile, UnsupportedReason,
};
use crate::discovery::{scan_mod_tree, DiscoveredMod, ModTree};
use crate::path::{self, PathError, SafeRelativePath};
use crate::report::{Diagnostic, DiagnosticKind, ImportReport};
use crate::vslice::discovery::metadata_kind;
use crate::vslice::model::{
    Character, CharacterAnimation, Chart, ChartEvent, Contributor, Note, PlayData, ProjectMetadata,
    Song, SongCharacters, SongMetadata, Stage, StageCharacter, StageProp, TimeChange,
    UnparsedDocument, VsliceMod, Week, VSLICE_METADATA_FILE,
};

/// Directory holding song folders.
pub const SONGS_DIRECTORY: &str = "songs";
/// Directories that may hold character definitions, in lookup order.
pub const CHARACTERS_DIRECTORIES: [&str; 2] = ["characters", "data/characters"];
/// Directories that may hold stage definitions, in lookup order.
pub const STAGES_DIRECTORIES: [&str; 2] = ["stages", "data/stages"];
/// Directories that may hold story week definitions, in lookup order.
pub const WEEKS_DIRECTORIES: [&str; 2] = ["weeks", "data/weeks"];

/// Limits applied while parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsliceParseOptions {
    /// Maximum directory depth scanned inside the mod.
    pub max_depth: usize,
    /// Maximum size of a single text file, in bytes.
    pub max_file_bytes: u64,
}

impl Default for VsliceParseOptions {
    fn default() -> Self {
        Self {
            max_depth: crate::discovery::DEFAULT_MAX_DEPTH,
            max_file_bytes: path::MAX_IMPORT_FILE_BYTES,
        }
    }
}

/// A parsed mod together with everything noticed on the way.
#[derive(Debug, Clone)]
pub struct ParsedVsliceMod {
    /// The imported mod.
    pub module: VsliceMod,
    /// Findings from scanning and parsing. Validation adds to this report.
    pub report: ImportReport,
}

/// Parses a discovered mod directory with default limits.
pub fn parse_vslice_mod(discovered: &DiscoveredMod) -> ParsedVsliceMod {
    parse_vslice_mod_with(discovered, VsliceParseOptions::default())
}

/// Parses a discovered mod directory.
pub fn parse_vslice_mod_with(
    discovered: &DiscoveredMod,
    options: VsliceParseOptions,
) -> ParsedVsliceMod {
    let tree = scan_mod_tree(&discovered.root, options.max_depth);
    let mut report = ImportReport::with_target(discovered.name.clone());
    report.extend(tree.report.clone());

    let assets = AssetInventory::build(
        tree.files
            .iter()
            .map(|file| (file.relative.to_string(), file.size)),
    );

    let module = VsliceMod {
        name: discovered.name.clone(),
        root: discovered.root.to_string_lossy().to_string(),
        assets,
        ..VsliceMod::default()
    };

    let mut parser = Parser {
        root: discovered.root.clone(),
        tree,
        report,
        module,
        options,
    };
    parser.run();
    ParsedVsliceMod {
        module: parser.module,
        report: parser.report,
    }
}

struct Parser {
    root: std::path::PathBuf,
    tree: ModTree,
    report: ImportReport,
    module: VsliceMod,
    options: VsliceParseOptions,
}

impl Parser {
    fn run(&mut self) {
        self.collect_scripts_and_binaries();
        self.parse_project_metadata();
        self.parse_songs();
        self.parse_characters();
        self.parse_stages();
        self.parse_weeks();
        self.report.sort();
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

    /// Parses JSON and a typed view of it, retaining the raw text on failure.
    fn decode<T>(&mut self, path: &str, text: &str) -> Option<(T, Value)>
    where
        T: DeserializeOwned,
    {
        let raw: Value = match serde_json::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                self.report
                    .add_at(DiagnosticKind::MalformedJson, path, error.to_string());
                self.push_unparsed(path, error.to_string(), text);
                return None;
            }
        };
        if !raw.is_object() {
            let reason = "document is not a JSON object".to_string();
            self.report
                .add_at(DiagnosticKind::MalformedMetadata, path, reason.clone());
            self.push_unparsed(path, reason, text);
            return None;
        }
        match serde_json::from_value::<T>(raw.clone()) {
            Ok(value) => Some((value, raw)),
            Err(error) => {
                let reason = error.to_string();
                self.report
                    .add_at(DiagnosticKind::MalformedMetadata, path, reason.clone());
                self.push_unparsed(path, reason, text);
                None
            }
        }
    }

    fn push_unparsed(&mut self, path: &str, reason: String, contents: &str) {
        self.module.unparsed.push(UnparsedDocument {
            path: path.to_string(),
            reason,
            contents: contents.to_string(),
        });
    }

    fn collect_scripts_and_binaries(&mut self) {
        let entries = self.module.assets.entries.clone();
        let mut script_count = 0usize;
        for entry in entries {
            match &entry.kind {
                AssetKind::Script(language) => {
                    script_count += 1;
                    self.module.scripts.push(ScriptAsset {
                        path: entry.path,
                        language: language.clone(),
                        size: entry.size,
                        in_event_directory: false,
                    });
                }
                AssetKind::Binary => {
                    let reason = binary_reason(&entry.path);
                    self.report.push(
                        Diagnostic::new(
                            DiagnosticKind::UnsupportedBinary,
                            format!(
                                "{} was recorded and will never be loaded",
                                describe_reason(&reason)
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
                    "{script_count} script(s) were inventoried by path and size only; \
                     they are never read or executed"
                ),
            ));
        }
    }

    fn parse_project_metadata(&mut self) {
        let Some(kind) = metadata_kind(&self.root) else {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                VSLICE_METADATA_FILE,
                "no `_polymod_meta.json` or `manifest.json` was found at the mod root",
            );
            return;
        };
        let file = kind.file_name().to_string();
        let Some(text) = self.read_text(&file) else {
            return;
        };
        let Some((json, raw)) = self.decode::<ProjectMetadataJson>(&file, &text) else {
            return;
        };

        let contributors: Vec<Contributor> = json
            .contributors
            .into_iter()
            .map(|contributor| Contributor {
                name: contributor.name,
                role: contributor.role,
                email: contributor.email,
                url: contributor.url,
                unknown_fields: contributor.unknown,
            })
            .collect();

        let mut authors: Vec<String> = Vec::new();
        if let Some(author) = json
            .author
            .as_ref()
            .filter(|author| !author.trim().is_empty())
        {
            authors.push(author.clone());
        }
        for contributor in &contributors {
            if let Some(name) = contributor
                .name
                .as_ref()
                .filter(|name| !name.trim().is_empty())
            {
                if !authors.iter().any(|existing| existing == name) {
                    authors.push(name.clone());
                }
            }
        }

        let title = json.title.or(json.name);
        let title = title.filter(|title| !title.trim().is_empty());
        let id = json.id.filter(|id| !id.trim().is_empty());
        if id.is_none() && title.is_none() {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                &file,
                "metadata declares neither an id nor a title",
            );
        }

        self.module.metadata = Some(ProjectMetadata {
            source: file.clone(),
            id,
            title,
            description: json.description,
            homepage: json.homepage,
            api_version: json.api_version,
            mod_version: json.mod_version,
            license: json.license,
            icon: json.icon.or(json.logo),
            authors,
            contributors,
            dependencies: stringify_map(json.dependencies),
            optional_dependencies: stringify_map(json.optional_dependencies),
            raw,
            unknown_fields: json.unknown,
        });
        self.module.metadata_source = Some(file);
    }

    fn parse_songs(&mut self) {
        // V-Slice keeps song data under `data/songs/<id>/` and the audio under
        // `songs/<id>/`, so a song is assembled from both directories and other
        // mods keep everything next to the audio.
        let mut locations: BTreeMap<String, SongLocation> = BTreeMap::new();
        for (root, is_data) in self.song_roots() {
            let prefix = format!("{root}/");
            let mut directories: Vec<String> = self
                .tree
                .directories
                .iter()
                .filter(|directory| {
                    directory
                        .strip_prefix(&prefix)
                        .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
                })
                .cloned()
                .collect();
            directories.sort();
            directories.dedup();

            for directory in directories {
                let id = directory.rsplit('/').next().unwrap_or_default().to_string();
                let location =
                    locations
                        .entry(id.to_ascii_lowercase())
                        .or_insert_with(|| SongLocation {
                            id,
                            data_dir: None,
                            audio_dir: None,
                        });
                if is_data {
                    location.data_dir.get_or_insert(directory);
                } else {
                    location.audio_dir.get_or_insert(directory);
                }
            }
        }

        for location in locations.into_values() {
            let song = self.parse_song(location);
            self.module.songs.push(song);
        }
    }

    /// Directories that may hold song folders, with `true` for `data/songs`.
    fn song_roots(&self) -> Vec<(String, bool)> {
        let mut roots = Vec::new();
        let data_songs = self
            .tree
            .directories
            .iter()
            .find(|directory| {
                let mut parts = directory.split('/');
                matches!(parts.next(), Some(first) if first.eq_ignore_ascii_case("data"))
                    && matches!(parts.next(), Some(second) if second.eq_ignore_ascii_case("songs"))
                    && parts.next().is_none()
            })
            .cloned();
        if let Some(root) = data_songs {
            roots.push((root, true));
        }
        if let Some(root) = self.top_level_directory(SONGS_DIRECTORY) {
            roots.push((root, false));
        }
        roots
    }

    fn parse_song(&mut self, location: SongLocation) -> Song {
        let SongLocation {
            id,
            data_dir,
            audio_dir,
        } = location;
        let directory = data_dir
            .clone()
            .or_else(|| audio_dir.clone())
            .unwrap_or_default();
        let mut song = Song {
            id,
            directory: directory.clone(),
            ..Song::default()
        };

        let mut metadata_files: Vec<(Option<String>, String)> = Vec::new();
        let mut chart_files: Vec<(Option<String>, String)> = Vec::new();
        let mut nested_chart_directory: Option<String> = None;

        let mut content_directories: Vec<String> = Vec::new();
        if let Some(data) = &data_dir {
            content_directories.push(data.clone());
        }
        if let Some(audio) = &audio_dir {
            if data_dir.as_deref() != Some(audio.as_str()) {
                content_directories.push(audio.clone());
            }
        }

        for content_directory in &content_directories {
            for file in self.files_in(content_directory) {
                let Some(name) = file.rsplit('/').next() else {
                    continue;
                };
                let Some(role) = classify_song_file(name) else {
                    continue;
                };
                match role.0 {
                    "metadata" => metadata_files.push((role.1, file)),
                    "chart" => chart_files.push((role.1, file)),
                    _ => {}
                }
            }
        }
        metadata_files.sort();
        metadata_files.dedup();
        chart_files.sort();
        chart_files.dedup();

        // A `charts/` folder is used by some mods; it is only consulted when the
        // song directory has no chart file of its own.
        if chart_files.is_empty() {
            let wanted = format!("{directory}/charts");
            if let Some(nested) = self
                .tree
                .directories
                .iter()
                .find(|candidate| candidate.eq_ignore_ascii_case(&wanted))
            {
                nested_chart_directory = Some(nested.clone());
            }
        }

        metadata_files.sort();
        chart_files.sort();

        for (variation, file) in metadata_files {
            let Some(text) = self.read_text(&file) else {
                continue;
            };
            let Some(metadata) = self.parse_song_metadata(&file, variation.clone(), &text) else {
                continue;
            };
            match variation {
                None => song.metadata = Some(metadata),
                Some(_) => song.variation_metadata.push(metadata),
            }
        }

        for (variation, file) in chart_files {
            let Some(text) = self.read_text(&file) else {
                continue;
            };
            if let Some(chart) = self.parse_chart(&file, variation, &text) {
                song.charts.push(chart);
            }
        }

        if let Some(nested) = nested_chart_directory {
            let mut nested_files = self.files_in(&nested);
            nested_files.sort();
            if !nested_files.is_empty() {
                self.report.add_at(
                    DiagnosticKind::UnknownChartField,
                    &nested,
                    "charts are read from a `charts` folder because the song directory \
                     has no chart file of its own",
                );
            }
            for file in nested_files {
                if !file.to_ascii_lowercase().ends_with(".json") {
                    continue;
                }
                let Some(text) = self.read_text(&file) else {
                    continue;
                };
                if let Some(chart) = self.parse_chart(&file, None, &text) {
                    song.charts.push(chart);
                }
            }
        }

        if song.metadata.is_none() {
            self.report.add_at(
                DiagnosticKind::MissingMetadata,
                &directory,
                "song directory has no readable metadata file",
            );
        }
        if song.charts.is_empty() {
            self.report.add_at(
                DiagnosticKind::MissingSongFile,
                &directory,
                "song directory has no readable chart file",
            );
        }

        let asset_directory = audio_dir.unwrap_or_else(|| directory.clone());
        for file in self.files_in(&asset_directory) {
            let Some(entry) = self.module.assets.get(&file).cloned() else {
                continue;
            };
            if entry.kind == AssetKind::Audio {
                song.audio.push(entry);
            } else if entry.kind != AssetKind::Json {
                song.other_files.push(entry.path);
            }
        }

        song
    }

    fn parse_song_metadata(
        &mut self,
        path: &str,
        variation: Option<String>,
        text: &str,
    ) -> Option<SongMetadata> {
        let (json, raw) = self.decode::<SongMetadataJson>(path, text)?;

        let mut play_data = json.play_data.map(|play_data| PlayData {
            album: play_data.album,
            preview_start: play_data.preview_start,
            preview_end: play_data.preview_end,
            ratings: play_data.ratings,
            song_variations: play_data.song_variations,
            difficulties: play_data.difficulties,
            characters: play_data.characters.map(|characters| SongCharacters {
                player: characters.player,
                girlfriend: characters.girlfriend,
                opponent: characters.opponent,
                instrumental: characters.instrumental,
                alt_instrumentals: characters.alt_instrumentals,
                unknown_fields: characters.unknown,
            }),
            stage: play_data.stage,
            note_style: play_data.note_style,
            unknown_fields: play_data.unknown,
        });

        if play_data.is_none() {
            let has_legacy_data =
                !json.difficulties.is_empty() || json.characters.is_some() || json.stage.is_some();
            if has_legacy_data {
                self.report.push(
                    Diagnostic::new(
                        DiagnosticKind::LegacyChartFormat,
                        "song metadata stores play data at the top level; it was mapped \
                         into `playData` and the original document was preserved",
                    )
                    .at(path),
                );
                play_data = Some(PlayData {
                    difficulties: json.difficulties,
                    characters: json.characters.map(|characters| SongCharacters {
                        player: characters.player,
                        girlfriend: characters.girlfriend,
                        opponent: characters.opponent,
                        instrumental: characters.instrumental,
                        alt_instrumentals: characters.alt_instrumentals,
                        unknown_fields: characters.unknown,
                    }),
                    stage: json.stage,
                    note_style: json.note_style,
                    ..PlayData::default()
                });
            }
        }

        let mut time_changes = Vec::new();
        for (index, item) in json.time_changes.iter().enumerate() {
            match serde_json::from_value::<TimeChangeJson>(item.clone()) {
                Ok(change) => time_changes.push(TimeChange {
                    t: change.t,
                    bpm: change.bpm,
                    n: change.n,
                    d: change.d,
                    bt: change.bt,
                    raw: item.clone(),
                    unknown_fields: change.unknown,
                }),
                Err(error) => {
                    self.report.add_at(
                        DiagnosticKind::MalformedMetadata,
                        path,
                        format!("time change {index}: {error}"),
                    );
                    time_changes.push(TimeChange {
                        raw: item.clone(),
                        ..TimeChange::default()
                    });
                }
            }
        }

        Some(SongMetadata {
            source: path.to_string(),
            variation,
            version: json.version,
            song_name: json.song_name,
            artist: json.artist,
            charter: json.charter,
            time_format: json.time_format,
            looped: json.looped,
            divisions: json.divisions,
            offsets: json.offsets,
            time_changes,
            play_data,
            generated_by: json.generated_by,
            raw,
            unknown_fields: json.unknown,
        })
    }

    fn parse_chart(&mut self, path: &str, variation: Option<String>, text: &str) -> Option<Chart> {
        let (json, raw) = self.decode::<ChartJson>(path, text)?;

        if raw.get("notes").is_none() && raw.get("song").is_some() {
            let reason = "chart wraps its data in `song`, which is not the V-Slice layout";
            self.report.push(
                Diagnostic::new(
                    DiagnosticKind::LegacyChartFormat,
                    format!("{reason}; the document is preserved verbatim but not interpreted"),
                )
                .at(path),
            );
            self.push_unparsed(path, reason.to_string(), text);
            return None;
        }

        let notes = self.parse_notes(path, json.notes.as_ref());
        let events = self.parse_events(path, json.events.as_ref());
        let scroll_speed = self.parse_scroll_speed(path, json.scroll_speed.as_ref());

        Some(Chart {
            source: path.to_string(),
            variation,
            version: json.version,
            format: json.format,
            generated_by: json.generated_by,
            scroll_speed,
            notes,
            events,
            raw,
            unknown_fields: json.unknown,
        })
    }

    fn parse_notes(&mut self, path: &str, value: Option<&Value>) -> BTreeMap<String, Vec<Note>> {
        let mut notes: BTreeMap<String, Vec<Note>> = BTreeMap::new();
        match value {
            None => {}
            Some(Value::Array(items)) => {
                self.report.push(
                    Diagnostic::new(
                        DiagnosticKind::LegacyChartFormat,
                        "chart stores notes as a single array; they were imported as the \
                         `default` difficulty",
                    )
                    .at(path),
                );
                notes.insert(
                    "default".to_string(),
                    self.parse_note_array(path, "default", items),
                );
            }
            Some(Value::Object(map)) => {
                for (difficulty, items) in map {
                    match items {
                        Value::Array(items) => {
                            let parsed = self.parse_note_array(path, difficulty, items);
                            notes.insert(difficulty.clone(), parsed);
                        }
                        _ => self.report.add_at(
                            DiagnosticKind::InvalidChart,
                            path,
                            format!("difficulty '{difficulty}' is not an array of notes"),
                        ),
                    }
                }
            }
            Some(_) => self.report.add_at(
                DiagnosticKind::InvalidChart,
                path,
                "`notes` is neither an object keyed by difficulty nor an array",
            ),
        }
        notes
    }

    fn parse_note_array(&mut self, path: &str, difficulty: &str, items: &[Value]) -> Vec<Note> {
        let mut notes = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            match serde_json::from_value::<NoteJson>(item.clone()) {
                Ok(json) => notes.push(Note {
                    index,
                    time_ms: json.t,
                    direction: json.d,
                    sustain_ms: json.l,
                    kind: json.k,
                    params: json.p,
                    raw: item.clone(),
                    unknown_fields: json.unknown,
                }),
                Err(error) => {
                    self.report.add_at(
                        DiagnosticKind::InvalidNote,
                        path,
                        format!("{difficulty} note {index} could not be decoded: {error}"),
                    );
                    notes.push(Note {
                        index,
                        raw: item.clone(),
                        ..Note::default()
                    });
                }
            }
        }
        notes
    }

    fn parse_events(&mut self, path: &str, value: Option<&Value>) -> Vec<ChartEvent> {
        let Some(value) = value else {
            return Vec::new();
        };
        let Value::Array(items) = value else {
            self.report.add_at(
                DiagnosticKind::InvalidChart,
                path,
                "`events` is not an array",
            );
            return Vec::new();
        };

        let mut events = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            match serde_json::from_value::<EventJson>(item.clone()) {
                Ok(json) => events.push(ChartEvent {
                    time_ms: json.t,
                    name: json.e,
                    value: json.v,
                    raw: item.clone(),
                    unknown_fields: json.unknown,
                }),
                Err(error) => {
                    self.report.add_at(
                        DiagnosticKind::InvalidEvent,
                        path,
                        format!("event {index} could not be decoded: {error}"),
                    );
                    events.push(ChartEvent {
                        raw: item.clone(),
                        ..ChartEvent::default()
                    });
                }
            }
        }
        events
    }

    fn parse_scroll_speed(&mut self, path: &str, value: Option<&Value>) -> BTreeMap<String, f64> {
        let mut speeds: BTreeMap<String, f64> = BTreeMap::new();
        match value {
            None => {}
            Some(Value::Number(number)) => {
                if let Some(speed) = number.as_f64() {
                    speeds.insert("default".to_string(), speed);
                }
            }
            Some(Value::Object(map)) => {
                for (difficulty, value) in map {
                    match value.as_f64() {
                        Some(speed) => {
                            speeds.insert(difficulty.clone(), speed);
                        }
                        None => self.report.add_at(
                            DiagnosticKind::MalformedMetadata,
                            path,
                            format!("scroll speed for '{difficulty}' is not a number"),
                        ),
                    }
                }
            }
            Some(_) => self.report.add_at(
                DiagnosticKind::MalformedMetadata,
                path,
                "`scrollSpeed` is neither a number nor an object",
            ),
        }
        speeds
    }

    fn parse_characters(&mut self) {
        for directory in self.directories_in(CHARACTERS_DIRECTORIES) {
            let mut files = self.files_in(&directory);
            files.sort();
            for file in files {
                if !file.to_ascii_lowercase().ends_with(".json") {
                    continue;
                }
                let Some(text) = self.read_text(&file) else {
                    continue;
                };
                if let Some(character) = self.parse_character(&file, &text) {
                    self.module.characters.push(character);
                }
            }
        }
    }

    fn parse_character(&mut self, path: &str, text: &str) -> Option<Character> {
        let (json, raw) = self.decode::<CharacterJson>(path, text)?;
        let id = file_stem(path);

        let mut animations = Vec::with_capacity(json.animations.len());
        for (index, item) in json.animations.iter().enumerate() {
            match serde_json::from_value::<AnimationJson>(item.clone()) {
                Ok(animation) => animations.push(CharacterAnimation {
                    name: animation.name,
                    prefix: animation.prefix,
                    frame_rate: animation.frame_rate,
                    frame_indices: animation.frame_indices,
                    looped: animation.looped,
                    offsets: animation.offsets,
                    flip_x: animation.flip_x,
                    flip_y: animation.flip_y,
                    anim_type: animation.anim_type,
                    raw: item.clone(),
                    unknown_fields: animation.unknown,
                }),
                Err(error) => self.report.add_at(
                    DiagnosticKind::InvalidAnimation,
                    path,
                    format!("animation {index} could not be decoded: {error}"),
                ),
            }
        }

        if animations.is_empty() {
            self.report.add_at(
                DiagnosticKind::InvalidAnimation,
                path,
                "character declares no usable animations",
            );
        }

        Some(Character {
            id,
            source: path.to_string(),
            name: json.name,
            render_type: json.render_type,
            asset_path: json.asset_path,
            scale: json.scale,
            is_pixel: json.is_pixel,
            flip_x: json.flip_x,
            flip_y: json.flip_y,
            offsets: json.offsets,
            camera_offsets: json.camera_offsets,
            dance_every: json.dance_every,
            sing_time: json.sing_time,
            starting_animation: json.starting_animation,
            health_icon: json.health_icon,
            death: json.death,
            atlas_settings: json.atlas_settings,
            animations,
            raw,
            unknown_fields: json.unknown,
        })
    }

    fn parse_stages(&mut self) {
        for directory in self.directories_in(STAGES_DIRECTORIES) {
            let mut files = self.files_in(&directory);
            files.sort();
            for file in files {
                if !file.to_ascii_lowercase().ends_with(".json") {
                    continue;
                }
                let Some(text) = self.read_text(&file) else {
                    continue;
                };
                if let Some(stage) = self.parse_stage(&file, &text) {
                    self.module.stages.push(stage);
                }
            }
        }
    }

    fn parse_stage(&mut self, path: &str, text: &str) -> Option<Stage> {
        let (json, raw) = self.decode::<StageJson>(path, text)?;
        let id = file_stem(path);

        let mut props = Vec::with_capacity(json.props.len());
        for (index, item) in json.props.iter().enumerate() {
            match serde_json::from_value::<StagePropJson>(item.clone()) {
                Ok(prop) => props.push(StageProp {
                    name: prop.name,
                    asset_path: prop.asset_path,
                    position: prop.position,
                    z_index: prop.z_index,
                    scale: prop.scale,
                    alpha: prop.alpha,
                    scroll: prop.scroll,
                    animations: prop.animations,
                    anim_type: prop.anim_type,
                    starting_animation: prop.starting_animation,
                    raw: item.clone(),
                    unknown_fields: prop.unknown,
                }),
                Err(error) => self.report.add_at(
                    DiagnosticKind::MalformedMetadata,
                    path,
                    format!("prop {index} could not be decoded: {error}"),
                ),
            }
        }

        let mut characters: BTreeMap<String, StageCharacter> = BTreeMap::new();
        for (role, item) in &json.characters {
            match serde_json::from_value::<StageCharacterJson>(item.clone()) {
                Ok(placement) => {
                    characters.insert(
                        role.clone(),
                        StageCharacter {
                            z_index: placement.z_index,
                            position: placement.position,
                            scale: placement.scale,
                            camera_offsets: placement.camera_offsets,
                            scroll: placement.scroll,
                            alpha: placement.alpha,
                            raw: item.clone(),
                            unknown_fields: placement.unknown,
                        },
                    );
                }
                Err(error) => self.report.add_at(
                    DiagnosticKind::MalformedMetadata,
                    path,
                    format!("character placement '{role}' could not be decoded: {error}"),
                ),
            }
        }

        Some(Stage {
            id,
            source: path.to_string(),
            name: json.name,
            version: json.version,
            directory: json.directory,
            camera_zoom: json.camera_zoom,
            is_pixel_stage: json.is_pixel_stage,
            props,
            layers: json.layers,
            characters,
            raw,
            unknown_fields: json.unknown,
        })
    }

    fn parse_weeks(&mut self) {
        for directory in self.directories_in(WEEKS_DIRECTORIES) {
            let mut files = self.files_in(&directory);
            files.sort();
            for file in files {
                if !file.to_ascii_lowercase().ends_with(".json") {
                    continue;
                }
                let Some(text) = self.read_text(&file) else {
                    continue;
                };
                let Some((json, raw)) = self.decode::<WeekJson>(&file, &text) else {
                    continue;
                };
                let mut songs = Vec::new();
                for item in &json.songs {
                    match item {
                        Value::String(id) => songs.push(id.clone()),
                        Value::Object(object) => {
                            for key in ["song", "id", "name"] {
                                if let Some(Value::String(id)) = object.get(key) {
                                    songs.push(id.clone());
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                self.module.weeks.push(Week {
                    id: file_stem(&file),
                    source: file,
                    story_name: json.story_name.or(json.name),
                    songs,
                    raw,
                    unknown_fields: json.unknown,
                });
            }
        }
    }

    /// The mod-relative path of a top-level directory, matched case-insensitively.
    fn top_level_directory(&self, name: &str) -> Option<String> {
        self.tree
            .directories
            .iter()
            .find(|directory| !directory.contains('/') && directory.eq_ignore_ascii_case(name))
            .cloned()
    }

    /// Every existing candidate directory from `names`, in order.
    fn directories_in(&self, names: [&str; 2]) -> Vec<String> {
        let mut found = Vec::new();
        for name in names {
            let mut candidates: Vec<String> = self
                .tree
                .directories
                .iter()
                .filter(|directory| directory.eq_ignore_ascii_case(name))
                .cloned()
                .collect();
            candidates.sort();
            found.append(&mut candidates);
        }
        found
    }

    /// Files directly inside `directory`, sorted.
    fn files_in(&self, directory: &str) -> Vec<String> {
        let mut files: Vec<String> = self
            .tree
            .files
            .iter()
            .filter(|file| file.relative.parent() == Some(directory))
            .map(|file| file.relative.to_string())
            .collect();
        files.sort();
        files
    }
}

/// Where a song's documents and audio were found.
struct SongLocation {
    id: String,
    data_dir: Option<String>,
    audio_dir: Option<String>,
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(stem, _)| stem.to_string())
        .unwrap_or_else(|| name.to_string())
}

/// Whether `name` is a song metadata or chart file, and for which variation.
///
/// Recognized forms are `<song>-metadata.json`, `<song>-metadata-<variation>.json`,
/// the same with `chart`, and the prefix-free `metadata.json`/`chart.json`.
fn classify_song_file(name: &str) -> Option<(&'static str, Option<String>)> {
    let stem = name.strip_suffix(".json")?;
    let lower = stem.to_ascii_lowercase();
    if lower == "metadata" {
        return Some(("metadata", None));
    }
    if lower == "chart" {
        return Some(("chart", None));
    }
    for role in ["metadata", "chart"] {
        let marker = format!("-{role}");
        let Some(position) = lower.rfind(&marker) else {
            continue;
        };
        if position == 0 {
            continue;
        }
        let rest = &stem[position + marker.len()..];
        if rest.is_empty() {
            return Some((role, None));
        }
        if let Some(variation) = rest.strip_prefix('-').filter(|value| !value.is_empty()) {
            return Some((role, Some(variation.to_string())));
        }
    }
    None
}

fn binary_reason(path: &str) -> UnsupportedReason {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "dll" | "ndll" | "so" | "dylib" => UnsupportedReason::NativeLibrary,
        "exe" | "msi" | "bat" | "cmd" | "sh" | "py" | "js" | "jar" => UnsupportedReason::Executable,
        _ => UnsupportedReason::Unknown,
    }
}

fn describe_reason(reason: &UnsupportedReason) -> &'static str {
    match reason {
        UnsupportedReason::NativeLibrary => "a native library",
        UnsupportedReason::Executable => "an executable or interpreter script",
        UnsupportedReason::Archive => "an archive",
        UnsupportedReason::Unknown => "an opaque binary",
    }
}

fn stringify_map(values: BTreeMap<String, Value>) -> BTreeMap<String, String> {
    values
        .into_iter()
        .map(|(key, value)| {
            let rendered = match value {
                Value::String(text) => text,
                other => other.to_string(),
            };
            (key, rendered)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// JSON shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ProjectMetadataJson {
    id: Option<String>,
    title: Option<String>,
    name: Option<String>,
    description: Option<String>,
    homepage: Option<String>,
    #[serde(alias = "api_version")]
    api_version: Option<String>,
    #[serde(alias = "mod_version")]
    mod_version: Option<String>,
    license: Option<String>,
    icon: Option<String>,
    logo: Option<String>,
    author: Option<String>,
    contributors: Vec<ContributorJson>,
    dependencies: BTreeMap<String, Value>,
    optional_dependencies: BTreeMap<String, Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ContributorJson {
    name: Option<String>,
    role: Option<String>,
    email: Option<String>,
    url: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SongMetadataJson {
    version: Option<String>,
    song_name: Option<String>,
    artist: Option<String>,
    charter: Option<String>,
    time_format: Option<String>,
    looped: Option<bool>,
    divisions: Option<f64>,
    offsets: BTreeMap<String, Value>,
    time_changes: Vec<Value>,
    play_data: Option<PlayDataJson>,
    generated_by: Option<String>,
    difficulties: Vec<String>,
    characters: Option<SongCharactersJson>,
    stage: Option<String>,
    note_style: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TimeChangeJson {
    t: Option<f64>,
    bpm: Option<f64>,
    n: Option<f64>,
    d: Option<f64>,
    bt: Option<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PlayDataJson {
    album: Option<String>,
    preview_start: Option<f64>,
    preview_end: Option<f64>,
    ratings: BTreeMap<String, f64>,
    song_variations: Vec<String>,
    difficulties: Vec<String>,
    characters: Option<SongCharactersJson>,
    stage: Option<String>,
    note_style: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SongCharactersJson {
    player: Option<String>,
    girlfriend: Option<String>,
    opponent: Option<String>,
    instrumental: Option<String>,
    alt_instrumentals: Vec<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ChartJson {
    version: Option<String>,
    format: Option<String>,
    generated_by: Option<String>,
    scroll_speed: Option<Value>,
    notes: Option<Value>,
    events: Option<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NoteJson {
    t: Option<f64>,
    d: Option<f64>,
    l: Option<f64>,
    k: Option<Value>,
    p: Option<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EventJson {
    t: Option<f64>,
    e: Option<String>,
    v: Option<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CharacterJson {
    name: Option<String>,
    render_type: Option<String>,
    asset_path: Option<String>,
    scale: Option<f64>,
    is_pixel: Option<bool>,
    flip_x: Option<bool>,
    flip_y: Option<bool>,
    offsets: Option<Value>,
    camera_offsets: Option<Value>,
    dance_every: Option<f64>,
    sing_time: Option<f64>,
    starting_animation: Option<String>,
    health_icon: Option<Value>,
    death: Option<Value>,
    atlas_settings: Option<Value>,
    animations: Vec<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AnimationJson {
    name: Option<String>,
    prefix: Option<String>,
    frame_rate: Option<f64>,
    frame_indices: Option<Value>,
    looped: Option<bool>,
    offsets: Option<Value>,
    flip_x: Option<bool>,
    flip_y: Option<bool>,
    anim_type: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StageJson {
    name: Option<String>,
    version: Option<String>,
    directory: Option<String>,
    #[serde(alias = "defaultZoom")]
    camera_zoom: Option<f64>,
    is_pixel_stage: Option<bool>,
    props: Vec<Value>,
    layers: Option<Value>,
    characters: BTreeMap<String, Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StagePropJson {
    name: Option<String>,
    asset_path: Option<String>,
    position: Option<Value>,
    z_index: Option<f64>,
    scale: Option<Value>,
    alpha: Option<f64>,
    scroll: Option<Value>,
    animations: Vec<Value>,
    starting_animation: Option<String>,
    anim_type: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StageCharacterJson {
    z_index: Option<f64>,
    position: Option<Value>,
    scale: Option<f64>,
    camera_offsets: Option<Value>,
    scroll: Option<Value>,
    alpha: Option<f64>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WeekJson {
    name: Option<String>,
    story_name: Option<String>,
    songs: Vec<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn song_file_names_are_classified() {
        assert_eq!(
            classify_song_file("song-metadata.json"),
            Some(("metadata", None))
        );
        assert_eq!(
            classify_song_file("song-chart-erect.json"),
            Some(("chart", Some("erect".to_string())))
        );
        assert_eq!(
            classify_song_file("metadata.json"),
            Some(("metadata", None))
        );
        assert_eq!(
            classify_song_file("dadbattle-chart.json"),
            Some(("chart", None))
        );
        assert_eq!(classify_song_file("autoload.txt"), None);
        assert_eq!(classify_song_file("-chart.json"), None);
    }

    #[test]
    fn binary_reasons_use_extensions_only() {
        assert_eq!(
            binary_reason("lib/foo.ndll"),
            UnsupportedReason::NativeLibrary
        );
        assert_eq!(binary_reason("run.bat"), UnsupportedReason::Executable);
        assert_eq!(binary_reason("tool.bin"), UnsupportedReason::Unknown);
    }

    fn fixture() -> TempDir {
        let temp = TempDir::new("vslice-parse").unwrap();
        temp.write(
            "mod/_polymod_meta.json",
            r##"{
                "title": "Demo",
                "api_version": "0.5.0",
                "mod_version": "1.0.0",
                "contributors": [{"name": "Tester", "role": "Developer"}],
                "customFlag": true
            }"##,
        )
        .unwrap();
        temp.write(
            "mod/songs/dadbattle/dadbattle-metadata.json",
            r##"{
                "version": "2.2.3",
                "songName": "Dad Battle",
                "artist": "Kawai Sprite",
                "timeFormat": "ms",
                "timeChanges": [{"t": 0, "bpm": 180}],
                "playData": {
                    "difficulties": ["easy", "hard"],
                    "characters": {"player": "bf", "girlfriend": "gf", "opponent": "dad"},
                    "stage": "mainStage",
                    "noteStyle": "funkin"
                },
                "generatedBy": "test"
            }"##,
        )
        .unwrap();
        temp.write(
            "mod/songs/dadbattle/dadbattle-chart.json",
            r##"{
                "version": "2.0.0",
                "scrollSpeed": {"easy": 1.0, "hard": 1.4},
                "events": [{"t": 100, "e": "Zoom Camera", "v": {"zoom": 1.2}}],
                "notes": {
                    "easy": [{"t": 1000, "d": 0, "l": 0, "k": ""}],
                    "hard": [{"t": 1000, "d": 4, "l": 250}, {"t": 1500, "d": 1}]
                },
                "customField": [1, 2, 3]
            }"##,
        )
        .unwrap();
        temp.write("mod/songs/dadbattle/Inst.ogg", "audio").unwrap();
        temp
    }

    #[test]
    fn parses_metadata_and_song_content() {
        let temp = fixture();
        let discovered = crate::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_vslice_mod(&discovered);
        let module = &parsed.module;

        let metadata = module.metadata.as_ref().expect("mod metadata");
        assert_eq!(metadata.title.as_deref(), Some("Demo"));
        assert_eq!(metadata.authors, vec!["Tester"]);
        assert_eq!(metadata.api_version.as_deref(), Some("0.5.0"));
        assert!(metadata.unknown_fields.contains_key("customFlag"));

        let song = module.song("dadbattle").expect("song");
        let song_metadata = song.metadata.as_ref().expect("song metadata");
        assert_eq!(song_metadata.song_name.as_deref(), Some("Dad Battle"));
        assert_eq!(song_metadata.time_changes.len(), 1);
        assert_eq!(song_metadata.time_changes[0].bpm, Some(180.0));

        let play_data = song_metadata.play_data.as_ref().expect("play data");
        assert_eq!(play_data.difficulties, vec!["easy", "hard"]);
        assert_eq!(
            play_data.characters.as_ref().unwrap().opponent.as_deref(),
            Some("dad")
        );
        assert_eq!(play_data.stage.as_deref(), Some("mainStage"));

        let chart = song.chart_for(None).expect("chart");
        assert_eq!(chart.version.as_deref(), Some("2.0.0"));
        assert_eq!(chart.scroll_speed.get("hard"), Some(&1.4));
        assert_eq!(chart.notes["hard"].len(), 2);
        assert_eq!(chart.notes["hard"][1].lane(), Some(1));
        assert!(chart.unknown_fields.contains_key("customField"));
        assert_eq!(chart.events[0].name.as_deref(), Some("Zoom Camera"));
        assert_eq!(song.audio.len(), 1);
        assert!(!parsed.report.has_errors(), "{:?}", parsed.report);
    }

    #[test]
    fn malformed_json_is_reported_and_preserved() {
        let temp = fixture();
        temp.write("mod/songs/broken/broken-metadata.json", "{ nope")
            .unwrap();
        let discovered = crate::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_vslice_mod(&discovered);
        assert!(parsed.report.contains_code("malformed-json"));
        let preserved = parsed
            .module
            .unparsed
            .iter()
            .find(|document| document.path.ends_with("broken-metadata.json"))
            .expect("raw text retained");
        assert_eq!(preserved.contents, "{ nope");
        assert!(parsed.module.song("broken").is_some());
    }

    #[test]
    fn characters_stages_and_weeks_are_discovered() {
        let temp = fixture();
        temp.write(
            "mod/data/characters/bf.json",
            r##"{
                "name": "Boyfriend",
                "renderType": "sparrow",
                "assetPath": "characters/BOYFRIEND",
                "scale": 1,
                "animations": [
                    {"name": "idle", "prefix": "BF idle", "frameRate": 24, "looped": true},
                    {"name": "singLEFT", "prefix": "BF left", "frameRate": 24}
                ]
            }"##,
        )
        .unwrap();
        temp.write(
            "mod/data/stages/mainStage.json",
            r##"{
                "name": "Main Stage",
                "directory": "week1",
                "cameraZoom": 0.9,
                "props": [
                    {"name": "floor", "assetPath": "week1/floor", "position": [0, 0], "scroll": [1, 1]}
                ],
                "characters": {"bf": {"position": [1, 2], "scale": 1}, "dad": {"position": [3, 4]}}
            }"##,
        )
        .unwrap();
        temp.write(
            "mod/weeks/week1.json",
            r##"{"name": "Week 1", "storyName": "Daddy Dearest", "songs": ["dadbattle", {"song": "bopeebo"}]}"##,
        )
        .unwrap();

        let discovered = crate::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_vslice_mod(&discovered);
        let module = &parsed.module;

        let character = module.character("bf").expect("character");
        assert_eq!(
            character.asset_path.as_deref(),
            Some("characters/BOYFRIEND")
        );
        assert_eq!(character.animations.len(), 2);
        assert!(character.animation("singleft").is_some());
        assert_eq!(character.animations[1].frame_rate, Some(24.0));

        let stage = module.stage("mainStage").expect("stage");
        assert_eq!(stage.camera_zoom, Some(0.9));
        assert_eq!(stage.props.len(), 1);
        assert_eq!(stage.props[0].name.as_deref(), Some("floor"));
        assert_eq!(stage.characters.len(), 2);
        assert_eq!(
            stage.characters["dad"].position,
            Some(serde_json::json!([3, 4]))
        );

        let week = module.week("week1").expect("week");
        assert_eq!(week.story_name.as_deref(), Some("Daddy Dearest"));
        assert_eq!(week.songs, vec!["dadbattle", "bopeebo"]);
    }

    #[test]
    fn song_documents_and_audio_come_from_different_directories() {
        let temp = TempDir::new("vslice-split-layout").unwrap();
        temp.write("mod/_polymod_meta.json", r##"{"title": "Split Layout"}"##)
            .unwrap();
        temp.write(
            "mod/data/songs/pico/pico-metadata.json",
            r##"{"version": "2.2.4", "songName": "Pico", "timeChanges": [{"t": 0, "bpm": 150}], "playData": {"difficulties": ["hard"]}}"##,
        )
        .unwrap();
        temp.write(
            "mod/data/songs/pico/pico-chart.json",
            r##"{"version": "2.0.0", "notes": {"hard": [{"t": 500, "d": 1}]}}"##,
        )
        .unwrap();
        temp.write("mod/songs/pico/Inst.ogg", "audio").unwrap();
        temp.write("mod/songs/pico/Voices-bf.ogg", "audio").unwrap();

        let discovered = crate::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_vslice_mod(&discovered);
        let song = parsed.module.song("pico").expect("song");

        assert_eq!(song.directory, "data/songs/pico");
        assert_eq!(
            song.metadata.as_ref().unwrap().song_name.as_deref(),
            Some("Pico")
        );
        assert_eq!(song.charts.len(), 1);
        assert_eq!(song.charts[0].notes["hard"].len(), 1);
        assert_eq!(song.audio.len(), 2, "audio is read from songs/pico");
        assert!(!parsed.report.contains_code("missing-metadata"));
        assert!(!parsed.report.contains_code("missing-song-file"));
        assert!(!parsed.report.has_errors(), "{:?}", parsed.report);
    }

    #[test]
    fn a_single_note_array_is_imported_as_the_default_difficulty() {
        let temp = fixture();
        temp.write(
            "mod/songs/legacy/legacy-chart.json",
            r##"{"version": "2.0.0", "notes": [{"t": 0, "d": 0}]}"##,
        )
        .unwrap();
        let discovered = crate::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_vslice_mod(&discovered);
        let song = parsed.module.song("legacy").expect("song");
        let chart = song.chart_for(None).expect("chart");
        assert_eq!(chart.notes["default"].len(), 1);
        assert!(parsed.report.contains_code("legacy-chart-format"));
    }

    #[test]
    fn a_missing_chart_is_reported() {
        let temp = TempDir::new("vslice-nochart").unwrap();
        temp.write("mod/_polymod_meta.json", "{}").unwrap();
        temp.write(
            "mod/songs/x/x-metadata.json",
            r##"{"songName": "X", "playData": {"difficulties": ["hard"]}}"##,
        )
        .unwrap();
        let discovered = crate::discovery::open_mod_dir(&temp.join("mod")).unwrap();
        let parsed = parse_vslice_mod(&discovered);
        assert!(parsed.report.contains_code("missing-song-file"));
        assert!(parsed.report.has_errors());
    }
}
