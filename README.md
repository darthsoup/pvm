# pvm — PHP Version Manager

Switch between installed PHP versions on macOS and Linux, or pin a version for a project. Switching is global and can affect other terminals. Use `pvm exec` or `pvm run` when only one command needs a different PHP version.

## Install

From a checkout of this repository, install PVM with a current stable Rust toolchain.

```sh
cargo install --path . --locked
pvm --version
```

Cargo installs to `~/.cargo/bin` by default. Make sure that directory is on your `PATH`.

## Everyday use

```sh
pvm list                     # See installed versions and the active one
pvm use 8.3                  # Switch to the installed 8.3 release line
pvm use                      # Choose interactively
pvm current --short          # Print the active release line
pvm exec 8.2 -- php -v       # Use PHP 8.2 for one command
pvm doctor                   # Diagnose PATH and configuration issues
```

## Pin a project

Run these commands in the project directory. Commit `.pvmrc` to share the pin with your team.

```sh
pvm pin 8.3                  # Write .pvmrc
pvm auto                     # Activate the project pin
```

To switch automatically when you enter a project, add `eval "$(pvm init --shell zsh)"` to `~/.zshrc`, or `eval "$(pvm init --shell bash)"` to `~/.bashrc`, then restart your shell. Fish users can follow the [Fish setup instructions](docs/cli.md#upgrading-shell-integration). **Reload the integration after upgrading PVM.**

## More information

- [CLI guide](docs/cli.md) — aliases, pin replacement, shell setup, configuration, JSON output, and exit codes. For help in the terminal, run `pvm <command> --help`.
- [Benchmark guide](docs/benchmarks.md) — measure hook, discovery, and startup performance.
- [MIT license](LICENSE.md)
