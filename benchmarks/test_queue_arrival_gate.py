#!/usr/bin/env python3
"""Contracts for the queue arrival/service gate (P03-MG-002)."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from typing import Any

from benchmarks import queue_arrival_gate as G


def _packet() -> dict[str, Any]:
    return {
        "status": "ESTIMATE_ONLY_MEASUREMENT_GAP",
        "measurement_gap": {"candidate_id": "P03-MG-002"},
        "rows": [
            {
                "size_class": "1KB",
                "arrival_rate_ops_per_second": {
                    "value": 1.0,
                    "provenance": "estimated_scenario",
                },
                "service_mean_seconds_proxy": {
                    "value": 0.003,
                    "provenance": "estimated_from_measured_p50",
                },
            }
        ],
    }


def _north() -> dict[str, Any]:
    return {
        "snapshot_id": "test",
        "compression": {"workloads": [], "totals": {}},
        "expand": [{"size_class": "1KB", "p50_ms": 3.0, "samples": 10}],
    }


class QueueArrivalGateTests(unittest.TestCase):
    def _root(self, samples: dict[str, Any] | None = None) -> Path:
        root = Path(tempfile.mkdtemp())
        north = root / "benchmarks" / "northstar"
        north.mkdir(parents=True)
        (north / "current.json").write_text(json.dumps(_north()) + "\n")
        packet_dir = (
            root
            / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence"
        )
        packet_dir.mkdir(parents=True)
        (packet_dir / "queue-bound.json").write_text(json.dumps(_packet()) + "\n")
        if samples is not None:
            claims = root / "benchmarks" / "claims"
            claims.mkdir(parents=True)
            (claims / "queue-arrival-samples.json").write_text(
                json.dumps(samples) + "\n"
            )
        return root

    def test_accepts_estimate_only_without_samples(self) -> None:
        out = G.evaluate(self._root())
        self.assertEqual(out["result"], "ACCEPT_GATED_MEASUREMENT_GAP")
        self.assertEqual(out["packet_status"], "ESTIMATE_ONLY_MEASUREMENT_GAP")

    def test_accepts_measured_when_observed_dist_complete(self) -> None:
        samples = {
            "observed": True,
            "size_classes": [
                {
                    "size_class": "1KB",
                    "concurrency": 1,
                    "sample_count": 10,
                    "arrival_rate_ops_per_second": 5.0,
                    "arrival_rate_ops_per_second_provenance": "observed_harness",
                    "service_mean_seconds": 0.01,
                    "service_mean_seconds_provenance": "observed_harness",
                    "service_variance_seconds2": 1e-6,
                    "provenance": "observed_harness",
                }
            ],
        }
        out = G.evaluate(self._root(samples))
        self.assertEqual(out["result"], "ACCEPT_MEASURED")

    def test_rejects_false_stability_when_packet_upgraded_without_samples(self) -> None:
        root = self._root()
        packet_path = (
            root
            / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence/queue-bound.json"
        )
        bad = _packet()
        bad["status"] = "STABLE"
        packet_path.write_text(json.dumps(bad) + "\n")
        out = G.evaluate(root)
        self.assertEqual(out["result"], "REJECT_FALSE_STABILITY_CLAIM")


if __name__ == "__main__":
    unittest.main()
