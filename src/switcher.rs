use anyhow::{Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::aliases::{load_aliases, resolve_version_or_alias};
use crate::detector::{
    detect_php_versions, find_version, get_active_php_quick as detect_php_versions_quick,
    normalize_version_str, PhpVersion,
};
use crate::webserver::{handle_post_switch_restart, update_apache_config, RestartOpts};

#[derive(Default)]
pub struct SwitchOptions {
    pub no_restart: bool,
    pub skip: Vec<String>,
    pub update_apache: bool,
    pub silent: bool,
    pub silent_if_unchanged: bool,
}

pub fn switch_version(version_str: &str, opts: &SwitchOptions) -> Result<()> {
    let aliases = load_aliases()?;
    let resolved = resolve_version_or_alias(version_str, &aliases);
    let normalized = normalize_version_str(&resolved);

    // Fast path: one subprocess instead of N*2. If the active PHP is already
    // the requested version we can return immediately without full detection.
    if let Some((current_ver, current_path)) = detect_php_versions_quick() {
        if current_ver == normalized {
            if !opts.silent && !opts.silent_if_unchanged {
                println!(
                    "Already using PHP {} ({})",
                    current_ver,
                    current_path.display()
                );
            }
            return Ok(());
        }
    }

    let versions = detect_php_versions();
    if versions.is_empty() {
        anyhow::bail!(
            "No PHP versions found. Install PHP via Homebrew (macOS) \
             or your package manager (Linux)."
        );
    }

    let target = find_version(&versions, &normalized).ok_or_else(|| {
        let available: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        anyhow::anyhow!(
            "PHP {} not found. Available: {}",
            normalized,
            available.join(", ")
        )
    })?;

    if target.active {
        if !opts.silent && !opts.silent_if_unchanged {
            println!(
                "Already using PHP {} ({})",
                target.version,
                target.binary_path.display()
            );
        }
        return Ok(());
    }

    do_switch(target, &versions, opts)?;

    if !opts.silent {
        println!(
            "{} Now using PHP {} ({})",
            "✓".green(),
            target.version.green().bold(),
            target.binary_path.display()
        );
    }

    if opts.update_apache {
        let _ = update_apache_config(&target.version); // best-effort
    }

    handle_post_switch_restart(&RestartOpts {
        no_restart: opts.no_restart,
        skip: &opts.skip,
        silent: opts.silent || opts.silent_if_unchanged,
    })?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn do_switch(target: &PhpVersion, all_versions: &[PhpVersion], _opts: &SwitchOptions) -> Result<()> {
    if which::which("brew").is_ok() {
        for v in all_versions {
            if v.version != target.version {
                let _ = Command::new("brew")
                    .args(["unlink", &format!("php@{}", v.version)])
                    .output();
            }
        }
        // Also unlink the unversioned formula in case it's the active link
        let _ = Command::new("brew").args(["unlink", "php"]).output();

        let formula = format!("php@{}", target.version);
        let status = Command::new("brew")
            .args(["link", "--overwrite", "--force", &formula])
            .output()
            .context("Failed to run brew link")?;

        if status.status.success() {
            return Ok(());
        }

        let status2 = Command::new("brew")
            .args(["link", "--overwrite", "--force", "php"])
            .output()
            .context("Failed to run brew link php")?;

        if status2.status.success() {
            return Ok(());
        }
    }

    update_symlink(&target.binary_path)
}

#[cfg(target_os = "linux")]
fn do_switch(target: &PhpVersion, _all: &[PhpVersion], _opts: &SwitchOptions) -> Result<()> {
    if which::which("update-alternatives").is_ok() {
        let bin = target.binary_path.to_str().unwrap_or("");
        let status = Command::new("sudo")
            .args(["update-alternatives", "--set", "php", bin])
            .status();
        if let Ok(s) = status {
            if s.success() {
                return Ok(());
            }
        }
    }

    update_symlink(&target.binary_path)
}

fn update_symlink(binary_path: &Path) -> Result<()> {
    let candidates = [
        PathBuf::from("/usr/local/bin/php"),
        PathBuf::from("/opt/homebrew/bin/php"),
    ];

    for link in &candidates {
        if let Some(parent) = link.parent() {
            if !parent.exists() {
                continue;
            }
        }
        if link.exists() || link.is_symlink() {
            if let Err(e) = std::fs::remove_file(link) {
                let _ = Command::new("sudo")
                    .args(["rm", "-f", link.to_str().unwrap_or("")])
                    .status();
                let _ = e; // bind to suppress unused-result warning
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            match symlink(binary_path, link) {
                Ok(()) => return Ok(()),
                Err(_) => {
                    // Try with sudo
                    let status = Command::new("sudo")
                        .args([
                            "ln",
                            "-sf",
                            binary_path.to_str().unwrap_or(""),
                            link.to_str().unwrap_or(""),
                        ])
                        .status();
                    if let Ok(s) = status {
                        if s.success() {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "Could not update PHP symlink. Try running with sudo or use 'brew link' manually."
    )
}

pub fn exec_version(version_str: &str, args: &[String]) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("No command specified. Usage: pvm exec <version> -- <cmd> [args...]");
    }

    let aliases = load_aliases()?;
    let resolved = resolve_version_or_alias(version_str, &aliases);
    let normalized = normalize_version_str(&resolved);

    let versions = detect_php_versions();
    if versions.is_empty() {
        anyhow::bail!("No PHP versions found.");
    }

    let target = find_version(&versions, &normalized)
        .ok_or_else(|| anyhow::anyhow!("PHP {} not found", normalized))?;

    let bin_dir = target
        .binary_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid binary path: {}", target.binary_path.display()))?;

    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), current_path);

    let (cmd, cmd_args) = args.split_first().unwrap();

    let status = Command::new(cmd)
        .args(cmd_args)
        .env("PATH", &new_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to run '{}'", cmd))?;

    std::process::exit(status.code().unwrap_or(1));
}

pub fn run_version(version_str: &str, script: &str, args: &[String]) -> Result<()> {
    let mut exec_args = vec!["php".to_string(), script.to_string()];
    exec_args.extend_from_slice(args);
    exec_version(version_str, &exec_args)
}
