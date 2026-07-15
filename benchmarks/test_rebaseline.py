#!/usr/bin/env python3
"""Focused contracts for northstar rebaseline aggregation."""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from typing import Any

from benchmarks import rebaseline as R


class RebaselineTests(unittest.TestCase):
    def snapshot(self, snapshot_id: str, savings: float, boot: int, expand: float) -> dict[str, Any]:
        return {
            "snapshot_id": snapshot_id,
            "environment": {"commit": "abc", "mode": "test"},
            "compression": {
                "workloads": [
                    {
                        "workload": "read",
                        "raw_tokens": 100,
                        "visible_tokens": 10,
                        "savings_pct": savings,
                    }
                ],
                "totals": {
                    "raw_tokens": 100,
                    "visible_tokens": 10,
                    "savings_pct": savings,
                },
            },
            "boot": [
                {
                    "corpus": "repository",
                    "files": 10,
                    "boot_tokens": boot,
                    "components": {},
                }
            ],
            "expand": [
                {
                    "size_class": "1KB",
                    "samples": 5,
                    "p50_ms": expand,
                    "p95_ms": expand + 1,
                    "p99_ms": expand + 2,
                }
            ],
        }

    def test_normalizers_and_markdown_retain_every_row(self) -> None:
        compression = R.compression_from_jsonl(
            '\n'.join(
                [
                    '{"workload":"read","raw_tokens":100,"tokenzero_visible_tokens":10,"savings_pct":90}',
                    '{"workload":"find","raw_tokens":200,"tokenzero_visible_tokens":20,"savings_pct":90}',
                    '{"workload":"TOTAL","raw_tokens":300,"tokenzero_visible_tokens":30,"savings_pct":90}',
                ]
            )
        )
        self.assertEqual([row["workload"] for row in compression["workloads"]], ["read", "find"])
        snapshot = self.snapshot("one", 90.0, 21, 3.0)
        snapshot["compression"] = compression
        snapshot["trend"] = {"previous_snapshot": None, "deltas": {}, "losses": []}
        markdown = R.render_markdown(snapshot)
        self.assertIn("| read | 100 | 10 | 90.0% |", markdown)
        self.assertIn("| find | 200 | 20 | 90.0% |", markdown)
        self.assertIn("| repository | 10 | 21 |", markdown)
        self.assertIn("| 1KB | 5 | 3.000 ms | 4.000 ms | 5.000 ms |", markdown)

    def test_trend_publishes_every_regression(self) -> None:
        previous = self.snapshot("before", 90.0, 21, 3.0)
        current = self.snapshot("after", 88.0, 24, 4.5)
        result = R.trend(previous, current)
        self.assertEqual(result["previous_snapshot"], "before")
        self.assertEqual(result["deltas"]["headline_savings_pct"], -2.0)
        self.assertEqual(result["deltas"]["boot_tokens"]["repository"], 3)
        self.assertEqual(result["deltas"]["expand_p50_ms"]["1KB"], 1.5)
        self.assertEqual(len(result["losses"]), 3)

    def test_write_outputs_keeps_history_and_current_report(self) -> None:
        snapshot = self.snapshot("stored", 90.0, 21, 3.0)
        snapshot["trend"] = {"previous_snapshot": None, "deltas": {}, "losses": []}
        with tempfile.TemporaryDirectory() as raw_temp:
            root = Path(raw_temp)
            history, current, report = R.write_outputs(snapshot, root)
            self.assertTrue(history.is_file())
            self.assertEqual(history.read_text(), current.read_text())
            self.assertIn("# TokenZero Northstar", report.read_text())


if __name__ == "__main__":
    raise SystemExit(unittest.main(verbosity=2))
