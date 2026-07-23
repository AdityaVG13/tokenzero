#!/usr/bin/env python3
"""Contracts for the boot-cost CI lock."""
from __future__ import annotations

import copy
import importlib.util
import unittest
from pathlib import Path
from types import ModuleType
from typing import Any


def _load_boot_cost() -> ModuleType:
    path = Path(__file__).with_name("boot-cost.py")
    spec = importlib.util.spec_from_file_location("tokenzero_boot_cost", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


boot_cost = _load_boot_cost()
BASELINE: dict[str, Any] = {
    "thresholds": {
        "max_visible_boot_tokens_exclusive": 100,
        "max_repo_size_growth_tokens": 0,
        "small_corpus": "repository",
        "large_corpus": "synthetic-23k",
    },
    "components": {
        "repository": {"manifest": 12, "delta": 3, "toc_working_set": 6, "other": 0},
        "synthetic-23k": {"manifest": 12, "delta": 3, "toc_working_set": 6, "other": 0},
    },
}


def corpus(label: str, *, manifest: int = 12, delta: int = 3) -> dict[str, object]:
    components = {
        "manifest": manifest,
        "delta": delta,
        "toc_working_set": 6,
        "other": 0,
    }
    components["total"] = sum(components.values())
    return {
        "label": label,
        "boot_tokens": components["total"],
        "components": components,
    }


class BootCostGateTests(unittest.TestCase):
    def test_accepts_constant_sub_100_boot(self) -> None:
        result = boot_cost.evaluate_gate(
            [corpus("repository"), corpus("synthetic-23k")], copy.deepcopy(BASELINE)
        )
        self.assertTrue(result["all_passed"])
        self.assertEqual(result["measured_repo_size_growth_tokens"], 0)

    def test_over_budget_failure_attributes_component(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, r"corpus=repository.*component=manifest.*baseline_delta=\+89"
        ):
            boot_cost.evaluate_gate(
                [corpus("repository", manifest=101), corpus("synthetic-23k")],
                copy.deepcopy(BASELINE),
            )

    def test_repo_size_growth_failure_attributes_component(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, r"growth_tokens=1.*epsilon=0.*component=manifest"
        ):
            boot_cost.evaluate_gate(
                [corpus("repository"), corpus("synthetic-23k", manifest=13)],
                copy.deepcopy(BASELINE),
            )


if __name__ == "__main__":
    unittest.main()
