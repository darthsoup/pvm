use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::utils::ensure_pvm_dir;

pub type Aliases = HashMap<String, String>;

fn aliases_path() -> Result<std::path::PathBuf> {
    Ok(ensure_pvm_dir()?.join("aliases.json"))
}

pub fn load_aliases() -> Result<Aliases> {
    let path = aliases_path()?;
    if !path.exists() {
        return Ok(Aliases::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse aliases from {}", path.display()))
}

pub fn save_aliases(aliases: &Aliases) -> Result<()> {
    let path = aliases_path()?;
    let ordered: std::collections::BTreeMap<_, _> = aliases.iter().collect();
    let content = serde_json::to_string_pretty(&ordered)?;
    std::fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
}

pub fn set_alias(name: &str, version: &str) -> Result<()> {
    let mut aliases = load_aliases()?;
    aliases.insert(name.to_string(), version.to_string());
    save_aliases(&aliases)
}

pub fn remove_alias(name: &str) -> Result<()> {
    let mut aliases = load_aliases()?;
    if aliases.remove(name).is_none() {
        anyhow::bail!("Alias '{}' not found", name);
    }
    save_aliases(&aliases)
}

pub fn resolve_version_or_alias(input: &str, aliases: &Aliases) -> String {
    aliases
        .get(input)
        .cloned()
        .unwrap_or_else(|| input.to_string())
}
