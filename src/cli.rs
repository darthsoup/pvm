use clap::{Args, Parser, Subcommand, ValueEnum};
use std::ffi::OsString;

#[derive(ValueEnum, Clone, Debug)]
pub enum ShellChoice {
    Bash,
    Zsh,
    Fish,
}
#[derive(ValueEnum, Clone, Copy, Debug, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Parser)]
#[command(name = "pvm", version, help_template = ROOT_HELP, about = "PHP Version Manager — global switching with project version files",
    after_help = "Examples:\n  pvm use 8.3\n  pvm pin 8.3\n  pvm exec 8.3 -- php artisan migrate\n  eval \"$(pvm init --shell zsh)\"\n\nSwitching is global and can affect other terminals. Patch suffixes select a major/minor release line.")]
pub struct Cli {
    /// List installed PHP versions (alias for list)
    #[arg(short = 'l', long = "list")]
    pub list: bool,
    /// Explain version resolution, discovery, and switching on stderr
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
    /// Control color (explicit choice overrides NO_COLOR)
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub color: ColorChoice,
    #[command(subcommand)]
    pub command: Option<Commands>,
}
#[derive(Args, Clone, Default, Debug)]
pub struct RestartArgs {
    /// Restart running web servers without prompting
    #[arg(long, conflicts_with = "no_restart")]
    pub restart: bool,
    /// Skip all web server restarts
    #[arg(long)]
    pub no_restart: bool,
    /// Skip this server, even with --restart
    #[arg(long, value_enum, value_name = "SERVER")]
    pub skip: Vec<crate::webserver::WebServer>,
}
#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Show effective settings, or one setting
    Get { key: Option<String> },
    /// Set auto_switch (true/false) or restart_webserver (ask/always/never)
    Set { key: String, value: String },
}
#[derive(Subcommand)]
pub enum Commands {
    /// List installed PHP versions
    #[command(visible_alias = "ls")]
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show the active PHP version and binary path
    Current {
        #[arg(long, conflicts_with = "short")]
        json: bool,
        /// Print only the major/minor release line
        #[arg(long)]
        short: bool,
    },
    /// Print only the binary path for the active PHP, a version, or an alias
    Which {
        /// Installed release line or defined alias (omitting it uses PATH)
        version: Option<String>,
    },
    /// Switch global PHP (keyboard picker when VERSION is omitted)
    Use {
        /// Release line or defined alias, such as 8.3 or default
        version: Option<String>,
        #[command(flatten)]
        restart: RestartArgs,
    },
    /// Show information about an installed version or alias
    Info {
        /// Installed release line or defined alias
        version: String,
        #[arg(long)]
        json: bool,
    },
    /// Manage named aliases (no arguments lists aliases)
    Alias {
        name: Option<String>,
        version: Option<String>,
    },
    /// Remove an alias (dependent aliases must be removed first)
    Unalias { name: String },
    /// Write .pvmrc; valid versions need not be installed
    Pin {
        /// Version or alias (defaults to active PHP)
        version: Option<String>,
        /// Replace a different existing .pvmrc
        #[arg(long)]
        force: bool,
    },
    /// Read the nearest .pvmrc, .php-version, or .pvm and switch global PHP
    Auto {
        /// Suppress normal output; retain errors
        #[arg(long, conflicts_with = "silent_if_unchanged")]
        silent: bool,
        /// Suppress normal output when no switch is needed
        #[arg(long)]
        silent_if_unchanged: bool,
        #[command(flatten)]
        restart: RestartArgs,
        #[arg(long, hide = true)]
        hook: bool,
    },
    /// Print shell integration; reload your shell after upgrading PVM
    Init {
        #[arg(long, value_enum)]
        shell: Option<ShellChoice>,
    },
    /// Run a command with a version in PATH, without switching global PHP
    #[command(after_help = "Example: pvm exec 8.3 -- php artisan migrate")]
    Exec {
        /// Installed release line or defined alias
        version: String,
        #[arg(last = true, required = true)]
        args: Vec<OsString>,
    },
    /// Run a PHP script; every argument after SCRIPT is forwarded
    #[command(
        after_help = "Example: pvm run -v 8.3 index.php --filter=users\nPVM options must precede SCRIPT.",
        override_usage = "pvm run [OPTIONS] <VERSION> <SCRIPT> [ARGS]..."
    )]
    Run {
        /// Installed release line or defined alias
        version: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1.., required = true, value_name = "SCRIPT")]
        command: Vec<OsString>,
    },
    /// Check configuration; failed checks return status 1
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Restart one server, or all running supported servers
    Restart {
        #[arg(value_enum)]
        server: Option<crate::webserver::WebServer>,
    },
    /// Read or update configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print shell completions to stdout
    Completions {
        #[arg(long, value_enum)]
        shell: Option<ShellChoice>,
    },
}
impl Cli {
    pub fn json_command(&self) -> Option<&'static str> {
        match self.command {
            Some(Commands::List { json: true }) => Some("list"),
            Some(Commands::Current { json: true, .. }) => Some("current"),
            Some(Commands::Info { json: true, .. }) => Some("info"),
            Some(Commands::Doctor { json: true }) => Some("doctor"),
            _ => None,
        }
    }
}
const ROOT_HELP: &str = "{about}\n\nUsage: {usage}\n\nEveryday commands:\n  list (ls)    List installed versions\n  current      Show active PHP\n  which        Print a binary path\n  use          Switch global PHP\n  info         Show PHP details\n\nProject setup:\n  alias        Manage named aliases\n  unalias      Remove an alias\n  pin          Write .pvmrc\n  auto         Activate the nearest project version\n  init         Print shell integration\n\nExecution:\n  exec         Run a command with a PHP version\n  run          Run a PHP script with a version\n\nMaintenance:\n  doctor       Diagnose configuration\n  restart      Restart web servers\n  config       Read or update settings\n  completions  Print shell completions\n  help         Show command help\n\nOptions:\n{options}\n{after-help}\n";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_contracts() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        for args in [
            vec!["pvm", "use", "--restart", "--no-restart"],
            vec!["pvm", "use", "--skip", "apach"],
            vec!["pvm", "current", "--short", "--json"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        let cli =
            Cli::try_parse_from(["pvm", "run", "8.3", "index.php", "--help", "-v", "--weird"])
                .unwrap();
        let Some(Commands::Run { command, .. }) = cli.command else {
            panic!()
        };
        assert_eq!(
            command,
            ["index.php", "--help", "-v", "--weird"].map(OsString::from)
        );
    }
}
