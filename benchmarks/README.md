# Performance benchmarks

Requires Python 3.9+ and a release build of `pvm`. No Python packages are needed.

## Run after a change

Save a baseline before editing, then repeat the same workload after rebuilding:

```sh
just bench --shell zsh --include-discovery --json /tmp/pvm-before.json
# Make changes; just bench rebuilds the release binary each time.
just bench --shell zsh --include-discovery --baseline /tmp/pvm-before.json --json /tmp/pvm-after.json
```

The second command exits with status 1 if a median increases by **both** more than
20% and more than 0.1 ms. Adjust with `--max-regression-percent` and
`--min-regression-ms`. Compare on the same machine with similar background load.
Keep shells, iterations, samples, depth, synthetic delay, and discovery options
identical. Incompatible or incomplete baselines fail explicitly. Rerun a timing
failure to distinguish a persistent regression from machine noise.

`just bench-test` checks the comparison rules and discovery safeguards;
`just check` includes these tests. CI also runs the hook scenarios on Linux and
macOS and uploads timing JSON as `benchmarks-<os>` artifacts. CI enforces invocation
counts and successful completion; timing comparisons use a local baseline because
hosted runners have variable performance. Fish runs when installed; use
`--shell fish` to require it explicitly.

## Reusable functions

Run Python from the repository root:

```python
from benchmarks import benchmark_discovery, benchmark_hooks, benchmark_startup
from benchmarks.comparison import compare_results

binary = "./target/release/pvm"
hooks = benchmark_hooks(binary, shells=["zsh"], iterations=100, samples=5, depth=5)
startup = benchmark_startup(binary, iterations=100, samples=5)
discovery = benchmark_discovery(binary, iterations=3, samples=5)
```

- `benchmark_hooks()` returns one row per shell/scenario and asserts the expected
  `pvm auto` call count. Scenarios cover no pin, cached/uncached pins, alternating
  versions, and failed attempts. Shell startup is excluded.
- `benchmark_startup()` returns real `pvm --help` timings.
- `benchmark_discovery()` returns real `current`/`list` timings, including PHP
  subprocesses. It skips `list` when it would create `~/.pvm`.
- `compare_results(current, baseline)` compares complete JSON run dictionaries
  and returns rows containing `regressed`, deltas, and the allowed timing increase.

Timing rows include `median_ms`, `p95_batch_ms`, and raw batch averages. One warmup
batch is discarded. The p95 describes batch averages, not individual transitions.
Hooks use temporary fixtures and an external logging stub; they never perform real
PHP switches or service restarts. `--auto-delay-ms` models backend delay explicitly.
See [the investigation](../docs/performance.md) for measured production bottlenecks.

## Compare two builds directly

Build and save both executables with the same dependencies and toolchain, then run:

```sh
python3 benchmarks/compare_binaries.py --before /tmp/pvm-before --after ./target/release/pvm --json /tmp/pvm-paired.json
```

This measures `--help`, `current`, and `list` with alternating before/after execution
order and nine measured pairs after warmup. `benchmark_command_pair()` in
`benchmarks.commands` provides the same measurement as a Python function.
It permits only those read-only commands and skips `list` in the CLI when `~/.pvm`
does not exist. See [the patch assessment](../docs/performance-review.md) for the
detector/switcher comparison and its correctness limitations.
