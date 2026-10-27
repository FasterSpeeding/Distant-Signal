"""Tests for scripts/measure-transit-trip-planning.py's budget checks.

  uv run python -m unittest discover -s scripts/tests

Hand-made results only: `measure` needs a running api, so only `compare`'s
checks (the golden rule, the latency and memory budgets) are tested.
"""

import importlib.util
import io
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from types import ModuleType
from typing import Any

SCRIPT = Path(__file__).resolve().parent.parent / "measure-transit-trip-planning.py"


def _load() -> ModuleType:
    spec = importlib.util.spec_from_file_location("measure_transit", SCRIPT)
    if spec is None or spec.loader is None:
        msg = f"cannot load {SCRIPT}"
        raise ImportError(msg)
    module = importlib.util.module_from_spec(spec)
    sys.modules["measure_transit"] = module
    spec.loader.exec_module(module)
    return module


measure = _load()


def result(
    samples: list[tuple[str, float, str]], rss: int | None = None
) -> dict[str, Any]:
    """Build a run from `(mode, seconds, sha256)` samples of one query."""
    memory = {} if rss is None else {"end_rss_bytes": rss}
    return {
        "label": "x",
        "date": "2026-11-04",
        "transit": True,
        "samples": [
            {
                "query": "SPT-LDS@08:00",
                "mode": mode,
                "seconds": [seconds],
                "status": 200,
                "sha256": sha,
            }
            for mode, seconds, sha in samples
        ],
        "memory": memory,
    }


class CompareTests(unittest.TestCase):
    """The golden rule and the two budgets."""

    def test_within_budget_passes(self) -> None:
        """5% slower and 20 MB more is within the budget."""
        off = result([("fastest", 1.0, "a"), ("options", 2.0, "b")], rss=100 * 2**20)
        on = result(
            [
                ("fastest", 1.0, "a"),
                ("options", 2.0, "b"),
                ("fastest+transit", 1.05, "c"),
                ("options+transit", 2.1, "d"),
            ],
            rss=120 * 2**20,
        )
        with redirect_stdout(io.StringIO()):
            self.assertEqual(measure.latency_failures(off, on), [])
            self.assertEqual(measure.memory_failures(off, on), [])
        self.assertEqual(measure.golden_failures(off, on), [])

    def test_a_changed_response_without_the_opt_in_fails(self) -> None:
        """The golden rule: the same request, a different body."""
        off = result([("fastest", 1.0, "a")])
        on = result([("fastest", 1.0, "z")])
        self.assertEqual(len(measure.golden_failures(off, on)), 1)

    def test_over_budget_fails(self) -> None:
        """12% slower and 30 MB more are both over."""
        off = result([("fastest", 1.0, "a")], rss=100 * 2**20)
        on = result([("fastest+transit", 1.12, "c")], rss=130 * 2**20)
        with redirect_stdout(io.StringIO()):
            self.assertEqual(len(measure.latency_failures(off, on)), 1)
            self.assertEqual(len(measure.memory_failures(off, on)), 1)


if __name__ == "__main__":
    unittest.main()
