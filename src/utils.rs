use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn pvm_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".pvm"))
}

pub fn ensure_pvm_dir() -> Result<PathBuf> {
    let dir = pvm_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().context("File has no parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = std::fs::metadata(path) {
        file.as_file().set_permissions(metadata.permissions())?;
    }
    file.write_all(content)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn is_command_available(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    pub auto_switch: Option<bool>,
    pub restart_webserver: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = pvm_dir()?.join("config.json");
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
        };
        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        if !["ask", "always", "never"].contains(&config.restart_policy()) {
            bail!(
                "Invalid restart_webserver in {}: use ask, always, or never",
                path.display()
            );
        }
        Ok(config)
    }
    pub fn restart_policy(&self) -> &str {
        self.restart_webserver.as_deref().unwrap_or("ask")
    }
    pub fn auto_enabled(&self) -> bool {
        self.auto_switch.unwrap_or(true)
    }
    pub fn effective(&self) -> serde_json::Value {
        serde_json::json!({"auto_switch": self.auto_enabled(), "restart_webserver": self.restart_policy()})
    }
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "auto_switch" => {
                self.auto_switch = Some(match value {
                    "true" => true,
                    "false" => false,
                    _ => bail!("auto_switch must be true or false"),
                })
            }
            "restart_webserver" if ["ask", "always", "never"].contains(&value) => {
                self.restart_webserver = Some(value.to_owned());
            }
            "restart_webserver" => bail!("restart_webserver must be ask, always, or never"),
            _ => bail!("Unknown configuration key '{key}'. Use auto_switch or restart_webserver"),
        }
        Ok(())
    }
    pub fn save(&self) -> Result<()> {
        atomic_write(
            &ensure_pvm_dir()?.join("config.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
}

pub fn command_output(command: &mut std::process::Command) -> Result<std::process::Output> {
    if crate::prompts::interrupted() {
        return Err(crate::prompts::Interrupted.into());
    }
    crate::output::verbose(&format!("Running {command:?}"));
    let output = command
        .stdin(std::process::Stdio::inherit())
        .output()
        .with_context(|| format!("Failed to run {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} exited with {}: {}{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    crate::output::verbose(&String::from_utf8_lossy(&output.stderr));
    Ok(output)
}
