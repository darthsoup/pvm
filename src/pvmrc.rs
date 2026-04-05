use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn find_pvmrc(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(".pvmrc");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn read_pvmrc(path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content.trim().to_string())
}

pub fn write_pvmrc(dir: &Path, version: &str) -> Result<()> {
    let path = dir.join(".pvmrc");
    std::fs::write(&path, format!("{}\n", version))
        .with_context(|| format!("Failed to write {}", path.display()))
}
