"""Reusable performance checks for pvm; Python 3.9+, standard library only."""

from .commands import benchmark_discovery, benchmark_startup
from .hooks import benchmark_hooks

__all__ = ["benchmark_hooks", "benchmark_startup", "benchmark_discovery"]
