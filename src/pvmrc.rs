use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const VERSION_FILES: &[&str] = &[".pvmrc", ".php-version", ".pvm"];

pub fn find_pvmrc(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        for name in VERSION_FILES {
            let candidate = current.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
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

pub fn write_pvmrc(dir: &Path, version: &str, force: bool) -> Result<bool> {
    let path = dir.join(".pvmrc");
    if path.exists() {
        let previous = read_pvmrc(&path)?;
        if previous == version {
            return Ok(false);
        }
        if !force {
            anyhow::bail!(
                "{} already pins '{}'. Use pvm pin --force {} to replace it",
                path.display(),
                previous,
                version
            );
        }
    }
    crate::utils::atomic_write(&path, format!("{version}\n").as_bytes())?;
    Ok(true)
}
