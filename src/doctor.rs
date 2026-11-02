use crate::{aliases, detector, utils};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct Check {
    pub id: String,
    pub status: &'static str,
    pub message: String,
    pub remediation: Option<String>,
}
#[derive(Default, Serialize)]
pub struct Summary {
    pub passed: usize,
    pub warning: usize,
    pub failed: usize,
}
#[derive(Default, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub summary: Summary,
}
impl Report {
    fn add(&mut self, id: &str, status: &'static str, message: String, remediation: Option<&str>) {
        match status {
            "passed" => self.summary.passed += 1,
            "warning" => self.summary.warning += 1,
            _ => self.summary.failed += 1,
        }
        self.checks.push(Check {
            id: id.into(),
            status,
            message,
            remediation: remediation.map(str::to_owned),
        });
    }
}
pub fn inspect() -> Report {
    let mut report = Report::default();
    match which::which("php") {
        Ok(path) => {
            if let Some(version) = detector::probe_php_binary(&path) {
                report.add(
                    "php",
                    "passed",
                    format!("Active PHP {}: {}", version.version, path.display()),
                    None,
                );
            } else {
                report.add(
                    "php",
                    "failed",
                    format!(
                        "PHP in PATH cannot execute successfully: {}",
                        path.display()
                    ),
                    Some("Check php --version and PHP configuration, then run pvm use <version>"),
                );
            }
        }
        Err(_) => report.add(
            "php",
            "failed",
            "php not found in PATH".into(),
            Some("Install PHP or correct PATH; run pvm list"),
        ),
    }
    match utils::ensure_pvm_dir().and_then(|dir| Ok(tempfile::NamedTempFile::new_in(dir)?)) {
        Ok(_) => report.add("state", "passed", "~/.pvm/ is writable".into(), None),
        Err(e) => report.add(
            "state",
            "failed",
            format!("Cannot write PVM state: {e:#}"),
            Some("Correct permissions on ~/.pvm/"),
        ),
    }
    match utils::Config::load() {
        Ok(_) => report.add("config", "passed", "Configuration is valid".into(), None),
        Err(e) => report.add(
            "config",
            "failed",
            format!("Invalid configuration: {e:#}"),
            Some("Correct ~/.pvm/config.json; restart_webserver must be ask, always, or never"),
        ),
    }
    match aliases::load_aliases() {
        Ok(aliases) => {
            let mut names: Vec<_> = aliases.keys().collect();
            names.sort();
            for name in names {
                match aliases::resolve_version_or_alias(name, &aliases) {
                    Ok(version) => report.add(
                        &format!("alias:{name}"),
                        "passed",
                        format!("Alias {name} resolves to {version}"),
                        None,
                    ),
                    Err(e) => report.add(
                        &format!("alias:{name}"),
                        "failed",
                        format!("Invalid alias {name}: {e:#}"),
                        Some("Update or remove the alias using pvm alias or pvm unalias"),
                    ),
                }
            }
        }
        Err(e) => report.add(
            "aliases",
            "failed",
            format!("Cannot read aliases: {e:#}"),
            Some("Correct ~/.pvm/aliases.json"),
        ),
    }
    for directory in ["/usr/local/bin", "/opt/homebrew/bin"] {
        let path = Path::new(directory);
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut broken: Vec<_> = entries
                .flatten()
                .filter(|entry| {
                    entry.file_name().to_string_lossy().starts_with("php")
                        && entry.path().is_symlink()
                        && !entry.path().exists()
                })
                .map(|e| e.path())
                .collect();
            broken.sort();
            for path in broken {
                report.add(
                    "symlink",
                    "failed",
                    format!("Broken PHP symlink: {}", path.display()),
                    Some("Repair the symlink or reinstall its PHP package"),
                );
            }
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let binaries: Vec<_> = std::env::split_paths(&path)
        .map(|dir| dir.join("php"))
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .collect();
    if binaries.len() > 1 {
        report.add(
            "path",
            "warning",
            format!(
                "Multiple PHP binaries in PATH: {}",
                binaries
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("Only the first PHP in PATH is active; remove obsolete PATH entries"),
        );
    } else {
        report.add(
            "path",
            "passed",
            "No conflicting PHP binaries in PATH".into(),
            None,
        );
    }
    #[cfg(target_os = "macos")]
    if which::which("brew").is_ok() {
        match std::process::Command::new("brew")
            .args(["list", "--formula"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let formulas: Vec<_> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|v| v.starts_with("php"))
                    .map(str::to_owned)
                    .collect();
                report.add(
                    "homebrew",
                    if formulas.is_empty() {
                        "warning"
                    } else {
                        "passed"
                    },
                    format!("Homebrew PHP formulas: {}", formulas.join(", ")),
                    formulas
                        .is_empty()
                        .then_some("Install a PHP formula using Homebrew"),
                );
            }
            _ => report.add(
                "homebrew",
                "warning",
                "Could not query Homebrew".into(),
                Some("Run brew doctor"),
            ),
        }
    }
    let integration = dirs::home_dir().is_some_and(|home| {
        [".zshrc", ".bashrc", ".bash_profile", ".profile"]
            .iter()
            .any(|name| {
                std::fs::read_to_string(home.join(name)).is_ok_and(|s| s.contains("pvm init"))
            })
            || home.join(".config/fish/conf.d/pvm.fish").is_file()
    });
    report.add("shell", if integration { "passed" } else { "warning" }, if integration { "Shell integration found" } else { "Shell integration not detected" }.into(), (!integration).then_some("Add eval \"$(pvm init --shell zsh)\" to ~/.zshrc, or use the matching Bash/Fish setup in README"));
    report
}
