# Folder-change performance investigation

Investigated on macOS on 2026-11-02. Measurements describe this machine and the
current working tree; they are not performance targets for other machines.

This is the historical investigation before the detector refinements. The
[applied follow-up](performance-review.md#applied-follow-up) records the fixes,
regression tests, and updated measurements.

The later [matched-build patch assessment](performance-review.md) measures the
original and modified detector/switcher implementations directly and documents
their benefits and correctness limitations.

## Where the time goes

The generated hook walks upward to find a version file, compares its contents
with `PVM_PHP_VERSION`, and runs `pvm auto --silent-if-unchanged` on a cache miss.
The Rust command resolves aliases, checks active PHP, discovers installed versions
if necessary, switches PHP, and then handles webserver restarts.

The cached hook is inexpensive on local storage. An isolated Zsh probe replaced
`pvm` with a shell function and measured three batches of 1,000 hook invocations:

| Scenario | Median time per invocation | `pvm` calls per batch |
| --- | ---: | ---: |
| Matching cached pin in current directory | 0.046 ms | 0 |
| Matching cached pin 20 directories above | 0.257 ms | 0 |
| No version file | 0.093 ms | 0 |

These measurements isolate hook overhead, exclude shell startup, and do not time
PHP switching. Slow filesystems can make the directory walk more expensive.

## Reproducing measurements

```sh
just bench --shell zsh --include-discovery --json /tmp/pvm-benchmark.json
python3 benchmarks/auto_switch.py --binary /usr/local/bin/pvm --include-discovery
```

The benchmark measures real folder transitions, including directory changes and
loop overhead, with an external logging stub for `pvm auto`. Its hook timings
therefore differ from the function-only probe above. It asserts invocation counts
and separately measures real executable startup and optional discovery. Synthetic
backend delays are labeled explicitly; no scenario performs a real PHP switch.

## Causes of perceptible delays

1. **Homebrew revision suffixes defeat the detector's fast path.** The active
   canonical path on this machine is `Cellar/php/8.5.11_1/bin/php`.
   `PATH_VERSION_DIR_RE` and `PATH_FULL_RE` in `src/detector.rs` reject the `_1`
   suffix. Canonicalization also removes the recognizable `php@8.5` opt alias.
   Discovery consequently invokes `php --version` through four candidate paths
   that resolve to this same executable, because fallback probes do not deduplicate
   canonical paths. Read-only release-build `current` and `list` commands took
   approximately 247 ms and 259 ms, versus roughly 63 ms for one PHP startup.
   The quick active-version check shares this parsing problem.
2. **Sequential Homebrew processes.** `src/switcher.rs::do_switch` can run
   `brew unlink php@<active>`, `brew unlink php`, and `brew link` sequentially.
   A failed versioned link adds another `brew link php`. Read-only probes
   (`brew link --help` and `brew unlink --help`, three runs each) had warm medians
   of approximately 256 ms and 250 ms; the first link-help invocation took
   2.29 seconds. This suggests substantial Homebrew startup overhead. Help timings
   are a proxy, not measurements of real linking or an end-to-end switch.
3. **Repeated failed switches.** All hooks discard `pvm auto` stderr and clear
   the cache on failure. An unavailable version or permission error therefore
   retries on subsequent folder changes, or every Bash prompt.
4. **Service handling after switching.** `src/webserver.rs::handle_post_switch_restart`
   detects running servers before applying the restart policy. Even `never` and
   silent `ask` can spawn `pgrep`; `always` can synchronously restart services.
   `--no-restart` bypasses this work, but generated hooks do not supply it.
5. **Cache misses and discovery.** Leaving a pinned project clears the cache;
   equivalent pins such as `8.3` and `8.3.12` also miss the raw-string comparison.
   Actual switches scan Homebrew directories and repeatedly search `PATH`.
   Unrecognized PHP paths still require interpreter probes. Bash scans on every
   prompt, even without a directory change.

Command timings above use Python's `perf_counter()` around sequential subprocesses
with captured output: five runs for PHP and `pvm`, three for Homebrew, reporting
medians. `current` and `list` exercise full discovery, not the entire automatic
switch. The sandbox prevented a usable live `pgrep` timing, so service-check cost
is established by code inspection rather than a measured latency.

## Recommended follow-up

- Verify the installed executable matches the tested build. An identical generated
  hook does not establish that the backend is current. The installed executable's
  read-only `current` and `list` timings were approximately 461 ms and 469 ms,
  respectively, versus 247 ms and 259 ms for the rebuilt release.
- Recognize Homebrew revision suffixes such as `_1` and deduplicate canonical PHP
  paths before fallback probes. Add regression cases for both behaviors.
- Reduce Homebrew invocations by identifying the actual active formula before
  unlinking. Preserve global-switch semantics and test fallback behavior.
- Return before service discovery when the policy precludes any restart.
- Report failed automatic switches once and avoid immediate repeated attempts,
  while allowing recovery after a pin, installation, or permission change.
- Measure discovery separately before adding a persistent cache. Any cache must
  account for manual switches, other shells, aliases, and newly installed PHP.

This investigation and the hook benchmark do not change production switching
behavior. Real Homebrew linking, privileged operations, and service restarts were
not executed during measurement.
