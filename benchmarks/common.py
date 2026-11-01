"""Shared timing primitives for dependency-free performance benchmarks."""

import math
import statistics
import subprocess
import time


def validate_counts(**counts):
    for name, value in counts.items():
        if not isinstance(value, int) or value < 1:
            raise ValueError(name + " must be a positive integer")


def run(command, **kwargs):
    return subprocess.run(
        command, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=30, **kwargs
    )


def summarize(samples):
    ordered = sorted(samples)
    return {
        "median_ms": statistics.median(samples),
        "p95_batch_ms": ordered[math.ceil(0.95 * len(ordered)) - 1],
    }


def benchmark_command(binary, arguments, *, iterations=100, samples=5):
    """Time a read-only command, discarding one warmup batch.

    Return median/p95 batch averages and raw milliseconds per invocation.
    Callers choose the command and are responsible for its side effects.
    """
    validate_counts(iterations=iterations, samples=samples)
    measurements = []
    for sample in range(samples + 1):
        start = time.perf_counter()
        for _ in range(iterations):
            run([str(binary)] + list(arguments))
        elapsed_ms = (time.perf_counter() - start) * 1000 / iterations
        if sample:
            measurements.append(elapsed_ms)
    result = summarize(measurements)
    result["samples_ms_per_invocation"] = measurements
    return result
