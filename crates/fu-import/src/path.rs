//! Safe, platform-independent handling of mod-relative paths.
//!
//! Imported archives are untrusted input. Every path taken from mod content is
//! parsed into a [`SafeRelativePath`], which rejects absolute paths, drive
//! letters, UNC prefixes, parent traversal, NUL bytes, NTFS alternate data
//! streams and Windows device names. Files are then opened only after a
//! canonicalized containment check against the selected mod root, which also
//! prevents symlink escapes.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Maximum number of path segments accepted in a mod-relative path.
pub const MAX_RELATIVE_DEPTH: usize = 64;

/// Maximum accepted length, in bytes, of a mod-relative path.
pub const MAX_RELATIVE_LENGTH: usize = 1024;

/// Default ceiling for text files read during import.
pub const MAX_IMPORT_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Reasons a path is not acceptable for import.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathError {
    /// The path was empty or whitespace only.
    #[error("path is empty")]
    Empty,
    /// The path was absolute or UNC-prefixed.
    #[error("absolute paths are not allowed: {0}")]
    Absolute(String),
    /// The path began with a drive letter or drive-relative prefix.
    #[error("drive-letter paths are not allowed: {0}")]
    DriveLetter(String),
    /// The path contained a NUL byte.
    #[error("path contains a NUL byte")]
    NullByte,
    /// The path contained a `..` component.
    #[error("path escapes the mod root: {0}")]
    Traversal(String),
    /// The path nested deeper than [`MAX_RELATIVE_DEPTH`].
    #[error("path is nested too deeply: {0}")]
    TooDeep(String),
    /// The path exceeded [`MAX_RELATIVE_LENGTH`].
    #[error("path is too long: {0}")]
    TooLong(String),
    /// A segment contained characters that are never valid for import.
    #[error("path segment is not allowed: {0}")]
    InvalidSegment(String),
    /// The file exceeded the configured size limit.
    #[error("{path} is {bytes} bytes, above the {limit} byte import limit")]
    TooLarge {
        /// Offending relative path.
        path: String,
        /// Observed size in bytes.
        bytes: u64,
        /// Configured limit.
        limit: u64,
    },
    /// The file was not valid UTF-8.
    #[error("{0} is not valid UTF-8")]
    NotUtf8(String),
    /// Underlying filesystem failure.
    #[error("i/o error: {0}")]
    Io(String),
}

impl From<io::Error> for PathError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// A normalized, forward-slash separated path that is guaranteed to stay
/// inside the mod root once joined to it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafeRelativePath(String);

impl SafeRelativePath {
    /// Parses and normalizes `input`.
    ///
    /// Both `/` and `\` are treated as separators on every platform so that a
    /// Windows-authored mod cannot smuggle a separator past a Unix check (or
    /// the reverse). Empty segments and `.` segments are dropped.
    pub fn parse(input: impl AsRef<str>) -> Result<Self, PathError> {
        let raw = input.as_ref();
        if raw.contains('\0') {
            return Err(PathError::NullByte);
        }

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(PathError::Empty);
        }
        if trimmed.len() > MAX_RELATIVE_LENGTH {
            return Err(PathError::TooLong(trimmed.to_string()));
        }

        let bytes = trimmed.as_bytes();
        if bytes[0] == b'/' || bytes[0] == b'\\' {
            return Err(PathError::Absolute(trimmed.to_string()));
        }
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(PathError::DriveLetter(trimmed.to_string()));
        }

        let mut segments: Vec<&str> = Vec::new();
        for segment in trimmed.split(['/', '\\']) {
            let segment = segment.trim();
            if segment.is_empty() || segment == "." {
                continue;
            }
            if segment == ".." {
                return Err(PathError::Traversal(trimmed.to_string()));
            }
            if segment.len() > 255 {
                return Err(PathError::TooLong(trimmed.to_string()));
            }
            validate_segment(segment)?;
            segments.push(segment);
        }

        if segments.is_empty() {
            return Err(PathError::Empty);
        }
        if segments.len() > MAX_RELATIVE_DEPTH {
            return Err(PathError::TooDeep(trimmed.to_string()));
        }

        Ok(Self(segments.join("/")))
    }

    /// The normalized path as a borrowed string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The normalized path, consuming the wrapper.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Final path segment.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// Everything before the final segment, if any.
    pub fn parent(&self) -> Option<&str> {
        self.0.rfind('/').map(|index| &self.0[..index])
    }

    /// File name without its final extension.
    pub fn file_stem(&self) -> &str {
        let name = self.file_name();
        match name.rfind('.') {
            Some(index) if index > 0 => &name[..index],
            _ => name,
        }
    }

    /// Lowercase extension without the leading dot.
    pub fn extension(&self) -> Option<String> {
        let name = self.file_name();
        let (_, extension) = name.rsplit_once('.')?;
        if extension.is_empty() {
            return None;
        }
        Some(extension.to_ascii_lowercase())
    }

    /// True when the path is exactly `other` or lives below it.
    pub fn starts_with(&self, other: &str) -> bool {
        self.0 == other
            || (self.0.starts_with(other)
                && self
                    .0
                    .as_bytes()
                    .get(other.len())
                    .is_some_and(|separator| *separator == b'/'))
    }

    /// Appends a child path.
    pub fn join(&self, child: impl AsRef<str>) -> Result<Self, PathError> {
        let child = child.as_ref();
        if child.contains('\0') {
            return Err(PathError::NullByte);
        }
        let combined = if child.is_empty() {
            self.0.clone()
        } else {
            format!("{}/{}", self.0, child.trim_matches(['/', '\\']))
        };
        Self::parse(combined)
    }

    /// Converts to a host [`PathBuf`] using the platform separator.
    pub fn to_path_buf(&self) -> PathBuf {
        let mut buffer = PathBuf::new();
        for segment in self.0.split('/') {
            buffer.push(segment);
        }
        buffer
    }
}

impl fmt::Display for SafeRelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SafeRelativePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

fn validate_segment(segment: &str) -> Result<(), PathError> {
    if segment.contains(':') {
        // Rejects NTFS alternate data streams such as `file.png:hidden`.
        return Err(PathError::InvalidSegment(segment.to_string()));
    }
    if segment.chars().any(|character| character.is_control()) {
        return Err(PathError::InvalidSegment(segment.to_string()));
    }
    if is_windows_device_name(segment) {
        return Err(PathError::InvalidSegment(segment.to_string()));
    }
    Ok(())
}

fn is_windows_device_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    if stem.len() == 4 {
        let (prefix, digit) = stem.split_at(3);
        if matches!(prefix, "COM" | "LPT") && digit.as_bytes()[0].is_ascii_digit() {
            return true;
        }
    }
    false
}

/// Removes the Windows `\\?\` verbatim prefix so canonical paths compare and
/// display the same way as user-supplied paths.
pub fn simplify_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        if let Some(unc) = rest.strip_prefix(r"UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// Canonicalizes `root`, simplifies it, and verifies it is a directory.
pub fn canonical_root(root: &Path) -> Result<PathBuf, PathError> {
    let canonical = simplify_path(&std::fs::canonicalize(root)?);
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_dir() {
        return Err(PathError::Io(format!(
            "{} is not a directory",
            canonical.display()
        )));
    }
    Ok(canonical)
}

/// Joins `relative` to an already-canonical `root` without touching the disk.
///
/// The result is inside `root` by construction because `relative` was validated
/// by [`SafeRelativePath::parse`]. Symlinks are handled by
/// [`resolve_existing_within`].
pub fn resolve(root: &Path, relative: &SafeRelativePath) -> PathBuf {
    root.join(relative.to_path_buf())
}

/// Resolves `relative` and proves, after canonicalization, that the real target
/// is still inside `root`. Used for every file that is actually opened.
pub fn resolve_existing_within(
    root: &Path,
    relative: &SafeRelativePath,
) -> Result<PathBuf, PathError> {
    let candidate = simplify_path(&std::fs::canonicalize(resolve(root, relative))?);
    if !candidate.starts_with(root) {
        return Err(PathError::Traversal(relative.to_string()));
    }
    Ok(candidate)
}

/// Renders `path` relative to `root` using forward slashes, for diagnostics.
///
/// Paths outside `root` are rendered in full so a rejection is still readable.
pub fn display_relative(root: &Path, path: &Path) -> String {
    let simplified = simplify_path(path);
    match simplified.strip_prefix(root) {
        Ok(rest) => rest
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => simplified.to_string_lossy().replace('\\', "/"),
    }
}

/// Reads a UTF-8 text file from inside `root`, enforcing the size ceiling.
///
/// This is the only supported way for importers to read content: it validates
/// the relative path, canonicalizes the target, rejects escapes, refuses
/// non-files and oversized files, and never executes anything.
pub fn read_text_within(
    root: &Path,
    relative: &SafeRelativePath,
    max_bytes: u64,
) -> Result<String, PathError> {
    let absolute = resolve_existing_within(root, relative)?;
    let metadata = std::fs::metadata(&absolute)?;
    if !metadata.is_file() {
        return Err(PathError::Io(format!("{relative} is not a regular file")));
    }
    if metadata.len() > max_bytes {
        return Err(PathError::TooLarge {
            path: relative.to_string(),
            bytes: metadata.len(),
            limit: max_bytes,
        });
    }
    let bytes = std::fs::read(&absolute)?;
    String::from_utf8(bytes).map_err(|_| PathError::NotUtf8(relative.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_separators_and_dot_segments() {
        let path = SafeRelativePath::parse(r"songs\black-humor//./charts\hard.json").unwrap();
        assert_eq!(path.as_str(), "songs/black-humor/charts/hard.json");
        assert_eq!(path.file_name(), "hard.json");
        assert_eq!(path.file_stem(), "hard");
        assert_eq!(path.parent(), Some("songs/black-humor/charts"));
        assert_eq!(path.extension().as_deref(), Some("json"));
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for input in [
            "../secret",
            "a/../../secret",
            r"..\secret",
            "/etc/passwd",
            r"C:\Windows\System32",
            "C:relative",
            r"\\server\share",
            "a/\0/b",
            "images/CON.png",
            "images/COM1.png",
            "images/a.png:stream",
        ] {
            assert!(
                SafeRelativePath::parse(input).is_err(),
                "expected rejection: {input:?}"
            );
        }
    }

    #[test]
    fn keeps_legal_mod_folder_names() {
        let path = SafeRelativePath::parse("Vs Var Overhaul [Demo]").unwrap();
        assert_eq!(path.as_str(), "Vs Var Overhaul [Demo]");
        assert_eq!(
            SafeRelativePath::parse("images/characters/Auxiliary.png")
                .unwrap()
                .as_str(),
            "images/characters/Auxiliary.png"
        );
    }

    #[test]
    fn rejects_empty_and_oversized_paths() {
        assert_eq!(SafeRelativePath::parse("   "), Err(PathError::Empty));
        let deep = vec!["a"; MAX_RELATIVE_DEPTH + 1].join("/");
        assert!(matches!(
            SafeRelativePath::parse(deep),
            Err(PathError::TooDeep(_))
        ));
    }

    #[test]
    fn prefix_matching_respects_segment_boundaries() {
        let path = SafeRelativePath::parse("songs/abc/charts/hard.json").unwrap();
        assert!(path.starts_with("songs/abc"));
        assert!(!path.starts_with("songs/ab"));
        assert!(path.starts_with("songs"));
    }

    #[test]
    fn joins_children_safely() {
        let root = SafeRelativePath::parse("songs").unwrap();
        assert_eq!(
            root.join("abc/meta.json").unwrap().as_str(),
            "songs/abc/meta.json"
        );
        assert!(root.join("../escape").is_err());
    }

    #[test]
    fn read_text_within_enforces_containment_and_limits() {
        let temp = crate::test_support::TempDir::new("path-read").unwrap();
        temp.write("mod/a.txt", "hello").unwrap();
        let root = canonical_root(&temp.path().join("mod")).unwrap();

        let good = SafeRelativePath::parse("a.txt").unwrap();
        assert_eq!(read_text_within(&root, &good, 1024).unwrap(), "hello");

        let missing = SafeRelativePath::parse("b.txt").unwrap();
        assert!(read_text_within(&root, &missing, 1024).is_err());

        let small_limit = read_text_within(&root, &good, 2);
        assert!(matches!(small_limit, Err(PathError::TooLarge { .. })));
    }

    #[test]
    fn simplify_path_strips_verbatim_prefix() {
        let simplified = simplify_path(Path::new(r"\\?\C:\mods\demo"));
        assert_eq!(simplified, PathBuf::from(r"C:\mods\demo"));
        let unc = simplify_path(Path::new(r"\\?\UNC\server\share"));
        assert_eq!(unc, PathBuf::from(r"\\server\share"));
        let plain = simplify_path(Path::new("relative/path"));
        assert_eq!(plain, PathBuf::from("relative/path"));
    }
}
