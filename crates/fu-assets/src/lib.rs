use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn index_files(root: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_type().is_file() { files.push(entry.into_path()); }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    #[test] fn module_loads() { assert!(super::index_files(".").is_ok()); }
}
