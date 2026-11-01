# justfile for pvm — run `just` to list recipes. See https://github.com/casey/just
# Recipes mirror the CI gates (fmt / clippy / test) so local == CI.

set positional-arguments

# Destination for `just install`
prefix := "/usr/local/bin"

# List available recipes
default:
    @just --list

# Debug build
build:
    cargo build

# Optimized release build (target/release/pvm)
release:
    cargo build --release

# Run pvm locally, e.g. `just run list` or `just run use 8.3`
run *args:
    cargo run -- "$@"

# Run tests; pass a name to filter, e.g. `just test switch`
test *args:
    cargo test --all-features "$@"

# Format the code
fmt:
    cargo fmt --all

# Verify formatting without changing files (CI gate)
fmt-check:
    cargo fmt --all -- --check

# Lint with clippy, denying all warnings (CI gate)
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Run formatting, lint, Rust tests, and benchmark unit tests
check: fmt-check clippy test bench-test

# Check benchmark comparison logic and discovery safeguards
bench-test:
    python3 -m unittest discover -s benchmarks/tests -v

# Benchmark generated folder-change hooks without changing system PHP
bench *args: release
    python3 benchmarks/auto_switch.py "$@"

# Build a release binary and install it to the prefix dir (uses sudo)
install: release
    sudo cp target/release/pvm {{prefix}}/pvm

# Remove build artifacts
clean:
    cargo clean
