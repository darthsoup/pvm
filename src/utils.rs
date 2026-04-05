use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;

pub fn pvm_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".pvm"))
}

pub fn ensure_pvm_dir() -> Result<PathBuf> {
    let dir = pvm_dir()?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

pub fn horizontal_rule() {
    println!("{}", "─".repeat(58).dimmed());
}

pub fn prompt_yes_no(question: &str, default_yes: bool) -> bool {
    let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{} {} ", question, suffix);
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return default_yes;
    }

    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        return default_yes;
    }
    trimmed == "y" || trimmed == "yes"
}

pub fn prompt_pick(items: &[String], label: &str) -> Option<usize> {
    for (i, item) in items.iter().enumerate() {
        println!("  {}. {}", i + 1, item);
    }
    print!("{}: ", label);
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return None;
    }

    let trimmed = input.trim();
    if let Ok(n) = trimmed.parse::<usize>() {
        if n >= 1 && n <= items.len() {
            return Some(n - 1);
        }
    }
    items.iter().position(|item| item.starts_with(trimmed))
}

pub fn is_command_available(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    pub auto_switch: Option<bool>,
    pub restart_webserver: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = ensure_pvm_dir()?.join("config.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub fn restart_policy(&self) -> &str {
        self.restart_webserver.as_deref().unwrap_or("ask")
    }
}
