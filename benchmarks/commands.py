"""Read-only executable startup and PHP discovery benchmarks."""

from pathlib import Path
import time

from .common import benchmark_command, run, summarize, validate_counts


def benchmark_command_pair(before, after, command="current", *, samples=9):
    """Compare read-only CLI commands, alternating execution order each pair.

    Both binaries should use the same toolchain/dependencies. A warmup pair is
    discarded. Alternating order reduces bias from changing machine load.
    """
    validate_counts(samples=samples)
    if command not in ("--help", "current", "list"):
        raise ValueError("paired benchmark only supports --help, current, or list")
    if command == "list" and not (Path.home() / ".pvm").is_dir():
        raise ValueError("list could create ~/.pvm; use current instead")
    binaries = {"before": before, "after": after}
    measurements = {"before": [], "after": []}
    outputs = {}
    for sample in range(samples + 1):
        order = ("before", "after") if sample % 2 == 0 else ("after", "before")
        for label in order:
            start = time.perf_counter()
            output = run([str(binaries[label]), command])
            elapsed_ms = (time.perf_counter() - start) * 1000
            outputs[label] = (output.stdout, output.stderr)
            if sample:
                measurements[label].append(elapsed_ms)
    result = {"command": command, "samples": samples, "outputs_equal": outputs["before"] == outputs["after"]}
    for label in binaries:
        result[label] = summarize(measurements[label])
        result[label]["samples_ms_per_invocation"] = measurements[label]
    old, new = result["before"]["median_ms"], result["after"]["median_ms"]
    result["improvement_percent"] = (old - new) / old * 100 if old else None
    return result


def benchmark_startup(binary, *, iterations=100, samples=5):
    """Measure real pvm process startup and CLI help generation."""
    return benchmark_command(binary, ["--help"], iterations=iterations, samples=samples)


def benchmark_discovery(binary, *, iterations=3, samples=5):
    """Measure current/list, including any interpreter probes they perform.

    Skip list when it would create ~/.pvm. Neither command switches PHP or services.
    """
    results = []
    for command in ("current", "list"):
        if command == "list" and not (Path.home() / ".pvm").is_dir():
            results.append({
                "command": command,
                "skipped": "~/.pvm does not exist; list could create it",
            })
            continue
        row = benchmark_command(binary, [command], iterations=iterations, samples=samples)
        row["command"] = command
        results.append(row)
    return results
