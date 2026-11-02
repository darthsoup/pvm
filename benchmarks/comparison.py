"""Compare repeat benchmark runs without treating timing noise as a regression."""

import math


CONFIG_FIELDS = (
    "schema_version", "hook_fixture_version", "platform", "iterations", "samples", "depth", "auto_delay_ms",
    "include_discovery", "discovery_iterations",
)


def _metrics(result):
    metrics = {"startup/--help": result["help_startup"]["median_ms"]}
    for row in result["hooks"]:
        key = "hook/{}/{}".format(row["shell"], row["case"])
        if key in metrics:
            raise ValueError("duplicate benchmark metric: " + key)
        metrics[key] = row["median_ms"]
    for row in result.get("discovery", []):
        key = "discovery/" + row["command"]
        if key in metrics:
            raise ValueError("duplicate benchmark metric: " + key)
        metrics[key] = None if "skipped" in row else row["median_ms"]
    for key, value in metrics.items():
        if value is not None and (not math.isfinite(value) or value < 0):
            raise ValueError("invalid timing for " + key)
    return metrics


def compare_results(current, baseline, *, max_regression_percent=20, min_regression_ms=0.1):
    """Return comparison rows; a regression must exceed both allowed deltas.

    Reject different platforms, workloads, missing scenarios, or skipped metrics
    rather than silently reporting a successful comparison. Binary paths may differ.
    """
    for value in (max_regression_percent, min_regression_ms):
        if not math.isfinite(value) or value < 0:
            raise ValueError("comparison thresholds must be finite and nonnegative")
    if not isinstance(current, dict) or not isinstance(baseline, dict):
        raise ValueError("benchmark runs must be JSON objects")
    if current.get("schema_version") != 1 or baseline.get("schema_version") != 1:
        raise ValueError("unsupported baseline format; save a new baseline with this runner")
    for field in CONFIG_FIELDS:
        if field not in current or field not in baseline or current[field] != baseline[field]:
            raise ValueError("baseline configuration differs: " + field)
    before, after = _metrics(baseline), _metrics(current)
    if before.keys() != after.keys():
        raise ValueError("baseline scenarios differ; use the same shells and discovery options")
    comparisons = []
    for name, old in before.items():
        new = after[name]
        if old is None or new is None:
            if old != new:
                raise ValueError("benchmark availability differs: " + name)
            continue
        delta = new - old
        allowed = max(old * max_regression_percent / 100, min_regression_ms)
        comparisons.append({
            "name": name, "baseline_ms": old, "current_ms": new,
            "delta_ms": delta, "allowed_delta_ms": allowed,
            "regressed": delta > allowed,
        })
    return comparisons
