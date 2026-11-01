"""Measure generated shell hooks in temporary fixtures with a stubbed backend."""

import math
import os
from pathlib import Path
import selectors
import shlex
import shutil
import signal
import subprocess
import tempfile
import time

from .common import run, summarize, validate_counts


CASES = (
    ("no version file", "unversioned", False, False, 0),
    ("cached same version", "same", False, False, 0),
    ("uncached same version", "same", True, False, 1),
    ("alternating versions", "alternating", False, False, 1),
    ("failed auto (retries)", "alternating", False, True, 1),
)


def quote(value):
    return shlex.quote(str(value))


def shell_command(shell, executable, script):
    flags = {
        "bash": ["--noprofile", "--norc"],
        "zsh": ["-f"],
        "fish": ["--no-config"],
    }
    return [executable] + flags[shell] + [str(script)]


def make_driver(shell, hook, targets, iterations, clear_cache, cached):
    # Source in an unversioned directory before the parent starts the timer.
    if shell == "fish":
        lines = ["source " + quote(hook), "set -eg PVM_PHP_VERSION"]
        if cached:
            lines.append("set -gx PVM_PHP_VERSION 8.3")
        lines += [
            "printf 'PVM_BENCH_READY\\n'",
            "read -l pvm_bench_start",
            "set -l i 0",
            "while test $i -lt " + str(iterations),
            "  set i (math $i + 1)",
        ]
        if clear_cache:
            lines.append("  set -eg PVM_PHP_VERSION")
        lines += [
            "  if test (math $i % 2) -eq 1",
            "    cd " + quote(targets[0]),
            "  else",
            "    cd " + quote(targets[1]),
            "  end",
            "end",
            "printf 'PVM_BENCH_DONE\\n'",
        ]
    else:
        lines = ["source " + quote(hook), "unset PVM_PHP_VERSION"]
        if cached:
            lines.append("export PVM_PHP_VERSION=8.3")
        lines += [
            "printf 'PVM_BENCH_READY\\n'", "IFS= read -r pvm_bench_start",
            "i=0", "while (( i < " + str(iterations) + " )); do", "  (( i += 1 ))",
        ]
        if clear_cache:
            lines.append("  unset PVM_PHP_VERSION")
        lines += [
            "  if (( i % 2 )); then",
            "    cd " + quote(targets[0]),
            "  else",
            "    cd " + quote(targets[1]),
            "  fi",
        ]
        if shell == "bash":
            # Noninteractive Bash has no prompt; invoke the installed prompt
            # callback explicitly after each cd. Zsh/Fish fire their cd events.
            lines.append("  _pvm_auto_switch")
        lines.append("done")
        lines.append("printf 'PVM_BENCH_DONE\\n'")
    return "\n".join(lines) + "\n"


def read_marker(process, command, timeout):
    deadline = time.monotonic() + timeout
    data = b""
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while b"\n" not in data:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                raise subprocess.TimeoutExpired(command, timeout)
            chunk = os.read(process.stdout.fileno(), 4096)
            if not chunk:
                break
            data += chunk
    return data.decode(errors="replace").strip()


def time_driver(command, cwd, environment, iterations, delay_ms):
    process = subprocess.Popen(command, cwd=cwd, env=environment, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    try:
        if read_marker(process, command, 30) != "PVM_BENCH_READY":
            raise ValueError("shell failed before benchmark")
        start = time.perf_counter()
        process.stdin.write(b"\n")
        process.stdin.flush()
        # Allow synthetic latency plus startup/scheduling slack, while bounding
        # regressions such as an infinite parent-directory walk.
        timeout = 30 + iterations * (delay_ms / 1000 + 0.02)
        marker = read_marker(process, command, timeout)
        elapsed_ms = (time.perf_counter() - start) * 1000 / iterations
        _, stderr = process.communicate(timeout=5)
        if marker != "PVM_BENCH_DONE" or process.returncode:
            raise ValueError("shell failed during benchmark: " + stderr.decode(errors="replace"))
        return elapsed_ms
    finally:
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass  # Process exited between poll() and killpg().
        process.communicate(timeout=5)


def find_shells(shells=None):
    """Find requested shells, or auto-detect installed Bash, Zsh, and Fish."""
    available = []
    for shell in shells if shells is not None else ("bash", "zsh", "fish"):
        if shell not in ("bash", "zsh", "fish"):
            raise ValueError("unsupported shell: " + shell)
        executable = shutil.which(shell)
        if executable:
            available.append((shell, executable))
        elif shells is not None:
            raise ValueError("requested shell is not installed: " + shell)
    if not available:
        raise ValueError("no supported shell found (bash, zsh, fish)")
    return available


def benchmark_hooks(binary, shells=None, *, iterations=100, samples=5, depth=5,
                    auto_delay_ms=0):
    """Return timing rows for folder transitions and assert backend call counts.

    Shell startup is excluded. Real PHP, Homebrew, and services are never switched.
    auto_delay_ms adds explicitly synthetic latency to each stub invocation.
    """
    validate_counts(iterations=iterations, samples=samples, depth=depth)
    if not math.isfinite(auto_delay_ms) or auto_delay_ms < 0:
        raise ValueError("auto_delay_ms must be finite and nonnegative")
    binary = Path(binary).expanduser().resolve()
    shells = find_shells(shells)
    rows = []
    with tempfile.TemporaryDirectory(prefix="pvm-benchmark-") as directory:
        work = Path(directory)
        # No version files may leak into the no-version scenario from ancestors.
        for parent in work.parents:
            for name in (".pvmrc", ".php-version", ".pvm"):
                if (parent / name).is_file():
                    raise ValueError("ancestor version file would contaminate fixtures: " + str(parent / name))
        unversioned = work / "unversioned"
        projects = {}
        for group in ("unversioned", "same", "alternating"):
            paths = []
            for index in range(2):
                project = work / group / ("project" + str(index))
                nested = project.joinpath(*("level" + str(i) for i in range(depth)))
                nested.mkdir(parents=True)
                if group != "unversioned":
                    version = "8.2" if group == "alternating" and index == 0 else "8.3"
                    (project / ".pvmrc").write_text(version + "\n")
                paths.append(nested)
            projects[group] = paths

        stub_dir = work / "bin"
        stub_dir.mkdir()
        stub = stub_dir / "pvm"
        stub.write_text(
            "#!/bin/sh\n"
            "# Deliberately never invoke the real pvm binary.\n"
            "[ \"$1\" = auto ] || exit 99\n"
            "printf '%s\\n' \"$*\" >> \"$PVM_BENCH_LOG\"\n"
            "if [ \"$PVM_BENCH_DELAY\" != 0 ]; then\n"
            "  /bin/sleep \"$PVM_BENCH_DELAY\"\n"
            "fi\n"
            "exit \"$PVM_BENCH_FAIL\"\n"
        )
        stub.chmod(0o755)
        environment = os.environ.copy()
        # Leave HOME untouched; no benchmark operation reads/writes pvm state.
        environment.update({
            "PATH": str(stub_dir) + os.pathsep + environment.get("PATH", ""),
            "PVM_BENCH_LOG": str(work / "calls.log"),
            "PVM_BENCH_DELAY": str(auto_delay_ms / 1000) if auto_delay_ms else "0",
        })
        for shell, executable in shells:
            hook = work / ("hook." + shell)
            hook.write_bytes(run([str(binary), "init", "--shell", shell]).stdout)
            for name, group, clear_cache, fails, calls_per_transition in CASES:
                driver = work / ("driver." + shell)
                driver.write_text(make_driver(
                    shell, hook, projects[group], iterations, clear_cache,
                    cached=name == "cached same version",
                ))
                environment["PVM_BENCH_FAIL"] = "1" if fails else "0"
                measurements = []
                for sample in range(samples + 1):
                    log = Path(environment["PVM_BENCH_LOG"])
                    log.write_text("")
                    elapsed_ms = time_driver(
                        shell_command(shell, executable, driver), unversioned,
                        environment, iterations, auto_delay_ms,
                    )
                    calls = len(log.read_text().splitlines())
                    expected = calls_per_transition * iterations
                    if calls != expected:
                        raise ValueError("{} / {}: expected {} auto calls, observed {}".format(
                            shell, name, expected, calls
                        ))
                    if sample:  # Discard the first batch as warmup.
                        measurements.append(elapsed_ms)
                row = {
                    "shell": shell, "case": name,
                    "auto_calls_per_transition": calls_per_transition,
                    "samples_ms_per_transition": measurements,
                }
                row.update(summarize(measurements))
                rows.append(row)
    return rows
