//! Format-agnostic import diagnostics.
//!
//! Every importer in this crate reports through these types. They deliberately
//! contain no knowledge of any particular mod format so that new importers can
//! share the same machine-readable output.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// How serious a single diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Nothing needs fixing; provided for context.
    Info,
    /// The content is importable but needs attention.
    Warning,
    /// The affected content cannot be imported correctly.
    Error,
}

impl Severity {
    /// Stable lowercase identifier used in reports and fixtures.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Relative ordering used to summarize a report.
    pub fn rank(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Warning => 1,
            Self::Error => 2,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How well the importer can represent a piece of imported content.
///
/// This is intentionally more precise than a boolean: a great deal of mod
/// content is representable but cannot be verified or executed faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportLevel {
    /// Fully read and represented.
    Supported,
    /// Read and preserved, but the behaviour is unverified.
    Unverified,
    /// Only partly represented; some information or behaviour is lost.
    Partial,
    /// Detected and recorded, but not supported.
    Unsupported,
}

impl SupportLevel {
    /// Stable lowercase identifier used in reports and fixtures.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unverified => "unverified",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
        }
    }

    /// Relative ordering used to summarize a report.
    ///
    /// Higher is worse. `Unverified` sorts below `Partial` because unverified
    /// content is at least represented completely.
    pub fn rank(self) -> u8 {
        match self {
            Self::Supported => 0,
            Self::Unverified => 1,
            Self::Partial => 2,
            Self::Unsupported => 3,
        }
    }

    /// Returns whichever of `self` and `other` is the weaker classification.
    pub fn worst(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

impl fmt::Display for SupportLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Enumerates every diagnostic the importers can emit.
///
/// Each kind carries a default severity and support classification so call
/// sites only supply a message and an optional path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticKind {
    /// `mods/autoload.txt` was not present.
    AutoloadMissing,
    /// `mods/autoload.txt` could not be read as UTF-8 text.
    AutoloadUnreadable,
    /// An `autoload.txt` entry has no matching directory.
    AutoloadEntryMissing,
    /// An `autoload.txt` entry is not a usable mod directory name.
    AutoloadEntryMalformed,
    /// The same mod directory is listed more than once in `autoload.txt`.
    AutoloadEntryDuplicate,
    /// The selected mod root does not exist or is not a directory.
    ModRootMissing,
    /// A symlink was found during scanning and was not followed.
    ScanSymlinkSkipped,
    /// Recursive discovery stopped at the configured depth limit.
    ScanDepthLimit,
    /// A filesystem entry could not be read during scanning.
    ScanIoError,
    /// A path is absolute, drive-qualified, traversing or otherwise unsafe.
    UnsafePath,
    /// A native library or executable was found and will never be loaded.
    UnsupportedBinary,
    /// A script was inventoried but will never be executed.
    UnsupportedScript,
    /// A compressed archive was found and is not expanded.
    UnsupportedArchive,
    /// A configuration file has a syntax problem.
    MalformedIni,
    /// An XML document has a syntax problem.
    MalformedXml,
    /// A JSON document could not be parsed.
    MalformedJson,
    /// Metadata parsed but a field has an unexpected type or value.
    MalformedMetadata,
    /// A metadata file that the format normally requires is absent.
    MissingMetadata,
    /// A section, key, attribute or property is preserved but not interpreted.
    UnknownMetadata,
    /// A referenced asset does not exist in the mod.
    MissingAsset,
    /// A referenced asset exists with different casing.
    CaseMismatch,
    /// Two files differ only by casing, which is ambiguous on some filesystems.
    DuplicateCaseName,
    /// An atlas was recognized but its contents were not decoded.
    PartialAtlas,
    /// A chart references a character that this mod does not define.
    MissingCharacterReference,
    /// A chart references a stage that this mod does not define.
    MissingStageReference,
    /// A state redirect targets a state script that this mod does not contain.
    MissingStateReference,
    /// Two definitions share an identifier that must be unique.
    DuplicateDefinition,
    /// The same state redirect is declared more than once with the same target.
    DuplicateStateRedirect,
    /// The same state redirect is declared with different targets.
    ConflictingStateRedirect,
    /// An event is used but not defined by this mod.
    UndefinedEvent,
    /// A character tag or chart position is not one the importer recognizes.
    UnknownCharacterRole,
    /// A song file that the format normally requires is absent.
    MissingSongFile,
    /// A chart is structurally invalid.
    InvalidChart,
    /// More than one chart maps to the same difficulty.
    DuplicateChartDifficulty,
    /// An unrecognized chart property is preserved verbatim.
    UnknownChartField,
    /// A legacy chart layout was detected and is only partly represented.
    LegacyChartFormat,
    /// A character animation definition is incomplete or duplicated.
    InvalidAnimation,
    /// Content is recognized but cannot be imported faithfully.
    UnsupportedFeature,
}

impl DiagnosticKind {
    /// Stable kebab-case identifier suitable for filtering and tests.
    pub fn code(self) -> &'static str {
        match self {
            Self::AutoloadMissing => "autoload-missing",
            Self::AutoloadUnreadable => "autoload-unreadable",
            Self::AutoloadEntryMissing => "autoload-entry-missing",
            Self::AutoloadEntryMalformed => "autoload-entry-malformed",
            Self::AutoloadEntryDuplicate => "autoload-entry-duplicate",
            Self::ModRootMissing => "mod-root-missing",
            Self::ScanSymlinkSkipped => "scan-symlink-skipped",
            Self::ScanDepthLimit => "scan-depth-limit",
            Self::ScanIoError => "scan-io-error",
            Self::UnsafePath => "unsafe-path",
            Self::UnsupportedBinary => "unsupported-binary",
            Self::UnsupportedScript => "unsupported-script",
            Self::UnsupportedArchive => "unsupported-archive",
            Self::MalformedIni => "malformed-ini",
            Self::MalformedXml => "malformed-xml",
            Self::MalformedJson => "malformed-json",
            Self::MalformedMetadata => "malformed-metadata",
            Self::MissingMetadata => "missing-metadata",
            Self::UnknownMetadata => "unknown-metadata",
            Self::MissingAsset => "missing-asset",
            Self::CaseMismatch => "case-mismatch",
            Self::DuplicateCaseName => "duplicate-case-name",
            Self::PartialAtlas => "partial-atlas",
            Self::MissingCharacterReference => "missing-character-reference",
            Self::MissingStageReference => "missing-stage-reference",
            Self::MissingStateReference => "missing-state-reference",
            Self::DuplicateDefinition => "duplicate-definition",
            Self::DuplicateStateRedirect => "duplicate-state-redirect",
            Self::ConflictingStateRedirect => "conflicting-state-redirect",
            Self::UndefinedEvent => "undefined-event",
            Self::UnknownCharacterRole => "unknown-character-role",
            Self::MissingSongFile => "missing-song-file",
            Self::InvalidChart => "invalid-chart",
            Self::DuplicateChartDifficulty => "duplicate-chart-difficulty",
            Self::UnknownChartField => "unknown-chart-field",
            Self::LegacyChartFormat => "legacy-chart-format",
            Self::InvalidAnimation => "invalid-animation",
            Self::UnsupportedFeature => "unsupported-feature",
        }
    }

    /// Default severity for this kind.
    pub fn severity(self) -> Severity {
        match self {
            Self::AutoloadUnreadable
            | Self::AutoloadEntryMissing
            | Self::AutoloadEntryMalformed
            | Self::ModRootMissing
            | Self::UnsafePath
            | Self::MalformedJson
            | Self::MissingSongFile
            | Self::InvalidChart => Severity::Error,

            Self::AutoloadMissing
            | Self::AutoloadEntryDuplicate
            | Self::ScanSymlinkSkipped
            | Self::ScanDepthLimit
            | Self::ScanIoError
            | Self::UnsupportedBinary
            | Self::MalformedIni
            | Self::MalformedXml
            | Self::MalformedMetadata
            | Self::MissingMetadata
            | Self::MissingAsset
            | Self::CaseMismatch
            | Self::DuplicateCaseName
            | Self::MissingCharacterReference
            | Self::MissingStageReference
            | Self::MissingStateReference
            | Self::DuplicateDefinition
            | Self::ConflictingStateRedirect
            | Self::UndefinedEvent
            | Self::LegacyChartFormat
            | Self::InvalidAnimation => Severity::Warning,

            Self::UnsupportedScript
            | Self::UnsupportedArchive
            | Self::UnknownMetadata
            | Self::PartialAtlas
            | Self::DuplicateStateRedirect
            | Self::UnknownCharacterRole
            | Self::DuplicateChartDifficulty
            | Self::UnknownChartField
            | Self::UnsupportedFeature => Severity::Info,
        }
    }

    /// Default support classification for this kind.
    pub fn support(self) -> SupportLevel {
        match self {
            Self::UnknownMetadata | Self::UnknownChartField | Self::DuplicateChartDifficulty => {
                SupportLevel::Supported
            }

            Self::MissingCharacterReference
            | Self::MissingStageReference
            | Self::MissingStateReference
            | Self::UndefinedEvent
            | Self::UnknownCharacterRole => SupportLevel::Unverified,

            Self::AutoloadMissing
            | Self::AutoloadEntryDuplicate
            | Self::ScanSymlinkSkipped
            | Self::ScanDepthLimit
            | Self::ScanIoError
            | Self::MalformedIni
            | Self::MalformedXml
            | Self::MalformedMetadata
            | Self::MissingMetadata
            | Self::MissingAsset
            | Self::CaseMismatch
            | Self::DuplicateCaseName
            | Self::DuplicateDefinition
            | Self::PartialAtlas
            | Self::DuplicateStateRedirect
            | Self::ConflictingStateRedirect
            | Self::LegacyChartFormat
            | Self::InvalidAnimation
            | Self::UnsupportedFeature => SupportLevel::Partial,

            Self::AutoloadUnreadable
            | Self::AutoloadEntryMissing
            | Self::AutoloadEntryMalformed
            | Self::ModRootMissing
            | Self::UnsafePath
            | Self::UnsupportedBinary
            | Self::UnsupportedScript
            | Self::UnsupportedArchive
            | Self::MalformedJson
            | Self::MissingSongFile
            | Self::InvalidChart => SupportLevel::Unsupported,
        }
    }
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// A single import finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// The kind of finding, which determines the stable `code()`.
    pub kind: DiagnosticKind,
    /// How serious the finding is.
    pub severity: Severity,
    /// How well the affected content is supported.
    pub support: SupportLevel,
    /// Mod-relative path or logical location the finding refers to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Human-readable description. Never contains file contents.
    pub message: String,
}

impl Diagnostic {
    /// Creates a diagnostic using the defaults for `kind`.
    pub fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            severity: kind.severity(),
            support: kind.support(),
            path: None,
            message: message.into(),
        }
    }

    /// Attaches a mod-relative path.
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Overrides the default severity.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Overrides the default support classification.
    pub fn with_support(mut self, support: SupportLevel) -> Self {
        self.support = support;
        self
    }

    /// Stable machine identifier, for example `missing-asset`.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.severity, self.code())?;
        if let Some(path) = &self.path {
            write!(f, " {path}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// Counts and overall classification for a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    /// Number of error diagnostics.
    pub errors: usize,
    /// Number of warning diagnostics.
    pub warnings: usize,
    /// Number of informational diagnostics.
    pub info: usize,
    /// Weakest support classification across all diagnostics.
    pub support: SupportLevel,
    /// Per-kind diagnostic counts, ordered by code.
    pub by_kind: BTreeMap<String, usize>,
}

/// Collects diagnostics for one import run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// What was imported, for display only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    diagnostics: Vec<Diagnostic>,
}

impl ImportReport {
    /// Creates an empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty report labelled with `target`.
    pub fn with_target(target: impl Into<String>) -> Self {
        Self {
            target: Some(target.into()),
            diagnostics: Vec::new(),
        }
    }

    /// Appends a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Appends a diagnostic built from `kind` and `message`.
    pub fn add(&mut self, kind: DiagnosticKind, message: impl Into<String>) {
        self.push(Diagnostic::new(kind, message));
    }

    /// Appends a diagnostic built from `kind`, `path` and `message`.
    pub fn add_at(
        &mut self,
        kind: DiagnosticKind,
        path: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.push(Diagnostic::new(kind, message).at(path));
    }

    /// Appends every diagnostic from `other`, keeping this report's target.
    pub fn extend(&mut self, other: ImportReport) {
        self.diagnostics.extend(other.diagnostics);
    }

    /// Appends a raw slice of diagnostics.
    pub fn extend_diagnostics(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    /// All diagnostics in insertion order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Mutable access, used by validators that stream findings.
    pub fn diagnostics_mut(&mut self) -> &mut Vec<Diagnostic> {
        &mut self.diagnostics
    }

    /// Number of diagnostics with `severity`.
    pub fn count(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == severity)
            .count()
    }

    /// True when at least one error was reported.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    /// True when no diagnostic was reported at all.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Number of collected diagnostics.
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// True when `code` was reported at least once.
    pub fn contains_code(&self, code: &str) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == code)
    }

    /// Diagnostics matching `code`.
    pub fn with_code<'a>(&'a self, code: &'a str) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        self.diagnostics
            .iter()
            .filter(move |diagnostic| diagnostic.code() == code)
    }

    /// Weakest support classification among the findings that need attention.
    ///
    /// Only warnings and errors influence the result: an informational note such
    /// as "scripts were inventoried and not executed" still carries its own
    /// `unsupported` classification, but it does not downgrade the whole import.
    /// This keeps the roll-up answering "does this mod need attention?" rather
    /// than "does any recorded content have a limitation?".
    ///
    /// Returns [`SupportLevel::Supported`] when there is nothing actionable.
    pub fn overall_support(&self) -> SupportLevel {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity != Severity::Info)
            .fold(SupportLevel::Supported, |worst, diagnostic| {
                worst.worst(diagnostic.support)
            })
    }

    /// Counts, totals and per-kind summary.
    pub fn summary(&self) -> ImportSummary {
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        for diagnostic in &self.diagnostics {
            *by_kind.entry(diagnostic.code().to_string()).or_insert(0) += 1;
        }
        ImportSummary {
            errors: self.count(Severity::Error),
            warnings: self.count(Severity::Warning),
            info: self.count(Severity::Info),
            support: self.overall_support(),
            by_kind,
        }
    }

    /// Sorts diagnostics deterministically: severity, path, code, then message.
    ///
    /// Importers call this before returning so that reports are stable across
    /// platforms and filesystem iteration order.
    pub fn sort(&mut self) {
        self.diagnostics.sort_by(|left, right| {
            right
                .severity
                .rank()
                .cmp(&left.severity.rank())
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.code().cmp(right.code()))
                .then_with(|| left.message.cmp(&right.message))
        });
    }

    /// Renders the report as plain text for CLI output and logs.
    pub fn to_text(&self) -> String {
        let summary = self.summary();
        let mut out = String::new();
        if let Some(target) = &self.target {
            out.push_str(&format!("import report: {target}\n"));
        } else {
            out.push_str("import report\n");
        }
        out.push_str(&format!(
            "  support: {}\n  errors: {}, warnings: {}, info: {}\n",
            summary.support, summary.errors, summary.warnings, summary.info
        ));
        for diagnostic in &self.diagnostics {
            out.push_str(&format!("  {diagnostic}\n"));
        }
        out
    }
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

impl IntoIterator for ImportReport {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_kind() {
        let diagnostic = Diagnostic::new(DiagnosticKind::MissingAsset, "gone").at("images/x.png");
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.support, SupportLevel::Partial);
        assert_eq!(diagnostic.code(), "missing-asset");
        assert_eq!(diagnostic.path.as_deref(), Some("images/x.png"));
    }

    #[test]
    fn summary_counts_and_reports_worst_support() {
        let mut report = ImportReport::with_target("mod");
        report.add(DiagnosticKind::UnknownChartField, "extra field");
        report.add(DiagnosticKind::MissingAsset, "missing");
        report.add(DiagnosticKind::UnsafePath, "traversal");
        let summary = report.summary();
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.warnings, 1);
        assert_eq!(summary.info, 1);
        assert_eq!(summary.support, SupportLevel::Unsupported);
        assert_eq!(summary.by_kind.get("missing-asset"), Some(&1));
        assert!(report.has_errors());
    }

    #[test]
    fn sorting_is_deterministic_and_severity_first() {
        let mut report = ImportReport::new();
        report.add(DiagnosticKind::UnknownChartField, "b");
        report.add_at(DiagnosticKind::MissingAsset, "z.png", "a");
        report.add_at(DiagnosticKind::MissingAsset, "a.png", "c");
        report.add(DiagnosticKind::UnsafePath, "escape");
        report.sort();
        let codes: Vec<_> = report.diagnostics().iter().map(|d| d.code()).collect();
        assert_eq!(
            codes,
            vec![
                "unsafe-path",
                "missing-asset",
                "missing-asset",
                "unknown-chart-field"
            ]
        );
        assert_eq!(
            report.diagnostics()[1].path.as_deref(),
            Some("a.png"),
            "equal severities sort by path"
        );
    }

    #[test]
    fn informational_findings_do_not_downgrade_the_import() {
        let mut report = ImportReport::new();
        report.add(DiagnosticKind::UnsupportedScript, "inventoried only");
        assert_eq!(report.overall_support(), SupportLevel::Supported);

        report.add(DiagnosticKind::MissingAsset, "gone");
        assert_eq!(report.overall_support(), SupportLevel::Partial);
    }

    #[test]
    fn support_levels_order_by_weakness() {
        assert_eq!(
            SupportLevel::Supported.worst(SupportLevel::Unverified),
            SupportLevel::Unverified
        );
        assert_eq!(
            SupportLevel::Partial.worst(SupportLevel::Unsupported),
            SupportLevel::Unsupported
        );
        assert_eq!(
            SupportLevel::Unsupported.worst(SupportLevel::Supported),
            SupportLevel::Unsupported
        );
    }

    #[test]
    fn text_rendering_includes_codes() {
        let mut report = ImportReport::with_target("mods/x");
        report.add_at(DiagnosticKind::MissingAsset, "images/a.png", "not found");
        let text = report.to_text();
        assert!(text.contains("import report: mods/x"));
        assert!(text.contains("[missing-asset] images/a.png: not found"));
    }

    #[test]
    fn report_round_trips_through_json() {
        let mut report = ImportReport::with_target("mod");
        report.add(DiagnosticKind::UndefinedEvent, "event");
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: ImportReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, report);
    }
}
