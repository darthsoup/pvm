#!/usr/bin/env python3
"""Run repeatable folder-change benchmarks and optionally check a saved baseline."""

import argparse
import json
import math
from pathlib import Path
import platform
import subprocess
import sys

# Support both python3 benchmarks/auto_switch.py and python3 -m benchmarks.auto_switch.
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from benchmarks import benchmark_discovery, benchmark_hooks, benchmark_startup
from benchmarks.comparison import compare_results


ROOT = Path(__file__).resolve().parents[1]


def positive_int(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return number


def nonnegative_float(value):
    number = float(value)
    if not math.isfinite(number) or number < 0:
        raise argparse.ArgumentTypeError("must be finite and nonnegative")
    return number


def benchmark(args):
    """Collect a complete, JSON-serializable benchmark run."""
    binary = Path(args.binary).expanduser().resolve()
    if not binary.is_file():
        raise ValueError("binary not found: {}; run cargo build --release".format(binary))
    result = {
        "schema_version": 1,
        "hook_fixture_version": 2,
        "binary": str(binary),
        "platform": platform.platform(),
        "iterations": args.iterations,
        "samples": args.samples,
        "depth": args.depth,
        "auto_delay_ms": args.auto_delay_ms,
        "include_discovery": args.include_discovery,
        "discovery_iterations": args.discovery_iterations,
        "measurement": "batch wall time divided by folder transitions; shell startup excluded",
        "backend": "logging stub; no real PHP switching or service operations",
    }
    result["hooks"] = benchmark_hooks(
        binary, args.shell, iterations=args.iterations, samples=args.samples,
        depth=args.depth, auto_delay_ms=args.auto_delay_ms,
    )
    print("{:<5}  {:<23}  {:>9}  {:>9}  {:>5}".format(
        "Shell", "Scenario", "Median", "P95 batch", "Calls"
    ))
    for row in result["hooks"]:
        print("{:<5}  {:<23}  {:>9.3f}  {:>9.3f}  {:>5}".format(
            row["shell"], row["case"], row["median_ms"], row["p95_batch_ms"],
            row["auto_calls_per_transition"],
        ))
    result["help_startup"] = benchmark_startup(
        binary, iterations=args.iterations, samples=args.samples,
    )
    if args.include_discovery:
        result["discovery"] = benchmark_discovery(
            binary, iterations=args.discovery_iterations, samples=args.samples,
        )
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=str(ROOT / "target/release/pvm"))
    parser.add_argument("--shell", action="append", choices=("bash", "zsh", "fish"))
    parser.add_argument("--iterations", type=positive_int, default=100)
    parser.add_argument("--samples", type=positive_int, default=5)
    parser.add_argument("--depth", type=positive_int, default=5, help="subdirectories below each project")
    parser.add_argument("--auto-delay-ms", type=nonnegative_float, default=0,
                        help="synthetic latency per stubbed pvm auto call (default: 0)")
    parser.add_argument("--include-discovery", action="store_true",
                        help="also time real current/list discovery (may execute installed PHP)")
    parser.add_argument("--discovery-iterations", type=positive_int, default=3)
    parser.add_argument("--json", type=Path, help="write detailed results to a JSON file")
    parser.add_argument("--baseline", type=Path, help="compare against a previous JSON run")
    parser.add_argument("--max-regression-percent", type=nonnegative_float, default=20)
    parser.add_argument("--min-regression-ms", type=nonnegative_float, default=0.1,
                        help="ignore timing increases at or below this absolute delta")
    args = parser.parse_args()
    if args.baseline and args.json and args.baseline.resolve() == args.json.resolve():
        parser.error("--json must not overwrite --baseline")
    print("Hook benchmark: stubbed pvm auto; no system PHP changes")
    print("Timing: ms/transition, shell startup excluded; p95 is across batch averages")
    print("Synthetic auto delay: {:g} ms".format(args.auto_delay_ms))
    try:
        baseline = json.loads(args.baseline.read_text()) if args.baseline else None
        result = benchmark(args)
        print("Real pvm --help startup: {:.3f} ms/invocation (median)".format(
            result["help_startup"]["median_ms"]
        ))
        for row in result.get("discovery", []):
            if "skipped" in row:
                print("Real pvm {}: skipped ({})".format(row["command"], row["skipped"]))
            else:
                print("Real pvm {} discovery: {:.3f} ms/invocation (median)".format(
                    row["command"], row["median_ms"]
                ))
        regressions = []
        if baseline is not None:
            result["comparison"] = compare_results(
                result, baseline, max_regression_percent=args.max_regression_percent,
                min_regression_ms=args.min_regression_ms,
            )
            for row in result["comparison"]:
                status = "REGRESSION" if row["regressed"] else "PASS"
                print("{} {}: {:.3f} -> {:.3f} ms ({:+.3f} ms)".format(
                    status, row["name"], row["baseline_ms"], row["current_ms"], row["delta_ms"],
                ))
                if row["regressed"]:
                    regressions.append(row)
        if args.json:
            args.json.write_text(json.dumps(result, indent=2) + "\n")
            print("JSON: " + str(args.json))
        if regressions:
            parser.exit(1, "performance regressions: {}\n".format(len(regressions)))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, "benchmark failed: {}\n".format(error))


if __name__ == "__main__":
    main()
