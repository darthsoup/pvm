# Performance assessment of the detector and switcher patches

Measured on macOS on 2026-11-02. Both original patches were restored unchanged
for this assessment. Before/after release binaries use the same Rust toolchain,
Cargo manifest, and lockfile; only `detector.rs` and `switcher.rs` differ.

## Applied follow-up

The detector has since been refined to infer versions only from recognized
Homebrew Cellar roots and exact versioned names in standard system bin
directories. Homebrew revision suffixes are supported, formula/version mismatches
fall back to probing, and canonical paths are deduplicated before both successful
and failed probes. Custom binaries are executed to verify their actual version.
The switcher's selective unlinking is retained.

A second matched-build comparison measures the original optimization patch
against these refinements, using nine alternating pairs after warmup:

| Operation | Original optimization patch | Refined detector | Improvement |
| --- | ---: | ---: | ---: |
| `pvm current` | 257.2 ms | 7.2 ms | 97.2% |
| `pvm list` | 249.7 ms | 6.9 ms | 97.2% |
| Internal active-version check | 59.03 ms | 0.48 ms | 99.2% |

The CLI outputs and active version agree between builds. The internal check
excludes executable startup and uses isolated test binaries against the actual
installed PHP path; it is not an end-to-end switching measurement. Raw results are
`target/performance-review/fixed-vs-original-patch.json` and `fixed-quick.json`.
The refined executable is saved at `target/performance-review/fixed/pvm`.

Eight Rust regression tests cover revision parsing, ambiguous/custom paths,
broken executables, dangling symlinks, active-path preservation, and deduplication
of successful and failed fallback probes. These run with `cargo test --all-features`.

## Original patch verdict

The detector patch provides a substantial, measured discovery improvement.
The switcher patch reduces subprocess overhead when more than two PHP versions
are installed. Neither changes the already cached shell-hook path. The detector
patch also introduces a demonstrated version-identification error, so it needs
refinement before being considered ready.

## Real executable measurements

Nine measured pairs, alternating execution order after one warmup pair. Times are
medians of complete read-only CLI invocations, not an actual global switch.

| Command | Before | After | Improvement |
| --- | ---: | ---: | ---: |
| `--help` | 5.1 ms | 4.7 ms | 7.0% |
| `current` | 459.8 ms | 252.7 ms | 45.0% |
| `list` | 493.3 ms | 272.0 ms | 44.9% |

All three commands produced identical before/after output. Help is a startup
control; its small absolute difference should not be attributed to the patches.

On this machine, discovery has eight candidate paths. The old detector probes
all eight. The modified detector recognizes some paths and needs four fallback
probes, all resolving to the same PHP executable. Removing four PHP startups
explains the roughly 200 ms saving; regex caching alone does not explain it.

The separate active-version quick check measured **61.4 ms before** and
**65.0 ms after** across nine measured pairs, with the same version result.
This does not demonstrate a speedup. The actual canonical path contains
`Cellar/php/8.5.12_1/bin/php`; the new regex misses `_1`, so both implementations
still execute PHP. This check runs on an uncached automatic request even when the
requested version is already active. A shell-cache hit invokes neither binary.

## Isolated switcher measurements

Private-function probes were appended only to temporary source snapshots under
`target/performance-review/`. They call the actual platform-specific `do_switch`
with synthetic version lists and a logging `brew` stub that always succeeds.
Each stub invocation waits **40 ms**, deliberately synthetic. Three measured
pairs follow warmup; no real links, services, or privileged operations are changed.

| Installed versions | Brew calls before → after | Stubbed time before → after |
| --- | ---: | ---: |
| 2 | 3 → 3 | 159.7 → 157.4 ms |
| 3 | 4 → 3 | 212.6 → 160.2 ms |
| 6 | 7 → 3 | 373.5 → 168.2 ms |

For N installed versions with an identified active version different from the
target, the successful-link path goes from N+1 calls to three. There are three
installed versions locally, so this saves one Homebrew invocation. With no
identified active version the probe observed four calls becoming one; real
Homebrew symlink/fallback behavior in that condition remains unverified.
These timings demonstrate reduced subprocess work, not real relinking latency.

## Findings in the original patch

- **Incorrect version inferred from an unrelated parent directory.** An isolated
  executable at `/custom/8.1/bin/php` that prints PHP 8.3.11 is identified as 8.3
  by the original code and 8.1 by the patch. The modified quick check makes zero
  PHP calls. A request for 8.1 can therefore incorrectly return “already using”
  while PHP 8.3 is active. Restrict inference to validated PHP installation layouts
  and use a probe for ambiguous custom paths.
- **Homebrew revisions miss the fast path.** A fixture at
  `Cellar/php/8.3.11_1/bin/php` still invokes PHP once in both versions. A normal
  Cellar path and a `php8.3` filename avoid that call after the patch. Add revision
  suffix support and deduplicate canonical paths before fallback execution.
- **Path recognition does not validate executable health.** A broken executable
  named `php8.3` returned no active version before, but the modified check reports
  8.3 without executing it. This is a consequence of trusting names alone.
- Regex `LazyLock` values are cached within one process. Repeated `pvm auto`
  invocations still initialize needed regexes anew; the larger win is avoiding
  interpreter and package-manager processes.

## Reproduce and inspect

```sh
python3 benchmarks/compare_binaries.py \
  --before target/performance-review/baseline/pvm \
  --after target/performance-review/modified/pvm \
  --samples 9 --json target/performance-review/discovery.json
```

Raw local results are in `target/performance-review/discovery.json` and
`target/performance-review/probes.json`. The temporary `prepare_probes.py` and
`run_probes.py` scripts record the source instrumentation and fixture methodology.
These artifacts are ignored by Git and removed by `cargo clean`.
