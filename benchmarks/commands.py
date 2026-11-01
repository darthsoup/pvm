"""Read-only executable startup and PHP discovery benchmarks."""

from pathlib import Path

from .common import benchmark_command


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
