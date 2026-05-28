# pvm — PHP Version Manager

A cross-platform PHP version manager for **macOS** and **Linux**, written in Rust.
Inspired by nvm and fvm — automatically switches PHP versions when you enter a project directory.

## Install

```sh
cargo build --release
sudo cp target/release/pvm /usr/local/bin/pvm
```

**Shell integration** — add to `~/.zshrc` or `~/.bashrc` for auto-switching on `cd`:

```sh
eval "$(pvm init)"
```

For fish, save once:

```sh
pvm init --shell fish > ~/.config/fish/conf.d/pvm.fish
```

## Auto-switching

pvm automatically activates the correct PHP version when you `cd` into a project — no manual `pvm use` needed.

**Step 1:** Pin a version in your project:

```sh
cd ~/my-project
pvm pin 8.2          # writes .pvmrc with "8.2"
```

**Step 2:** That's it. Next time you `cd` into the project, pvm switches PHP automatically.

Supported version files (searched from cwd upwards):
- `.pvmrc`
- `.php-version`
- `.pvm`

You can also trigger the switch manually:

```sh
pvm auto             # read version file and switch
```

## Usage

```sh
pvm list                    # list installed PHP versions
pvm use 8.3                 # switch global PHP
pvm use                     # interactive picker
pvm pin 8.1                 # write .pvmrc in current directory
pvm exec 8.1 -- php artisan migrate   # run command with specific version (no global switch)
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
