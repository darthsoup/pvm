# Repository Guidelines

## Project Structure & Module Organization

`pvm` is a single-binary Rust 2024 PHP version manager targeting macOS and Linux; Windows support is experimental.

- `src/main.rs`: command dispatch, `cmd_*` handlers, and generated Bash/Zsh/Fish hooks.
- `src/cli.rs`: clap-derived subcommands and flags.
- `src/detector.rs` and `src/switcher.rs`: PHP discovery and platform-specific switching.
- `src/pvmrc.rs`, `src/aliases.rs`, and `src/utils.rs`: project version files, aliases, configuration, and shared helpers.
- `src/webserver.rs`: Apache/Nginx detection, configuration, and restarts.
- `.github/workflows/`: CI and tagged release builds. Cargo output lives in ignored `target/`; there are currently no dedicated test or asset directories.

## Build, Test, and Development Commands

Use stable Rust with rustfmt and Clippy installed.

```sh
cargo build                   # Build the debug binary
cargo build --release         # Build target/release/pvm
cargo run -- --help           # Inspect the CLI locally
cargo fmt --all               # Apply repository formatting
cargo fmt --all -- --check    # Verify formatting
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features     # Run all tests
```

Run the formatting check, Clippy, and tests before submitting changes. The `justfile` also provides `just check` for all three gates.

## Coding Style & Naming Conventions

Use four-space indentation, `snake_case` for functions/modules, and `PascalCase` for types. Follow `rustfmt.toml`: 100-column width, field-init shorthand, and `?` shorthand. Use `anyhow::Result` and contextual errors. Gate OS-specific code with `#[cfg(target_os = "...")]`. Keep CLI definitions in `cli.rs` and follow the existing `cmd_*` handler pattern.

## Testing Guidelines

Use Rust's built-in test framework: module-local `#[cfg(test)]` blocks and descriptive `#[test]` function names such as `normalizes_patch_version`. Filter with `cargo test normalizes_patch_version --all-features`. No tests or coverage threshold currently exist, so a passing test command alone does not verify behavior. For behavioral changes, cover relevant parsing and resolution cases and document manual platform/shell checks.

## Commit & Pull Request Guidelines

History commonly uses `feat:`, `fix:`, `perf:`, and `chore:` prefixes, alongside some unprefixed subjects. Prefer concise, imperative subjects with an appropriate prefix. PRs should explain the behavior change, link relevant issues, and report validation commands and platform/shell coverage. Include terminal examples when CLI output changes.

## Configuration & Architecture Safeguards

Persistent state lives in `~/.pvm/`. Reuse existing alias and version-normalization helpers. Preserve the shell hooks' subprocess-free fast path. Keep Apache configuration updates tied to explicit switching; `pvm auto` disables them. Verify switching and restart behavior in an isolated environment because these commands can modify system configuration.
