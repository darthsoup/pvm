mod aliases;
mod cli;
mod detector;
mod pvmrc;
mod switcher;
mod utils;
mod webserver;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;

use cli::{Cli, Commands, ShellChoice};

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "error:".red().bold(), e);
        for cause in e.chain().skip(1) {
            eprintln!("  {} {}", "caused by:".dimmed(), cause);
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.list {
        return cmd_list();
    }

    match cli.command {
        None => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
        Some(Commands::List) => cmd_list(),
        Some(Commands::Current) => cmd_current(),
        Some(Commands::Which { version }) => cmd_which(version.as_deref()),
        Some(Commands::Use {
            version,
            no_restart,
            skip,
        }) => cmd_use(version.as_deref(), no_restart, &skip),
        Some(Commands::Alias { name, version }) => cmd_alias(name.as_deref(), version.as_deref()),
        Some(Commands::Unalias { name }) => cmd_unalias(&name),
        Some(Commands::Pin { version }) => cmd_pin(version.as_deref()),
        Some(Commands::Auto {
            silent,
            silent_if_unchanged,
            no_restart,
            skip,
        }) => cmd_auto(silent, silent_if_unchanged, no_restart, &skip),
        Some(Commands::Exec { version, args }) => cmd_exec(&version, &args),
        Some(Commands::Run {
            version,
            script,
            args,
        }) => cmd_run(&version, &script, &args),
        Some(Commands::Info { version }) => cmd_info(&version),
        Some(Commands::Doctor) => cmd_doctor(),
        Some(Commands::Restart { server }) => cmd_restart(server.as_deref()),
        Some(Commands::Init { shell }) => cmd_init(shell),
        Some(Commands::Completions { shell }) => cmd_completions(shell),
    }
}

fn cmd_list() -> Result<()> {
    let versions = detector::detect_php_versions();
    let aliases = aliases::load_aliases()?;

    if versions.is_empty() {
        println!("{}", "No PHP versions found.".yellow());
        println!("Install PHP via Homebrew (macOS) or your package manager (Linux).");
        return Ok(());
    }

    println!(
        "{}",
        format!("PHP Versions Installed ({})", versions.len()).bold()
    );
    utils::horizontal_rule();

    for v in &versions {
        let bullet = if v.active {
            "●".green().bold().to_string()
        } else {
            "○".dimmed().to_string()
        };

        let alias_tags: Vec<String> = aliases
            .iter()
            .filter(|(_, ver)| {
                let resolved = aliases::resolve_version_or_alias(ver, &aliases);
                detector::normalize_version_str(&resolved) == v.version || ver.as_str() == v.version
            })
            .map(|(name, _)| format!("[{}]", name).dimmed().to_string())
            .collect();

        let active_tag = if v.active {
            format!(" {}", "[active]".green())
        } else {
            String::new()
        };

        let alias_str = if alias_tags.is_empty() {
            String::new()
        } else {
            format!(" {}", alias_tags.join(" "))
        };

        println!(
            "{} {}  {}{}{}",
            bullet,
            v.version.bold(),
            v.binary_path.display().to_string().dimmed(),
            active_tag,
            alias_str,
        );
    }

    utils::horizontal_rule();

    if !aliases.is_empty() {
        let alias_display: Vec<String> = {
            let mut sorted: Vec<_> = aliases.iter().collect();
            sorted.sort_by_key(|(k, _)| k.as_str());
            sorted
                .iter()
                .map(|(k, v)| format!("{} → {}", k.cyan(), v))
                .collect()
        };
        println!("Aliases: {}", alias_display.join(" | "));
    }

    Ok(())
}

fn cmd_current() -> Result<()> {
    let versions = detector::detect_php_versions();

    if let Some(active) = detector::get_active_version(&versions) {
        println!(
            "PHP {} ({})",
            active.version.green().bold(),
            active.binary_path.display()
        );
    } else if let Ok(php_path) = which::which("php") {
        println!("{}", php_path.display());
    } else {
        println!("{}", "No active PHP version found.".yellow());
        println!("Make sure PHP is installed and in your PATH.");
    }

    Ok(())
}

fn cmd_which(version: Option<&str>) -> Result<()> {
    match version {
        None => {
            let path = which::which("php").map_err(|_| anyhow::anyhow!("php not found in PATH"))?;
            println!("{}", path.display());
        }
        Some(v) => {
            let aliases = aliases::load_aliases()?;
            let resolved = aliases::resolve_version_or_alias(v, &aliases);
            let normalized = detector::normalize_version_str(&resolved);

            let versions = detector::detect_php_versions();
            let target = detector::find_version(&versions, &normalized).ok_or_else(|| {
                let available: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
                anyhow::anyhow!(
                    "PHP {} not found. Available: {}",
                    normalized,
                    available.join(", ")
                )
            })?;

            println!("{}", target.binary_path.display());
        }
    }
    Ok(())
}

fn cmd_use(version: Option<&str>, no_restart: bool, skip: &[String]) -> Result<()> {
    let opts = switcher::SwitchOptions {
        no_restart,
        skip: skip.to_vec(),
        update_apache: true,
        silent: false,
        silent_if_unchanged: false,
    };

    match version {
        Some(v) => switcher::switch_version(v, &opts),
        None => {
            let versions = detector::detect_php_versions();
            if versions.is_empty() {
                anyhow::bail!(
                    "No PHP versions found. Install PHP via Homebrew (macOS) \
                     or your package manager (Linux)."
                );
            }

            println!("{}", "Available PHP versions:".bold());
            let labels: Vec<String> = versions
                .iter()
                .map(|v| {
                    if v.active {
                        format!("{} {} [active]", v.version, v.binary_path.display())
                    } else {
                        format!("{} {}", v.version, v.binary_path.display())
                    }
                })
                .collect();

            let idx = utils::prompt_pick(&labels, "Enter number or version")
                .ok_or_else(|| anyhow::anyhow!("No version selected"))?;

            let selected = &versions[idx];
            switcher::switch_version(&selected.version, &opts)
        }
    }
}

fn cmd_alias(name: Option<&str>, version: Option<&str>) -> Result<()> {
    match (name, version) {
        (None, _) => {
            let aliases = aliases::load_aliases()?;
            if aliases.is_empty() {
                println!("{}", "No aliases defined.".dimmed());
                println!("Use 'pvm alias <name> <version>' to create one.");
            } else {
                println!("{}", "Aliases:".bold());
                let mut sorted: Vec<_> = aliases.iter().collect();
                sorted.sort_by_key(|(k, _)| k.as_str());
                for (name, ver) in sorted {
                    println!("  {} → {}", name.cyan().bold(), ver);
                }
            }
            Ok(())
        }
        (Some(n), None) => {
            let aliases = aliases::load_aliases()?;
            match aliases.get(n) {
                Some(v) => println!("{} → {}", n.cyan().bold(), v),
                None => anyhow::bail!("Alias '{}' not found", n),
            }
            Ok(())
        }
        (Some(n), Some(v)) => {
            aliases::set_alias(n, v)?;
            println!("{} Alias {} → {}", "✓".green(), n.cyan().bold(), v.bold());
            Ok(())
        }
    }
}

fn cmd_unalias(name: &str) -> Result<()> {
    aliases::remove_alias(name)?;
    println!("{} Removed alias '{}'", "✓".green(), name.cyan());
    Ok(())
}

fn cmd_pin(version: Option<&str>) -> Result<()> {
    let pin_version = match version {
        Some(v) => v.to_string(),
        None => {
            let versions = detector::detect_php_versions();
            let active = detector::get_active_version(&versions).ok_or_else(|| {
                anyhow::anyhow!(
                    "No active PHP version found. Pass a version explicitly: pvm pin <version>"
                )
            })?;
            active.version.clone()
        }
    };

    let cwd = std::env::current_dir().context("Cannot determine current directory")?;
    pvmrc::write_pvmrc(&cwd, &pin_version)?;
    println!(
        "{} Pinned PHP {} in {}/.pvmrc",
        "✓".green(),
        pin_version.bold(),
        cwd.display()
    );
    Ok(())
}

fn cmd_auto(
    silent: bool,
    silent_if_unchanged: bool,
    no_restart: bool,
    skip: &[String],
) -> Result<()> {
    let cwd = std::env::current_dir().context("Cannot determine current directory")?;

    let pvmrc_path = pvmrc::find_pvmrc(&cwd);

    match pvmrc_path {
        None => {
            if !silent && !silent_if_unchanged {
                println!(
                    "{}",
                    "No version file (.pvmrc, .php-version, .pvm) found in this directory or any parent.".dimmed()
                );
            }
            Ok(())
        }
        Some(path) => {
            let version_str = pvmrc::read_pvmrc(&path)?;
            if version_str.is_empty() {
                if !silent && !silent_if_unchanged {
                    println!(
                        "{}",
                        format!("Empty {} — nothing to do.", path.display()).dimmed()
                    );
                }
                return Ok(());
            }

            let opts = switcher::SwitchOptions {
                no_restart,
                skip: skip.to_vec(),
                update_apache: false,
                silent,
                silent_if_unchanged,
            };

            switcher::switch_version(&version_str, &opts)
        }
    }
}

fn cmd_exec(version: &str, args: &[String]) -> Result<()> {
    switcher::exec_version(version, args)
}

fn cmd_run(version: &str, script: &str, args: &[String]) -> Result<()> {
    switcher::run_version(version, script, args)
}

fn cmd_info(version_str: &str) -> Result<()> {
    let aliases = aliases::load_aliases()?;
    let resolved = aliases::resolve_version_or_alias(version_str, &aliases);
    let normalized = detector::normalize_version_str(&resolved);

    let versions = detector::detect_php_versions();
    let target = detector::find_version(&versions, &normalized).ok_or_else(|| {
        let available: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        anyhow::anyhow!(
            "PHP {} not found. Available: {}",
            normalized,
            available.join(", ")
        )
    })?;

    let build_date =
        detector::get_build_date(&target.binary_path).unwrap_or_else(|| "unknown".to_string());
    let modules = detector::get_loaded_modules(&target.binary_path);
    let (ini_path, scan_dir) = detector::probe_ini(&target.binary_path);

    let alias_names: Vec<String> = aliases
        .iter()
        .filter(|(_, v)| {
            let r = aliases::resolve_version_or_alias(v, &aliases);
            detector::normalize_version_str(&r) == target.version || v.as_str() == target.version
        })
        .map(|(k, _)| k.clone())
        .collect();

    let status_str = if target.active {
        format!("{} ✓", "active".green())
    } else {
        "inactive".dimmed().to_string()
    };

    println!("{}", target.full_version.bold());
    utils::horizontal_rule();
    println!(
        "  {:12} {}",
        "Binary:".dimmed(),
        target.binary_path.display()
    );
    if let Some(ref ini) = ini_path {
        println!("  {:12} {}", "php.ini:".dimmed(), ini);
    }
    if let Some(ref scan) = scan_dir {
        println!("  {:12} {}", "Scan dir:".dimmed(), scan);
    }
    println!("  {:12} {}", "Build date:".dimmed(), build_date);
    println!("  {:12} {}", "Status:".dimmed(), status_str);
    if !alias_names.is_empty() {
        println!(
            "  {:12} {}",
            "Alias:".dimmed(),
            alias_names.join(", ").cyan()
        );
    }
    utils::horizontal_rule();

    if modules.is_empty() {
        println!("{}", "Could not retrieve loaded modules.".dimmed());
    } else {
        println!("{}", format!("Loaded modules ({}):", modules.len()).bold());
        println!("  {}", modules.join(", ").dimmed());
    }

    Ok(())
}

fn cmd_doctor() -> Result<()> {
    println!("{}", "pvm doctor".bold());
    utils::horizontal_rule();

    let versions = detector::detect_php_versions();
    let active = detector::get_active_version(&versions);

    match which::which("php") {
        Ok(path) => {
            check_ok(&format!("php found in PATH: {}", path.display()));

            if let Some(av) = active {
                let path_canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                let bin_canon = std::fs::canonicalize(&av.binary_path)
                    .unwrap_or_else(|_| av.binary_path.clone());
                if path_canon == bin_canon {
                    check_ok(&format!(
                        "Active PHP is {}: {}",
                        av.version,
                        av.binary_path.display()
                    ));
                } else {
                    check_warn(&format!(
                        "PATH php ({}) differs from managed php ({})",
                        path.display(),
                        av.binary_path.display()
                    ));
                }
            } else {
                check_warn("Could not determine which pvm-managed version is active");
            }
        }
        Err(_) => {
            check_fail("php not found in PATH — install PHP or check your shell config");
        }
    }

    // 3. ~/.pvm/ is writable
    match utils::ensure_pvm_dir() {
        Ok(dir) => {
            let test_file = dir.join(".write_test");
            match std::fs::write(&test_file, "") {
                Ok(_) => {
                    let _ = std::fs::remove_file(&test_file);
                    check_ok(&format!("~/.pvm/ is writable ({})", dir.display()));
                }
                Err(_) => {
                    check_fail(&format!("~/.pvm/ is NOT writable: {}", dir.display()));
                }
            }
        }
        Err(e) => check_fail(&format!("Cannot access ~/.pvm/: {}", e)),
    }

    let bin_dirs: &[&str] = &["/usr/local/bin", "/opt/homebrew/bin"];
    for dir_str in bin_dirs {
        let dir = std::path::Path::new(dir_str);
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            let broken: Vec<_> = entries
                .flatten()
                .filter(|e| {
                    let name = e.file_name();
                    let n = name.to_string_lossy();
                    (n == "php" || n.starts_with("php"))
                        && e.path().is_symlink()
                        && !e.path().exists()
                })
                .map(|e| e.path())
                .collect();

            if broken.is_empty() {
                check_ok(&format!("No broken PHP symlinks in {}", dir_str));
            } else {
                for p in &broken {
                    check_fail(&format!("Broken symlink: {}", p.display()));
                }
            }
        }
    }

    {
        let path_val = std::env::var("PATH").unwrap_or_default();
        let php_bins: Vec<_> = path_val
            .split(':')
            .filter_map(|dir| {
                let p = std::path::Path::new(dir).join("php");
                if p.exists() {
                    Some(p)
                } else {
                    None
                }
            })
            .collect();

        if php_bins.len() <= 1 {
            check_ok("No conflicting PHP binaries in PATH");
        } else {
            check_warn(&format!(
                "{} php binaries found in PATH — only the first one is used",
                php_bins.len()
            ));
            for bin in &php_bins {
                println!("    {}", bin.display().to_string().dimmed());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if which::which("brew").is_ok() {
            let output = std::process::Command::new("brew")
                .args(["list", "--formula"])
                .output();
            match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let php_formulas: Vec<&str> =
                        stdout.lines().filter(|l| l.starts_with("php")).collect();
                    if php_formulas.is_empty() {
                        check_warn("No PHP formulas found in Homebrew");
                    } else {
                        check_ok(&format!(
                            "Homebrew PHP formula(s) found: {}",
                            php_formulas.join(", ")
                        ));
                    }
                }
                Err(_) => check_warn("Could not query Homebrew formula list"),
            }
        }
    }

    {
        let home = dirs::home_dir();
        let shell_files = [".zshrc", ".bashrc", ".bash_profile", ".profile"];
        let mut found = false;
        if let Some(home) = &home {
            for file in &shell_files {
                let path = home.join(file);
                if path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if content.contains("pvm init") {
                            check_ok(&format!("Shell integration found in ~/{}", file));
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                let fish_conf = home.join(".config/fish/conf.d/pvm.fish");
                if fish_conf.exists() {
                    check_ok("Shell integration found in ~/.config/fish/conf.d/pvm.fish");
                    found = true;
                }
            }
        }
        if !found {
            check_warn(
                "Shell integration not detected — add 'eval \"$(pvm init)\"' to your .zshrc/.bashrc, or run 'pvm init --shell fish | source' for fish",
            );
        }
    }

    utils::horizontal_rule();
    Ok(())
}

fn check_ok(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}

fn check_warn(msg: &str) {
    println!("{} {}", "⚠".yellow().bold(), msg);
}

fn check_fail(msg: &str) {
    println!("{} {}", "✗".red().bold(), msg);
}

fn cmd_restart(server: Option<&str>) -> Result<()> {
    match server {
        Some("apache") => webserver::restart_server(webserver::WebServer::Apache),
        Some("nginx") => webserver::restart_server(webserver::WebServer::Nginx),
        Some(other) => anyhow::bail!("Unknown server '{}'. Use 'apache' or 'nginx'.", other),
        None => {
            let running = webserver::detect_running_servers();
            if running.is_empty() {
                let apache = webserver::detect_apache();
                let nginx = webserver::detect_nginx();
                if !apache && !nginx {
                    println!("{}", "No supported web servers detected.".dimmed());
                } else {
                    println!("{}", "No web servers are currently running.".dimmed());
                }
                return Ok(());
            }
            for server in running {
                webserver::restart_server(server)?;
            }
            Ok(())
        }
    }
}

fn cmd_init(shell: Option<ShellChoice>) -> Result<()> {
    let chosen = shell.or_else(|| {
        let s = std::env::var("SHELL").unwrap_or_default();
        if s.contains("zsh") {
            Some(ShellChoice::Zsh)
        } else if s.contains("bash") {
            Some(ShellChoice::Bash)
        } else if s.contains("fish") {
            Some(ShellChoice::Fish)
        } else {
            None
        }
    });

    match chosen {
        Some(ShellChoice::Zsh) => print_zsh_hook(),
        Some(ShellChoice::Bash) => print_bash_hook(),
        Some(ShellChoice::Fish) => print_fish_hook(),
        None => {
            eprintln!(
                "{} Could not detect shell. Pass --shell bash|zsh|fish explicitly.",
                "warning:".yellow().bold()
            );
            print_zsh_hook();
            println!();
            println!("# --- Bash hook ---");
            println!();
            print_bash_hook();
        }
    }

    Ok(())
}

fn cmd_completions(shell: Option<ShellChoice>) -> Result<()> {
    use clap::CommandFactory;
    use clap_complete::{generate, Shell as ClapShell};

    let chosen = shell.or_else(|| {
        let s = std::env::var("SHELL").unwrap_or_default();
        if s.contains("zsh") {
            Some(ShellChoice::Zsh)
        } else if s.contains("bash") {
            Some(ShellChoice::Bash)
        } else if s.contains("fish") {
            Some(ShellChoice::Fish)
        } else {
            None
        }
    });

    let clap_shell = match chosen {
        Some(ShellChoice::Bash) => ClapShell::Bash,
        Some(ShellChoice::Zsh) => ClapShell::Zsh,
        Some(ShellChoice::Fish) => ClapShell::Fish,
        None => anyhow::bail!("Could not detect shell. Pass --shell bash|zsh|fish explicitly."),
    };

    let mut cmd = Cli::command();
    generate(clap_shell, &mut cmd, "pvm", &mut std::io::stdout());
    Ok(())
}

fn print_zsh_hook() {
    println!(
        r#"# pvm shell integration (zsh)
# Add the following line to your ~/.zshrc:
#   eval "$(pvm init)"

autoload -U add-zsh-hook

_pvm_auto_switch() {{
  if command -v pvm &>/dev/null; then
    pvm auto --silent-if-unchanged 2>/dev/null
  fi
}}

add-zsh-hook chpwd _pvm_auto_switch
_pvm_auto_switch  # run once on shell start"#
    );
}

fn print_bash_hook() {
    println!(
        r#"# pvm shell integration (bash)
# Add the following line to your ~/.bashrc or ~/.bash_profile:
#   eval "$(pvm init)"

_pvm_auto_switch() {{
  if command -v pvm &>/dev/null; then
    pvm auto --silent-if-unchanged 2>/dev/null
  fi
}}

if [[ -z "$PROMPT_COMMAND" ]]; then
  PROMPT_COMMAND="_pvm_auto_switch"
else
  PROMPT_COMMAND="_pvm_auto_switch;$PROMPT_COMMAND"
fi
_pvm_auto_switch  # run once on shell start"#
    );
}

fn print_fish_hook() {
    println!(
        r#"# pvm shell integration (fish)
# Save this to ~/.config/fish/conf.d/pvm.fish
# or run: pvm init --shell fish | source

function _pvm_auto_switch --on-variable PWD
    if command -v pvm > /dev/null 2>&1
        pvm auto --silent-if-unchanged 2>/dev/null
    end
end

_pvm_auto_switch"#
    );
}
