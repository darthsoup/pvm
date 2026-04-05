# pvm — PHP Version Manager

A cross-platform PHP version manager for **macOS** and **Linux**, written in Rust.

## Install

```sh
cargo build --release
sudo cp target/release/pvm /usr/local/bin/pvm
```

**Shell integration** — add to `~/.zshrc` or `~/.bashrc` for auto-switching on `cd`:

```sh
eval "$(pvm init)"
```

## Usage

```sh
pvm list                    # list installed PHP versions
pvm use 8.3                 # switch global PHP
pvm use                     # interactive picker
pvm exec 8.1 -- php artisan migrate   # run command with specific version (no global switch)
pvm pin 8.1                 # write .pvmrc in current directory
pvm alias default 8.3       # create a named alias
pvm info 8.3                # show binary path, php.ini, extensions
pvm doctor                  # diagnose PATH/symlink/config issues
```

Run `pvm --help` or `pvm <command> --help` for full options.

## State

```
~/.pvm/
├── aliases.json   # { "default": "8.3", "legacy": "7.4" }
└── config.json    # { "restart_webserver": "ask" }   (ask | always | never)
```

## License

MIT
