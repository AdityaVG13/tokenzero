#!/usr/bin/env python3
"""Contracts for the per-workload token expansion gate (P03-001)."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from typing import Any

from benchmarks import workload_token_gate as G


def _snapshot(workloads: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "snapshot_id": "test-snap",
        "compression": {
            "workloads": workloads,
            "totals": {
                "raw_tokens": sum(int(w["raw_tokens"]) for w in workloads),
                "visible_tokens": sum(int(w["visible_tokens"]) for w in workloads),
                "savings_pct": 99.0,
            },
        },
    }


class WorkloadTokenGateTests(unittest.TestCase):
    def _write_root(self, workloads: list[dict[str, Any]]) -> Path:
        root = Path(tempfile.mkdtemp())
        north = root / "benchmarks" / "northstar"
        north.mkdir(parents=True)
        (north / "current.json").write_text(
            json.dumps(_snapshot(workloads), indent=2) + "\n"
        )
        return root

    def test_accepts_known_cargo_test_expander_only(self) -> None:
        root = self._write_root(
            [
                {
                    "workload": "read large source file",
                    "raw_tokens": 100,
                    "visible_tokens": 10,
                    "savings_pct": 90.0,
                },
                {
                    "workload": "cargo test run (tokenzero-filters suite)",
                    "raw_tokens": 233,
                    "visible_tokens": 257,
                    "savings_pct": -11.0,
                },
            ]
        )
        out = G.evaluate(root)
        self.assertEqual(out["result"], "ACCEPT_GATED")
        self.assertFalse(out["universal_token_work_savings"])
        self.assertEqual(len(out["expanders"]), 1)

    def test_rejects_unexpected_expander(self) -> None:
        root = self._write_root(
            [
                {
                    "workload": "cargo test run (tokenzero-filters suite)",
                    "raw_tokens": 233,
                    "visible_tokens": 257,
                    "savings_pct": -11.0,
                },
                {
                    "workload": "repo-wide grep",
                    "raw_tokens": 100,
                    "visible_tokens": 150,
                    "savings_pct": -50.0,
                },
            ]
        )
        out = G.evaluate(root)
        self.assertEqual(out["result"], "REJECT_UNEXPECTED_EXPANSION")
        self.assertEqual(len(out["unexpected_expanders"]), 1)

    def test_accepts_zero_expanders_as_universal(self) -> None:
        root = self._write_root(
            [
                {
                    "workload": "read",
                    "raw_tokens": 100,
                    "visible_tokens": 10,
                    "savings_pct": 90.0,
                }
            ]
        )
        out = G.evaluate(root)
        self.assertEqual(out["result"], "ACCEPT_GATED")
        self.assertTrue(out["universal_token_work_savings"])


if __name__ == "__main__":
    unittest.main()
