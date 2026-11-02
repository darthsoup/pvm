use crate::state::{self, StateLock};
use crate::utils::{Config, atomic_write, ensure_pvm_dir, pvm_dir};
use crate::version::{ReleaseLine, alias_name, single_line};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};

pub type Aliases = HashMap<String, String>;
pub fn load_aliases() -> Result<Aliases> {
    let path = pvm_dir()?.join("aliases.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Aliases::new()),
        Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
    };
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse aliases from {}", path.display()))
}
fn save_aliases(aliases: &Aliases) -> Result<()> {
    let ordered: std::collections::BTreeMap<_, _> = aliases.iter().collect();
    atomic_write(
        &ensure_pvm_dir()?.join("aliases.json"),
        &serde_json::to_vec_pretty(&ordered)?,
    )
}
pub fn set_alias(name: &str, version: &str) -> Result<()> {
    if !alias_name(name) {
        bail!(
            "Invalid alias name '{name}'. Use letters, numbers, underscores, and hyphens; start with a letter or underscore"
        );
    }
    let _lock = StateLock::acquire(false)?;
    let config = Config::load()?;
    let mut aliases = load_aliases()?;
    let value = canonical_request(version, &aliases)?;
    aliases.insert(name.to_owned(), value);
    resolve_version_or_alias(name, &aliases)?;
    save_aliases(&aliases)?;
    state::publish(&config)?;
    Ok(())
}
pub fn remove_alias(name: &str) -> Result<()> {
    let _lock = StateLock::acquire(false)?;
    let config = Config::load()?;
    let mut aliases = load_aliases()?;
    if aliases.remove(name).is_none() {
        bail!("Alias '{name}' not found");
    }
    for (other, value) in &aliases {
        if value.trim() == name {
            bail!("Alias '{other}' depends on '{name}'. Update or remove it first");
        }
    }
    save_aliases(&aliases)?;
    state::publish(&config)?;
    Ok(())
}
pub fn canonical_request(input: &str, aliases: &Aliases) -> Result<String> {
    let value = single_line(input)?;
    if aliases.contains_key(value) {
        resolve_version_or_alias(value, aliases)?;
        return Ok(value.to_owned());
    }
    Ok(value.parse::<ReleaseLine>()?.to_string())
}
pub fn resolve_version_or_alias(input: &str, aliases: &Aliases) -> Result<String> {
    let mut value = single_line(input)?;
    let mut visited = HashSet::new();
    while let Some(next) = aliases.get(value) {
        if !visited.insert(value) {
            bail!("Alias cycle detected at '{value}'");
        }
        value = single_line(next)?;
    }
    let resolved = value
        .parse::<ReleaseLine>()
        .with_context(|| {
            if visited.is_empty() {
                format!("Unknown version or alias '{input}'")
            } else {
                format!("Alias '{input}' points to missing or invalid target '{value}'")
            }
        })?
        .to_string();
    crate::output::verbose(&format!(
        "Resolved {input:?} to release line {resolved} (patch suffixes do not select an exact patch)"
    ));
    Ok(resolved)
}
pub fn names_for(version: &str, aliases: &Aliases) -> Vec<String> {
    let mut names: Vec<_> = aliases
        .keys()
        .filter(|name| resolve_version_or_alias(name, aliases).is_ok_and(|value| value == version))
        .cloned()
        .collect();
    names.sort();
    names
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_chains_and_rejects_cycles_and_missing_targets() {
        let mut aliases = Aliases::from([
            ("default".into(), "8.3.4".into()),
            ("legacy".into(), "default".into()),
        ]);
        assert_eq!(resolve_version_or_alias("legacy", &aliases).unwrap(), "8.3");
        assert_eq!(names_for("8.3", &aliases), ["default", "legacy"]);
        aliases.insert("default".into(), "legacy".into());
        assert!(
            resolve_version_or_alias("legacy", &aliases)
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
        aliases.insert("default".into(), "missing".into());
        assert!(resolve_version_or_alias("legacy", &aliases).is_err());
    }
}
