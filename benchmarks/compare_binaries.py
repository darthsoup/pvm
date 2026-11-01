#!/usr/bin/env python3
"""Compare discovery in two already-built pvm binaries without switching PHP."""

import argparse
import json
from pathlib import Path
import platform
import sys

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from benchmarks.auto_switch import positive_int
from benchmarks.commands import benchmark_command_pair


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--samples", type=positive_int, default=9)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    for binary in (args.before, args.after):
        if not binary.is_file():
            parser.error("binary not found: " + str(binary))
    result = {"platform": platform.platform(), "before": str(args.before.resolve()),
              "after": str(args.after.resolve()), "commands": []}
    for command in ("--help", "current", "list"):
        if command == "list" and not (Path.home() / ".pvm").is_dir():
            print("Skipping list because ~/.pvm does not exist")
            continue
        row = benchmark_command_pair(args.before.resolve(), args.after.resolve(), command, samples=args.samples)
        result["commands"].append(row)
        print("{}: {:.3f} -> {:.3f} ms ({:+.1f}% improvement); output equal: {}".format(
            command, row["before"]["median_ms"], row["after"]["median_ms"],
            row["improvement_percent"], row["outputs_equal"],
        ), flush=True)
    if args.json:
        args.json.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
