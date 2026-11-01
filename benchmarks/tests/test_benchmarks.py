"""Regression checks for baseline decisions and read-only discovery safeguards."""

from copy import deepcopy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from benchmarks import benchmark_discovery
from benchmarks.comparison import compare_results


def result():
    return {
        "schema_version": 1, "platform": "test-machine", "iterations": 100,
        "samples": 5, "depth": 5, "auto_delay_ms": 0, "include_discovery": False,
        "discovery_iterations": 3, "help_startup": {"median_ms": 5.0},
        "hooks": [{"shell": "zsh", "case": "cached", "median_ms": 0.05}],
    }


class BaselineComparisonTests(unittest.TestCase):
    def test_small_absolute_noise_does_not_fail_fast_hook(self):
        baseline = result()
        current = deepcopy(baseline)
        current["hooks"][0]["median_ms"] = 0.09  # 80% increase, only 0.04 ms.
        self.assertFalse(any(row["regressed"] for row in compare_results(current, baseline)))

    def test_large_startup_regression_fails(self):
        baseline = result()
        current = deepcopy(baseline)
        current["help_startup"]["median_ms"] = 7.0
        failures = [row["name"] for row in compare_results(current, baseline) if row["regressed"]]
        self.assertEqual(failures, ["startup/--help"])

    def test_improvement_passes_zero_tolerance(self):
        baseline = result()
        current = deepcopy(baseline)
        current["help_startup"]["median_ms"] = 2.0
        rows = compare_results(current, baseline, max_regression_percent=0, min_regression_ms=0)
        self.assertFalse(any(row["regressed"] for row in rows))

    def test_mismatched_workloads_are_rejected(self):
        baseline = result()
        for field, value in (("platform", "other-machine"), ("depth", 20), ("auto_delay_ms", 10)):
            with self.subTest(field=field):
                current = deepcopy(baseline)
                current[field] = value
                with self.assertRaisesRegex(ValueError, field):
                    compare_results(current, baseline)

    def test_missing_scenario_cannot_silently_pass(self):
        baseline = result()
        current = deepcopy(baseline)
        current["hooks"] = []
        with self.assertRaisesRegex(ValueError, "scenarios differ"):
            compare_results(current, baseline)

    def test_old_or_malformed_baseline_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "JSON objects"):
            compare_results(result(), [])
        baseline = result()
        del baseline["schema_version"]
        with self.assertRaisesRegex(ValueError, "unsupported baseline format"):
            compare_results(result(), baseline)

    def test_nonfinite_timing_cannot_silently_pass(self):
        baseline = result()
        for timing in (float("nan"), float("inf"), -1):
            with self.subTest(timing=timing):
                current = deepcopy(baseline)
                current["help_startup"]["median_ms"] = timing
                with self.assertRaisesRegex(ValueError, "invalid timing"):
                    compare_results(current, baseline)

    def test_changed_discovery_availability_is_rejected(self):
        baseline = result()
        baseline["include_discovery"] = True
        baseline["discovery"] = [{"command": "list", "skipped": "no state directory"}]
        current = deepcopy(baseline)
        current["discovery"] = [{"command": "list", "median_ms": 2.0}]
        with self.assertRaisesRegex(ValueError, "availability differs"):
            compare_results(current, baseline)


class DiscoverySafetyTests(unittest.TestCase):
    def test_absent_state_directory_skips_list(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch("benchmarks.commands.Path.home", return_value=Path(directory)):
                with patch("benchmarks.commands.benchmark_command", return_value={"median_ms": 1}) as run:
                    rows = benchmark_discovery("pvm", iterations=1, samples=1)
        self.assertEqual(run.call_args_list[0].args, ("pvm", ["current"]))
        self.assertEqual(run.call_count, 1)
        self.assertEqual(rows[1]["command"], "list")
        self.assertIn("skipped", rows[1])


if __name__ == "__main__":
    unittest.main()
