use std::path::{Path, PathBuf};

pub fn ensure_dir(path: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    let path = path.as_ref();
    std::fs::create_dir_all(path)?;
    Ok(path.to_path_buf())
}

pub fn read_text(path: impl AsRef<Path>) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?)
}
