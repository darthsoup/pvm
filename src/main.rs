mod aliases;
mod cli;
mod detector;
mod doctor;
mod hooks;
mod output;
mod prompts;
mod pvmrc;
mod state;
mod switcher;
mod utils;
mod version;
mod webserver;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, FromArgMatches};
use cli::{Cli, Commands, ConfigCommand, RestartArgs, ShellChoice};
use prompts::{Prompts, TerminalPrompts};
use serde_json::json;

fn main() {
    let cli = parse_cli();
    let json_command = cli.json_command();
    output::configure(cli.verbose, cli.color);
    if let Err(error) = dispatch(cli).map(|code| {
        let code = if prompts::interrupted() { 130 } else { code };
        if code != 0 {
            std::process::exit(code);
        }
    }) {
        if let Some(command) = json_command {
            if let Err(render) = output::json(command, serde_json::Value::Null, Some(&error)) {
                output::error(&render);
            }
        } else {
            output::error(&error);
        }
        std::process::exit(
            if prompts::interrupted() || error.downcast_ref::<prompts::Interrupted>().is_some() {
                130
            } else {
                1
            },
        );
    }
}
fn parse_cli() -> Cli {
    let args: Vec<_> = std::env::args_os().collect();
    // Inspect only color spellings for Clap's early help/error rendering. The
    // actual parse remains authoritative, including all forwarded child flags.
    let mut color = clap::ColorChoice::Auto;
    let mut values = args.iter().skip(1);
    while let Some(arg) = values.next() {
        if arg == "--" {
            break;
        }
        let value = if arg == "--color" {
            values.next().and_then(|v| v.to_str())
        } else {
            arg.to_str().and_then(|v| v.strip_prefix("--color="))
        };
        if let Some(value) = value {
            color = match value {
                "always" => clap::ColorChoice::Always,
                "never" => clap::ColorChoice::Never,
                _ => clap::ColorChoice::Auto,
            };
        }
    }
    let matches = Cli::command().color(color).get_matches_from(args);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    if cli.list && cli.command.is_some() {
        Cli::command()
            .color(color)
            .error(
                clap::error::ErrorKind::ArgumentConflict,
                "--list cannot be combined with a subcommand; use pvm list",
            )
            .exit();
    }
    cli
}
fn clap_color(color: cli::ColorChoice) -> clap::ColorChoice {
    match color {
        cli::ColorChoice::Auto => clap::ColorChoice::Auto,
        cli::ColorChoice::Always => clap::ColorChoice::Always,
        cli::ColorChoice::Never => clap::ColorChoice::Never,
    }
}
fn dispatch(cli: Cli) -> Result<i32> {
    if cli.list {
        cmd_list(false)?;
        return Ok(0);
    }
    match cli.command {
        None => {
            Cli::command().color(clap_color(cli.color)).print_help()?;
            println!();
        }
        Some(Commands::List { json }) => cmd_list(json)?,
        Some(Commands::Current { json, short }) => cmd_current(json, short)?,
        Some(Commands::Which { version }) => cmd_which(version.as_deref())?,
        Some(Commands::Use { version, restart }) => {
            cmd_use(version.as_deref(), restart, &TerminalPrompts)?
        }
        Some(Commands::Alias { name, version }) => cmd_alias(name.as_deref(), version.as_deref())?,
        Some(Commands::Unalias { name }) => {
            aliases::remove_alias(&name)?;
            eprintln!("Removed alias '{name}'");
        }
        Some(Commands::Pin { version, force }) => cmd_pin(version.as_deref(), force)?,
        Some(Commands::Auto {
            silent,
            silent_if_unchanged,
            restart,
            hook,
        }) => cmd_auto(silent, silent_if_unchanged, restart, hook)?,
        Some(Commands::Exec { version, args }) => switcher::exec_version(&version, &args, false)?,
        Some(Commands::Run { version, command }) => {
            switcher::exec_version(&version, &command, true)?
        }
        Some(Commands::Info { version, json }) => cmd_info(&version, json)?,
        Some(Commands::Doctor { json }) => return cmd_doctor(json),
        Some(Commands::Restart { server }) => webserver::restart_explicit(server)?,
        Some(Commands::Config { command }) => cmd_config(command)?,
        Some(Commands::Init { shell }) => hooks::print(resolve_shell(shell)?),
        Some(Commands::Completions { shell }) => {
            let shell = match resolve_shell(shell)? {
                ShellChoice::Bash => clap_complete::Shell::Bash,
                ShellChoice::Zsh => clap_complete::Shell::Zsh,
                ShellChoice::Fish => clap_complete::Shell::Fish,
            };
            let command = Cli::command();
            // clap_complete includes hidden arguments, so remove the internal
            // protocol flag from the completion-only command model.
            let auto = command.find_subcommand("auto").unwrap();
            let public_auto = clap::Command::new("auto")
                .about(auto.get_about().unwrap().clone())
                .args(
                    auto.get_arguments()
                        .filter(|arg| !arg.is_hide_set())
                        .cloned(),
                );
            let mut command = command.mut_subcommand("auto", |_| public_auto);
            clap_complete::generate(shell, &mut command, "pvm", &mut std::io::stdout());
        }
    }
    Ok(0)
}
fn cmd_list(json_output: bool) -> Result<()> {
    let aliases = aliases::load_aliases()?;
    let versions: Vec<_> = detector::detect_php_versions()
        .iter()
        .map(|v| output::VersionRecord::new(v, &aliases))
        .collect();
    if json_output {
        output::json("list", json!({"versions": versions}), None)?;
    } else {
        output::list(&versions);
    }
    Ok(())
}
fn current() -> Result<detector::PhpVersion> {
    let path = which::which("php")
        .context("No active PHP found. Install PHP or check PATH; run pvm doctor")?;
    let mut version = detector::probe_php_binary(&path).with_context(|| {
        format!(
            "PHP in PATH cannot execute successfully: {}. Run pvm doctor",
            path.display()
        )
    })?;
    version.active = true;
    Ok(version)
}
fn cmd_current(json_output: bool, short: bool) -> Result<()> {
    let current = current()?;
    if json_output {
        output::json(
            "current",
            json!({"current": output::VersionRecord::new(&current, &aliases::load_aliases()?)}),
            None,
        )?;
    } else if short {
        println!("{}", current.version);
    } else {
        println!(
            "PHP {} ({})",
            output::style(&current.version, "1;32", false),
            current.binary_path.display()
        );
    }
    Ok(())
}
fn target(version: &str) -> Result<detector::PhpVersion> {
    let resolved = aliases::resolve_version_or_alias(version, &aliases::load_aliases()?)?;
    Ok(switcher::find_target(&detector::detect_php_versions(), &resolved)?.clone())
}
fn cmd_which(version: Option<&str>) -> Result<()> {
    let path = match version {
        Some(version) => target(version)?.binary_path,
        None => which::which("php").context("php not found in PATH")?,
    };
    println!("{}", path.display());
    Ok(())
}
fn cmd_use(version: Option<&str>, restart: RestartArgs, prompts: &impl Prompts) -> Result<()> {
    let chosen = match version {
        Some(version) => version.to_owned(),
        None => {
            if !prompts::interactive() {
                bail!(
                    "Version selection requires a terminal. Pass a version explicitly: pvm use 8.3"
                );
            }
            let versions = detector::detect_php_versions();
            if versions.is_empty() {
                bail!("No PHP versions found. Install PHP using Homebrew or your package manager");
            }
            let labels = versions
                .iter()
                .map(|v| {
                    format!(
                        "{} {} {}",
                        v.version,
                        if v.active { "[active]" } else { "" },
                        v.binary_path.display()
                    )
                })
                .collect::<Vec<_>>();
            let Some(index) = prompts.select(&labels, versions.iter().position(|v| v.active))?
            else {
                eprintln!("Selection cancelled; PHP unchanged.");
                return Ok(());
            };
            versions
                .get(index)
                .context("Invalid picker selection")?
                .version
                .clone()
        }
    };
    let result = switcher::switch_version(
        &chosen,
        &switcher::SwitchOptions {
            restart,
            update_apache: true,
            hook: false,
            quiet: false,
        },
    )?;
    render_switch(&result, false, false, None)
}
fn render_switch(
    result: &switcher::SwitchResult,
    silent: bool,
    silent_if_unchanged: bool,
    source: Option<&std::path::Path>,
) -> Result<()> {
    if !silent && (!silent_if_unchanged || result.changed) {
        let source = source
            .map(|p| format!(" from {}", p.display()))
            .unwrap_or_default();
        eprintln!(
            "{} PHP {} ({}){}",
            if result.changed {
                "Now using"
            } else {
                "Already using"
            },
            result.version,
            result.binary_path.display(),
            source
        );
        if result.apache != "skipped" {
            eprintln!("Apache configuration: {}", result.apache);
        }
        for outcome in &result.restarts {
            eprintln!("{}: {}", outcome.server.name(), outcome.status);
            if let Some(notice) = &outcome.notice {
                eprintln!("  {notice}");
            }
        }
    }
    if !result.errors.is_empty() {
        bail!(
            "PHP {} is active, but post-switch actions failed: {}",
            result.version,
            result.errors.join("; ")
        );
    }
    Ok(())
}
fn cmd_alias(name: Option<&str>, value: Option<&str>) -> Result<()> {
    if let (Some(name), Some(value)) = (name, value) {
        aliases::set_alias(name, value)?;
        eprintln!("Alias {name} → {value}");
        return Ok(());
    }
    let aliases = aliases::load_aliases()?;
    if let Some(name) = name {
        let value = aliases
            .get(name)
            .with_context(|| format!("Alias '{name}' not found"))?;
        println!("{name} → {value}");
    } else {
        let mut values: Vec<_> = aliases.iter().collect();
        values.sort_by_key(|(name, _)| *name);
        if values.is_empty() {
            println!("No aliases defined. Use pvm alias default 8.3");
        }
        for (name, value) in values {
            println!("{name} → {value}");
        }
    }
    Ok(())
}
fn cmd_pin(version: Option<&str>, force: bool) -> Result<()> {
    let _lock = state::StateLock::acquire(false)?;
    let pin = match version {
        Some(value) => aliases::canonical_request(value, &aliases::load_aliases()?)?,
        None => current()?.version,
    };
    let cwd = std::env::current_dir().context("Cannot determine current directory")?;
    let changed = pvmrc::write_pvmrc(&cwd, &pin, force)?;
    eprintln!(
        "{} PHP {pin} in {}",
        if changed { "Pinned" } else { "Already pinned" },
        cwd.join(".pvmrc").display()
    );
    Ok(())
}
fn cmd_auto(silent: bool, unchanged: bool, restart: RestartArgs, hook: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Cannot determine current directory")?;
    let Some(path) = pvmrc::find_pvmrc(&cwd) else {
        if !silent && !unchanged && !hook {
            eprintln!("No version file (.pvmrc, .php-version, .pvm) found. Use pvm pin 8.3");
        }
        return Ok(());
    };
    let version = pvmrc::read_pvmrc(&path)?;
    if version.is_empty() {
        if !silent && !unchanged && !hook {
            eprintln!("Empty {} — nothing to do", path.display());
        }
        return Ok(());
    }
    let result = switcher::switch_version(
        &version,
        &switcher::SwitchOptions {
            restart,
            update_apache: false,
            hook,
            quiet: silent,
        },
    )
    .with_context(|| format!("Activation from {} failed", path.display()))?;
    if hook {
        println!(
            "{}\t{}\t{}",
            result.token,
            result.version,
            u8::from(result.changed)
        );
        Ok(())
    } else {
        render_switch(&result, silent, unchanged, Some(&path))
    }
}
fn cmd_info(version: &str, json_output: bool) -> Result<()> {
    let version = target(version)?;
    let record = output::VersionRecord::new(&version, &aliases::load_aliases()?);
    let (ini_path, scan_dir) = detector::probe_ini(&version.binary_path);
    let build_date = detector::get_build_date(&version.binary_path);
    let modules = detector::get_loaded_modules(&version.binary_path);
    if json_output {
        let mut info = serde_json::to_value(&record)?;
        let object = info.as_object_mut().unwrap();
        object.insert("ini_path".into(), json!(ini_path));
        object.insert("scan_dir".into(), json!(scan_dir));
        object.insert("build_date".into(), json!(build_date));
        object.insert("modules".into(), json!(modules));
        output::json("info", json!({"version": info}), None)?;
    } else {
        println!("{}", version.full_version);
        println!("Binary:     {}", version.binary_path.display());
        println!(
            "Status:     {}",
            if version.active { "active" } else { "inactive" }
        );
        println!("Aliases:    {}", record.aliases.join(", "));
        println!("php.ini:    {}", ini_path.as_deref().unwrap_or("unknown"));
        println!("Scan dir:   {}", scan_dir.as_deref().unwrap_or("unknown"));
        println!("Build date: {}", build_date.as_deref().unwrap_or("unknown"));
        println!(
            "Modules:    {}",
            modules
                .map(|m| m.join(", "))
                .unwrap_or_else(|| "unknown".into())
        );
    }
    Ok(())
}
fn cmd_doctor(json_output: bool) -> Result<i32> {
    let report = doctor::inspect();
    if json_output {
        output::json("doctor", &report, None)?;
    } else {
        for check in &report.checks {
            let (label, color) = match check.status {
                "passed" => ("OK", "32"),
                "warning" => ("WARN", "33"),
                _ => ("FAIL", "31"),
            };
            println!("{} {}", output::style(label, color, false), check.message);
            if let Some(remediation) = &check.remediation {
                println!("  {remediation}");
            }
        }
        println!(
            "{} passed, {} warnings, {} failed",
            report.summary.passed, report.summary.warning, report.summary.failed
        );
    }
    Ok(i32::from(report.summary.failed > 0))
}
fn cmd_config(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Get { key } => {
            let effective = utils::Config::load()?.effective();
            if let Some(key) = key {
                println!(
                    "{}",
                    effective
                        .get(&key)
                        .with_context(|| format!("Unknown configuration key '{key}'"))?
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| effective[&key].to_string())
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&effective)?);
            }
        }
        ConfigCommand::Set { key, value } => {
            let _lock = state::StateLock::acquire(false)?;
            let mut config = utils::Config::load()?;
            config.set(&key, &value)?;
            config.save()?;
            state::publish(&config)?;
            eprintln!("{key} = {value}");
        }
    }
    Ok(())
}
fn resolve_shell(shell: Option<ShellChoice>) -> Result<ShellChoice> {
    shell
        .or_else(|| {
            match std::env::var_os("SHELL")
                .as_deref()
                .and_then(|s| std::path::Path::new(s).file_name())
                .and_then(|s| s.to_str())
            {
                Some("bash") => Some(ShellChoice::Bash),
                Some("zsh") => Some(ShellChoice::Zsh),
                Some("fish") => Some(ShellChoice::Fish),
                _ => None,
            }
        })
        .context("Could not detect shell. Pass --shell bash|zsh|fish explicitly")
}
