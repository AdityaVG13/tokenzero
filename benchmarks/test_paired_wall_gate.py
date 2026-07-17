#!/usr/bin/env python3
"""Contracts for the paired wall-time speedup gate (P03-MG-001)."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from typing import Any

from benchmarks import paired_wall_gate as G


def _north(workloads: list[dict[str, Any]]) -> dict[str, Any]:
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


def _packet() -> dict[str, Any]:
    return {
        "measurement_gap": {
            "candidate_id": "P03-MG-001",
            "needed": "paired raw and TokenZero wall-time samples",
        },
        "limits": [
            "This is a conditional token-work ceiling, not a measured wall-time speedup."
        ],
        "results": {"conditional_aggregate_ceiling_x": 10.0},
    }


class PairedWallGateTests(unittest.TestCase):
    def _root(
        self,
        workloads: list[dict[str, Any]],
        samples: dict[str, Any] | None = None,
    ) -> Path:
        root = Path(tempfile.mkdtemp())
        north = root / "benchmarks" / "northstar"
        north.mkdir(parents=True)
        (north / "current.json").write_text(
            json.dumps(_north(workloads), indent=2) + "\n"
        )
        packet_dir = (
            root
            / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence"
        )
        packet_dir.mkdir(parents=True)
        (packet_dir / "speedup-ceiling.json").write_text(
            json.dumps(_packet(), indent=2) + "\n"
        )
        if samples is not None:
            claims = root / "benchmarks" / "claims"
            claims.mkdir(parents=True)
            (claims / "paired-wall-samples.json").write_text(
                json.dumps(samples, indent=2) + "\n"
            )
        return root

    def test_accepts_gated_without_samples(self) -> None:
        root = self._root(
            [{"workload": "read", "raw_tokens": 100, "visible_tokens": 10}]
        )
        out = G.evaluate(root)
        self.assertEqual(out["result"], "ACCEPT_GATED_MEASUREMENT_GAP")
        self.assertEqual(out["measurement_gap_candidate_id"], "P03-MG-001")
        self.assertTrue(out["ceiling_explicitly_not_wall_time"])

    def test_accepts_measured_when_all_workloads_paired(self) -> None:
        name = "read"
        root = self._root(
            [{"workload": name, "raw_tokens": 100, "visible_tokens": 10}],
            samples={
                "environment_id": "ci",
                "workloads": [
                    {
                        "workload": name,
                        "environment_id": "ci",
                        "raw_wall_ms": {"n": 3, "p50": 10.0, "p95": 12.0},
                        "tokenzero_wall_ms": {"n": 3, "p50": 1.0, "p95": 1.2},
                    }
                ],
            },
        )
        out = G.evaluate(root)
        self.assertEqual(out["result"], "ACCEPT_MEASURED")
        self.assertEqual(out["workloads_covered"], [name])

    def test_stays_gated_when_sample_incomplete(self) -> None:
        name = "read"
        root = self._root(
            [{"workload": name, "raw_tokens": 100, "visible_tokens": 10}],
            samples={
                "workloads": [
                    {
                        "workload": name,
                        "raw_wall_ms": {"n": 3, "p50": 10.0},
                        "tokenzero_wall_ms": {"n": 3, "p50": 1.0, "p95": 1.2},
                    }
                ],
            },
        )
        out = G.evaluate(root)
        self.assertEqual(out["result"], "ACCEPT_GATED_MEASUREMENT_GAP")
        self.assertEqual(out["workloads_incomplete_samples"], [name])


if __name__ == "__main__":
    unittest.main()
