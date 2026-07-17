#!/usr/bin/env python3
"""Contracts for deployment telemetry evidence gate (P17-002)."""
from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from benchmarks import deployment_telemetry_gate as G
from benchmarks.deployment_telemetry_reducer import reduce_ledger

REPO = Path(__file__).resolve().parents[1]


class DeploymentTelemetryGateTests(unittest.TestCase):
    def _clone_repo_bits(self, readme_extra: str = "") -> Path:
        root = Path(tempfile.mkdtemp())
        src = REPO / "benchmarks/claims/deployment-telemetry"
        dst = root / "benchmarks/claims/deployment-telemetry"
        dst.parent.mkdir(parents=True)
        shutil.copytree(src, dst)
        readme = (
            "Across ~20,000 routed tool calls...\n"
            "Treat this as deployment telemetry, not a release claim.\n"
            "Evidence: benchmarks/claims/deployment-telemetry/evidence.json\n"
            + readme_extra
        )
        (root / "README.md").write_text(readme)
        return root

    def test_reducer_on_fixture(self) -> None:
        path = REPO / "benchmarks/claims/deployment-telemetry/fixture-ledger.jsonl"
        out = reduce_ledger(path)
        self.assertEqual(out["call_count"], 4)
        self.assertEqual(out["raw_tokens"], 1000)
        self.assertEqual(out["hidden_tokens"], 300)
        self.assertEqual(out["net_savings"], 0.25)

    def test_accepts_gated_with_evidence_package(self) -> None:
        out = G.evaluate(self._clone_repo_bits())
        self.assertEqual(out["result"], "ACCEPT_GATED_TELEMETRY_EVIDENCE")
        self.assertFalse(out["audited_in_checkout"])

    def test_rejects_without_readme_citation(self) -> None:
        root = self._clone_repo_bits()
        (root / "README.md").write_text("deployment totals with no evidence citation\n")
        out = G.evaluate(root)
        self.assertEqual(out["result"], "REJECT_MISSING_EVIDENCE")


if __name__ == "__main__":
    unittest.main()
