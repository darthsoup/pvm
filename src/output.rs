use crate::cli::ColorChoice;
use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);
static COLOR: AtomicU8 = AtomicU8::new(0);
pub fn configure(verbose: bool, color: ColorChoice) {
    // Decide per stream below rather than relying on Colored's stdout-only detection.
    colored::control::set_override(true);
    VERBOSE.store(verbose, Ordering::Relaxed);
    COLOR.store(
        match color {
            ColorChoice::Auto => 0,
            ColorChoice::Always => 1,
            ColorChoice::Never => 2,
        },
        Ordering::Relaxed,
    );
}
pub fn verbose(message: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        eprintln!("[pvm] {message}");
    }
}
pub fn style(text: &str, code: &str, stderr: bool) -> String {
    let enabled = match COLOR.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            std::env::var_os("NO_COLOR").is_none()
                && if stderr {
                    std::io::stderr().is_terminal()
                } else {
                    std::io::stdout().is_terminal()
                }
        }
    };
    if enabled {
        match code {
            "1" => text.bold().to_string(),
            "2" => text.dimmed().to_string(),
            "36" => text.cyan().to_string(),
            "1;36" => text.cyan().bold().to_string(),
            "1;31" => text.red().bold().to_string(),
            "1;32" => text.green().bold().to_string(),
            "31" => text.red().to_string(),
            "32" => text.green().to_string(),
            "33" => text.yellow().to_string(),
            _ => text.to_owned(),
        }
    } else {
        text.to_owned()
    }
}
pub fn error(error: &anyhow::Error) {
    eprintln!("{} {error}", style("error:", "1;31", true));
    for cause in error.chain().skip(1) {
        eprintln!("  caused by: {cause}");
    }
}
pub fn json(command: &str, data: impl Serialize, error: Option<&anyhow::Error>) -> Result<()> {
    let error = error.map(|error| {
        serde_json::json!({
            "message": error.to_string(),
            "causes": error.chain().skip(1).map(ToString::to_string).collect::<Vec<_>>()
        })
    });
    let envelope =
        serde_json::json!({"schema_version": 1, "command": command, "data": data, "error": error});
    println!("{}", serde_json::to_string(&envelope)?);
    Ok(())
}
#[derive(Debug, Serialize)]
pub struct VersionRecord {
    pub version: String,
    pub full_version: Option<String>,
    pub path: String,
    pub active: bool,
    pub aliases: Vec<String>,
}
impl VersionRecord {
    pub fn new(version: &crate::detector::PhpVersion, aliases: &crate::aliases::Aliases) -> Self {
        let full = version
            .full_version
            .strip_prefix("PHP ")
            .unwrap_or(&version.full_version);
        Self {
            version: version.version.clone(),
            full_version: (full.split('.').count() >= 3).then(|| full.to_owned()),
            path: version.binary_path.display().to_string(),
            active: version.active,
            aliases: crate::aliases::names_for(&version.version, aliases),
        }
    }
}
pub fn rule() {
    let width = dialoguer::console::Term::stdout().size().1;
    println!(
        "{}",
        style(&"─".repeat(usize::from(width.clamp(1, 58))), "2", false)
    );
}

pub fn list(versions: &[VersionRecord], aliases: &crate::aliases::Aliases) {
    if versions.is_empty() {
        println!("{}", style("No PHP versions found.", "33", false));
        println!("Install PHP using Homebrew or your package manager.");
        return;
    }
    if !std::io::stdout().is_terminal() {
        println!("VERSION\tSTATUS\tALIASES\tPATH");
        for v in versions {
            println!(
                "{}\t{}\t{}\t{}",
                style(&v.version, "1", false),
                style(
                    if v.active { "active" } else { "inactive" },
                    if v.active { "32" } else { "2" },
                    false
                ),
                style(&v.aliases.join(","), "36", false),
                style(&v.path, "2", false),
            );
        }
        return;
    }
    let narrow = dialoguer::console::Term::stdout().size().1 < 80;
    println!(
        "{}",
        style(
            &format!("PHP Versions Installed ({})", versions.len()),
            "1",
            false
        )
    );
    rule();
    for v in versions {
        let bullet = style(
            if v.active { "●" } else { "○" },
            if v.active { "1;32" } else { "2" },
            false,
        );
        let active = if v.active {
            format!(" {}", style("[active]", "32", false))
        } else {
            String::new()
        };
        let tags = v
            .aliases
            .iter()
            .map(|alias| format!(" {}", style(&format!("[{alias}]"), "2", false)))
            .collect::<String>();
        if narrow {
            println!("{bullet} {}{active}{tags}", style(&v.version, "1", false));
            println!("  {}", style(&v.path, "2", false));
        } else {
            println!(
                "{bullet} {}  {}{active}{tags}",
                style(&v.version, "1", false),
                style(&v.path, "2", false)
            );
        }
    }
    rule();
    if !aliases.is_empty() {
        let mut aliases: Vec<_> = aliases.iter().collect();
        aliases.sort_by_key(|(name, _)| *name);
        let aliases = aliases
            .iter()
            .map(|(name, version)| format!("{} → {version}", style(name, "36", false)))
            .collect::<Vec<_>>();
        println!("Aliases: {}", aliases.join(" | "));
    }
}
