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
pub fn list(versions: &[VersionRecord]) {
    if versions.is_empty() {
        println!("No PHP versions found. Install PHP using Homebrew or your package manager.");
        return;
    }
    if !std::io::stdout().is_terminal() {
        println!("VERSION\tSTATUS\tALIASES\tPATH");
        for v in versions {
            println!(
                "{}\t{}\t{}\t{}",
                v.version,
                if v.active { "active" } else { "inactive" },
                v.aliases.join(","),
                v.path
            );
        }
        return;
    }
    let width = dialoguer::console::Term::stdout().size().1;
    if width < 80 {
        for v in versions {
            println!(
                "{}  {}",
                style(&v.version, "1", false),
                if v.active { "active" } else { "inactive" }
            );
            println!("  {}", v.path);
            if !v.aliases.is_empty() {
                println!("  Aliases: {}", v.aliases.join(", "));
            }
        }
    } else {
        use comfy_table::{ContentArrangement, Table};
        let mut table = Table::new();
        table
            .set_header(["VERSION", "STATUS", "ALIASES", "PATH"])
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(width);
        for v in versions {
            table.add_row([
                v.version.clone(),
                if v.active { "active" } else { "inactive" }.into(),
                v.aliases.join(", "),
                v.path.to_string(),
            ]);
        }
        println!("{table}");
    }
}
