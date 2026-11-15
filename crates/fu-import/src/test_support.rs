//! Test scaffolding shared by unit and integration tests.
//!
//! This module is not part of the stable API. It exists so that every test can
//! build throw-away directory trees without taking a dependency on a temporary
//! directory crate.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A self-deleting temporary directory.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a unique directory under the system temporary directory.
    pub fn new(label: &str) -> io::Result<Self> {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let safe_label: String = label
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        let path = std::env::temp_dir().join(format!(
            "fu-import-{}-{}-{}-{}",
            std::process::id(),
            safe_label,
            unique,
            nanos
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// The directory path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolves a relative path inside the directory.
    pub fn join(&self, relative: &str) -> PathBuf {
        let mut buffer = self.path.clone();
        for segment in relative
            .split(['/', '\\'])
            .filter(|segment| !segment.is_empty())
        {
            buffer.push(segment);
        }
        buffer
    }

    /// Writes a UTF-8 file, creating parent directories.
    pub fn write(&self, relative: &str, contents: &str) -> io::Result<PathBuf> {
        self.write_bytes(relative, contents.as_bytes())
    }

    /// Writes a binary file, creating parent directories.
    pub fn write_bytes(&self, relative: &str, contents: &[u8]) -> io::Result<PathBuf> {
        let path = self.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
        Ok(path)
    }

    /// Creates an empty directory.
    pub fn mkdir(&self, relative: &str) -> io::Result<PathBuf> {
        let path = self.join(relative);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    /// True when a relative path exists.
    pub fn exists(&self, relative: &str) -> bool {
        self.join(relative).exists()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
