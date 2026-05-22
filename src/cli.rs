use clap::{Parser, Subcommand, ValueEnum};

#[derive(ValueEnum, Clone, Debug)]
pub enum ShellChoice {
    Bash,
    Zsh,
    Fish,
}

#[derive(Parser)]
#[command(
    name = "pvm",
    version,
    about = "PHP Version Manager — switch PHP versions globally or per-project",
    long_about = None
)]
pub struct Cli {
    /// List all installed PHP versions (alias for 'list')
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// Verbose output
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all installed PHP versions
    #[command(alias = "ls")]
    List,

    /// Show the currently active PHP version and path
    Current,

    /// Print the binary path for a specific version or alias
    Which {
        /// Version string or alias name (e.g. "8.2" or "default")
        version: Option<String>,
    },

    /// Switch the global PHP version (interactive picker if no version given)
    Use {
        /// Version string, alias name, or "default"
        version: Option<String>,

        /// Skip web server restart
        #[arg(long)]
        no_restart: bool,

        /// Skip restarting a specific server (apache or nginx)
        #[arg(long, value_name = "SERVER")]
        skip: Vec<String>,
    },

    /// Manage named aliases (list all if no args given)
    Alias {
        /// Alias name to create or update
        name: Option<String>,

        /// PHP version the alias should point to
        version: Option<String>,
    },

    /// Remove a named alias
    Unalias {
        /// Name of the alias to remove
        name: String,
    },

    /// Write a .pvmrc file in the current directory
    Pin {
        /// Version or alias to pin (defaults to currently active version)
        version: Option<String>,
    },

    /// Read .pvmrc, .php-version, or .pvm from cwd or parent directories and switch to that version
    Auto {
        /// Suppress all output (used by shell hook)
        #[arg(long)]
        silent: bool,

        /// Suppress output only when PHP version is already correct (better for cd hooks)
        #[arg(long)]
        silent_if_unchanged: bool,

        /// Skip web server restart
        #[arg(long)]
        no_restart: bool,

        /// Skip restarting a specific server
        #[arg(long, value_name = "SERVER")]
        skip: Vec<String>,
    },

    /// Run a command with a specific PHP version in PATH (no global switch)
    ///
    /// Example: pvm exec 8.1 -- php artisan migrate
    Exec {
        /// PHP version to use
        version: String,

        /// Command and arguments (everything after --)
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },

    /// Run a PHP script directly with a specific version
    ///
    /// Example: pvm run 8.3 index.php arg1 arg2
    Run {
        /// PHP version to use
        version: String,

        /// PHP script path
        script: String,

        /// Arguments to pass to the script
        args: Vec<String>,
    },

    /// Show detailed information about a PHP version
    Info {
        /// Version string or alias name
        version: String,
    },

    /// Check for common configuration issues
    Doctor,

    /// Restart web servers
    Restart {
        /// Specific server to restart: apache or nginx
        server: Option<String>,
    },

    /// Print shell integration script (add: eval "$(pvm init)" to .zshrc/.bashrc)
    Init {
        /// Shell to generate integration for (auto-detected from $SHELL if not given)
        #[arg(long, value_enum)]
        shell: Option<ShellChoice>,
    },

    /// Print shell completions to stdout
    ///
    /// Example: pvm completions --shell zsh > ~/.zsh/completions/_pvm
    Completions {
        /// Shell to generate completions for
        #[arg(long, value_enum)]
        shell: Option<ShellChoice>,
    },
}
