# CLI behavior and automation

PVM selects installed PHP **release lines**, such as `8.3`. Global switching changes
system/package-manager links and can affect every terminal using those links.
Project hooks also switch globally. Use `exec` or `run` to isolate a command.

## Versions, aliases, and project pins

Version requests accept `8.3`, `8.3.4`, `php8.3`, and `php@8.3`; PHP prefixes are
case-insensitive. Surrounding whitespace is trimmed. The entire remaining input
must be valid: surrounding garbage and multiple values are rejected. Patch
suffixes select the major/minor release line, **not an exact patch**. Use
`--verbose` to see resolution and discovery details on stderr.

```sh
pvm alias default 8.3
pvm alias project default       # alias chains resolve consistently
pvm pin project                # preserve the named alias in .pvmrc
pvm pin --force 8.4             # replace a different existing pin
```

Aliases may reference valid versions that are not installed yet, or existing
aliases. New alias names must start with an ASCII letter or underscore and contain
only letters, numbers, underscores, and hyphens. Cycles and missing targets fail
before writing. Remove dependent aliases before removing their target.

An identical trimmed pin succeeds without rewriting its file. Replacing a
different `.pvmrc` requires `--force`. Other version files are not modified.
At each directory, `.pvmrc` takes precedence over `.php-version`, then `.pvm`;
the nearest directory containing a file wins. An empty nearest file stops the
search and leaves PHP unchanged.

## Interactive and unattended use

`pvm use` opens a keyboard picker only with terminal stdin/stderr and a non-dumb
terminal. The active version is highlighted when found. Enter confirms a selected
item; Escape or `q` cancels successfully. Ctrl-C exits with status 130 and restores
the terminal. With redirected or closed input, provide a version explicitly:

```sh
pvm use 8.3 --no-restart
pvm use 8.3 --restart --skip nginx
```

Restart policy applies per server: `--skip` wins, then explicit `--restart` or
`--no-restart`, then `restart_webserver` configuration. Confirmations default to
No. With no interactive terminal, `ask` skips restarting and explains how to
choose explicitly. Privileged manual commands use noninteractive sudo when
there is no terminal. Generated hooks never request privileges or restart servers.
An explicit `--restart` also applies when the requested PHP is already active.

PVM verifies PHP resolved through PATH after an actual switch. It reports CLI,
Apache, and restart outcomes separately. A post-switch failure returns status 1
and states that PHP is already active; no automatic rollback is attempted. Apache
configuration changes occur only during an actual explicit `use`; `auto` never
updates Apache. Apache is not restarted if its configuration update failed.

## Running commands

```sh
pvm exec 8.3 -- php artisan migrate --force
pvm run -v 8.3 index.php --filter=users --help
```

All arguments after `run`'s script are forwarded, including `--help` and `-v`.
PVM options belong before the script. `exec` retains its required `--` separator.
Both commands put a temporary PHP shim ahead of PATH, so versioned binaries such
as `php8.3` also work for child commands named `php`. `run` invokes the chosen PHP
binary directly. Child exit codes are preserved; Unix signals map to 128 + signal.

## Output and exit status

Normal command data goes to stdout. Prompts, switching status, errors, and verbose
diagnostics go to stderr. `which` prints only a binary path. `current --short`
prints only the major/minor release line. Redirected `list` output is an unwrapped,
tab-separated table with VERSION, STATUS, ALIASES, PATH columns. Interactive lists
use a table at widths of at least 80 columns and a stacked layout below 80.

`--color auto|always|never` applies to each stream. Explicit always/never overrides
`NO_COLOR`; auto disables color when `NO_COLOR` is present or that stream is not a
terminal. `auto --silent` suppresses normal status but retains errors and does not
ask restart questions. `--silent-if-unchanged` is quiet on successful no-ops.

- 0: successful command, cancellation, or doctor warnings without failures.
- 1: operational failure, unusable/missing active PHP, or failed doctor checks.
- 2: command-line syntax errors from Clap, including invalid server names and
  conflicting flags. `--list` cannot be combined with a subcommand.
- 130: interrupted interactive operation.

Unknown shells cause `init` and `completions` to fail without producing a script.
Use `--shell bash`, `--shell zsh`, or `--shell fish` explicitly.

## JSON contract (schema version 1)

`list`, `current`, `info`, and `doctor` support command-local `--json`.
`current --json` conflicts with `--short`. Stdout contains exactly one JSON
document, without ANSI color or human status messages:

```json
{"schema_version":1,"command":"current","data":{"current":{"version":"8.3","full_version":"8.3.12","path":"/example/bin/php","active":true,"aliases":["default"]}},"error":null}
```

Envelope fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `1`; incompatible changes require a new version. |
| `command` | `list`, `current`, `info`, or `doctor`. |
| `data` | Command payload, or `null` on an operational error. |
| `error` | `null`, or `{ "message": string, "causes": [string] }`. |

`list` data contains `versions`, an array of version records. `current` contains a
`current` version record. Records have `version` (major/minor string),
`full_version` (numeric major/minor/patch string or null), `path` (string),
`active` (boolean), and `aliases` (sorted array of strings). `info` contains
`version`, a record additionally containing `ini_path`, `scan_dir`, `build_date`,
and `modules`. Unknown values are null; a successful empty module list is `[]`.

Doctor data contains `checks` and `summary`. Each check has `id`, `status`
(`passed`, `warning`, `failed`), `message`, and nullable `remediation`. The summary
contains integer `passed`, `warning`, and `failed` counts. Failed checks still
produce the complete report with `error: null`, and return status 1. Operational
errors return an error envelope; Clap syntax errors retain normal stderr and
status 2. `--verbose` may add diagnostics to stderr without changing JSON stdout.

## Configuration

```sh
pvm config get
pvm config get restart_webserver
pvm config set restart_webserver never
pvm config set auto_switch false
```

Defaults are `restart_webserver=ask` and `auto_switch=true`. Restart policy accepts
only `ask`, `always`, or `never`; automatic switching accepts only `true` or `false`.
Unknown keys and invalid values fail without writing. Existing unrelated JSON
configuration fields are preserved. Invalid configuration is reported rather than
silently replaced by defaults. `auto_switch` controls directory hooks; explicit
`pvm auto` still activates the project.

## Upgrading shell integration

**Reload your integration after upgrading PVM.** Bash and Zsh users can restart
their shell, or evaluate `pvm init --shell bash|zsh` for the appropriate shell.
Fish users must regenerate their saved script and restart/source it:

```fish
mkdir -p ~/.config/fish/conf.d
pvm init --shell fish > ~/.config/fish/conf.d/pvm.fish
source ~/.config/fish/conf.d/pvm.fish
```

Caches are shell-local and compare the version file's path and trimmed contents
with a shared generation token. Same-user PVM switches and alias/config updates
invalidate the token. Reads and cached directory changes use shell builtins and
start no PVM process. Empty/unversioned directories leave PHP unchanged.

Activation errors are shown once per source file, requested value, and generation
token. Failed activations retry on later hook invocations; successful recovery
clears the warning state. Hooks return success to avoid interrupting `cd` or
shells using `set -e`. They do not report activation success after a failure.

Changes made directly by Homebrew, another tool, or another user require an
explicit `pvm auto` or `pvm use` to resynchronize. Locking and invalidation apply
to operations sharing the same user state directory, not to external managers.

New internal state files under `~/.pvm/` are `switch.lock` and `hook-state`.
State writes are atomic; a shared lock serializes switches and state mutations.
Directory hooks never wait for that lock: they report contention and retry later.
The hidden `auto --hook` protocol is internal to matching binary/hook versions;
use the documented JSON commands for automation instead.
