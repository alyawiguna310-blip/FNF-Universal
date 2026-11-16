use crate::error::{VfsError, VfsResult};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A normalized, platform-independent virtual path.
///
/// Invariants:
/// - Always starts with `/`.
/// - Never ends with `/`, unless it is the root path `"/"`.
/// - Uses exclusively forward slashes (`/`).
/// - Never contains empty segments, `.` components, or `..` components.
/// - Does not contain null bytes or Windows drive letters (`:`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct VfsPath(String);

impl TryFrom<String> for VfsPath {
    type Error = VfsError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        VfsPath::new(&value)
    }
}

impl TryFrom<&str> for VfsPath {
    type Error = VfsError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        VfsPath::new(value)
    }
}

impl VfsPath {
    /// Creates and normalizes a virtual path from a string slice.
    ///
    /// Relative paths (e.g. `"images/bg.png"`) are anchored to root (e.g. `"/images/bg.png"`).
    /// Leading and trailing spaces within path segments are preserved.
    ///
    /// # Errors
    /// Returns [`VfsError::PathTraversal`] if `..` is encountered.
    /// Returns [`VfsError::InvalidPath`] if null bytes, drive colons, or invalid characters exist.
    pub fn new(raw: &str) -> VfsResult<Self> {
        if raw.contains('\0') {
            return Err(VfsError::InvalidPath("null byte in path".into()));
        }
        if raw.contains(':') {
            return Err(VfsError::InvalidPath(
                "drive letters or colons are forbidden in virtual paths".into(),
            ));
        }

        let mut segments = Vec::new();
        // Support both / and \ as separators during input normalization
        for segment in raw.split(['/', '\\']) {
            if segment.is_empty() || segment == "." {
                continue;
            }
            if segment == ".." {
                return Err(VfsError::PathTraversal(raw.to_string()));
            }
            segments.push(segment);
        }

        if segments.is_empty() {
            return Ok(Self("/".to_string()));
        }

        let mut normalized = String::with_capacity(raw.len() + 1);
        for segment in segments {
            normalized.push('/');
            normalized.push_str(segment);
        }

        Ok(Self(normalized))
    }

    /// Returns the root virtual path (`"/"`).
    pub fn root() -> Self {
        Self("/".to_string())
    }

    /// Returns the path as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Checks if this is the root path (`"/"`).
    #[inline]
    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    /// Returns the parent path, or `None` if this is already root.
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        match self.0.rfind('/') {
            Some(0) => Some(Self::root()),
            Some(idx) => Some(Self(self.0[..idx].to_string())),
            None => None,
        }
    }

    /// Returns the final file or directory name component, if not root.
    pub fn file_name(&self) -> Option<&str> {
        if self.is_root() {
            return None;
        }
        self.0.rsplit('/').next()
    }

    /// Returns the file extension, if present.
    pub fn extension(&self) -> Option<&str> {
        self.file_name().and_then(|name| {
            let (stem, ext) = name.rsplit_once('.')?;
            if stem.is_empty() {
                // Hidden dotfile like ".gitignore" has no extension
                None
            } else {
                Some(ext)
            }
        })
    }

    /// Joins a relative child path to this path.
    pub fn join(&self, child: &str) -> VfsResult<Self> {
        if child.is_empty() {
            return Ok(self.clone());
        }
        if self.is_root() {
            Self::new(child)
        } else {
            let combined = format!("{}/{}", self.0, child);
            Self::new(&combined)
        }
    }

    /// Checks whether this path starts with the given prefix path.
    pub fn starts_with(&self, base: &VfsPath) -> bool {
        if base.is_root() {
            return true;
        }
        if self.0 == base.0 {
            return true;
        }
        if let Some(rest) = self.0.strip_prefix(base.as_str()) {
            rest.starts_with('/')
        } else {
            false
        }
    }

    /// Strips a base path prefix, returning the relative subpath without leading slash.
    ///
    /// If `self` matches `base`, returns `Some("")`.
    pub fn strip_prefix<'a>(&'a self, base: &VfsPath) -> Option<&'a str> {
        if base.is_root() {
            if self.is_root() {
                return Some("");
            }
            return Some(&self.0[1..]);
        }

        if self.0 == base.0 {
            return Some("");
        }

        if let Some(rest) = self.0.strip_prefix(base.as_str()) {
            if let Some(stripped) = rest.strip_prefix('/') {
                Some(stripped)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Iterates over individual path segments.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|s| !s.is_empty())
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsPath(\"{}\")", self.0)
    }
}

impl std::ops::Deref for VfsPath {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for VfsPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_normalization() {
        assert_eq!(VfsPath::new("").unwrap().as_str(), "/");
        assert_eq!(VfsPath::new("/").unwrap().as_str(), "/");
        assert_eq!(VfsPath::new("///").unwrap().as_str(), "/");
        assert_eq!(
            VfsPath::new("images/bg.png").unwrap().as_str(),
            "/images/bg.png"
        );
        assert_eq!(
            VfsPath::new("/assets//data///stages/./castle.xml")
                .unwrap()
                .as_str(),
            "/assets/data/stages/castle.xml"
        );
        assert_eq!(
            VfsPath::new(r"assets\images\characters\bf.png")
                .unwrap()
                .as_str(),
            "/assets/images/characters/bf.png"
        );
    }

    #[test]
    fn path_preserves_spaces_in_components() {
        let p = VfsPath::new("/songs/ my song / Inst .ogg").unwrap();
        assert_eq!(p.as_str(), "/songs/ my song / Inst .ogg");
        assert_eq!(p.file_name(), Some(" Inst .ogg"));
        assert_eq!(p.extension(), Some("ogg"));
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(matches!(
            VfsPath::new("../secret"),
            Err(VfsError::PathTraversal(_))
        ));
        assert!(matches!(
            VfsPath::new("/assets/data/../../etc/passwd"),
            Err(VfsError::PathTraversal(_))
        ));
        assert!(matches!(
            VfsPath::new(r"foo\..\bar"),
            Err(VfsError::PathTraversal(_))
        ));
    }

    #[test]
    fn invalid_paths_rejected() {
        assert!(matches!(
            VfsPath::new("C:/Windows/System32"),
            Err(VfsError::InvalidPath(_))
        ));
        assert!(matches!(
            VfsPath::new("foo\0bar"),
            Err(VfsError::InvalidPath(_))
        ));
    }

    #[test]
    fn serde_deserialization_validation() {
        // Valid path deserializes and normalizes correctly
        let json_valid = r#""assets\\images\\bf.png""#;
        let path: VfsPath = serde_json::from_str(json_valid).unwrap();
        assert_eq!(path.as_str(), "/assets/images/bf.png");

        // Traversal is rejected during deserialization
        let json_traversal = r#""../../etc/passwd""#;
        assert!(serde_json::from_str::<VfsPath>(json_traversal).is_err());

        // Drive prefix is rejected during deserialization
        let json_drive = r#""C:/Windows""#;
        assert!(serde_json::from_str::<VfsPath>(json_drive).is_err());

        // Null byte is rejected during deserialization
        let json_null = r#""foo\u0000bar""#;
        assert!(serde_json::from_str::<VfsPath>(json_null).is_err());
    }

    #[test]
    fn parent_and_file_name() {
        let p = VfsPath::new("/data/stages/castle.xml").unwrap();
        assert_eq!(p.file_name(), Some("castle.xml"));
        assert_eq!(p.extension(), Some("xml"));
        assert_eq!(p.parent().unwrap().as_str(), "/data/stages");
        assert_eq!(p.parent().unwrap().parent().unwrap().as_str(), "/data");
        assert_eq!(
            p.parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .as_str(),
            "/"
        );
        assert_eq!(VfsPath::root().parent(), None);
        assert_eq!(VfsPath::root().file_name(), None);
    }

    #[test]
    fn join_and_strip_prefix() {
        let base = VfsPath::new("/assets").unwrap();
        let full = base.join("images/characters/bf.png").unwrap();
        assert_eq!(full.as_str(), "/assets/images/characters/bf.png");

        assert_eq!(full.strip_prefix(&base), Some("images/characters/bf.png"));
        assert_eq!(base.strip_prefix(&base), Some(""));
        assert_eq!(
            full.strip_prefix(&VfsPath::root()),
            Some("assets/images/characters/bf.png")
        );

        let other = VfsPath::new("/mods").unwrap();
        assert_eq!(full.strip_prefix(&other), None);
    }
}
