#!/usr/bin/env python3
"""Focused contracts for northstar rebaseline aggregation."""
from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from benchmarks import rebaseline as R


class RebaselineTests(unittest.TestCase):
    def snapshot(self, snapshot_id: str, savings: float, boot: int, expand: float) -> dict[str, Any]:
        return {
            "snapshot_id": snapshot_id,
            "environment": {
                "commit": "abc", "mode": "test", "machine": "arm64",
                "machine_conditions": {"architecture": "arm64", "processor": "test", "node": "host", "cpu_count": 8},
                "os": "test-os", "python": "3.test",
            },
            "methodology": dict(R.METHODOLOGY),
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
            with self.assertRaises(FileExistsError):
                R.write_outputs(snapshot, root)

    def test_trend_reports_incompatible_methodology_without_deltas(self) -> None:
        previous = self.snapshot("before", 90.0, 21, 3.0)
        current = self.snapshot("after", 80.0, 30, 9.0)
        previous["environment"]["os"] = "different-os"
        result = R.trend(previous, current)
        self.assertFalse(result["comparable"])
        self.assertEqual(result["deltas"], {})
        self.assertIn("environment.os differs", result["non_comparable_reasons"][0])
        current["trend"] = result
        self.assertIn("Trend is not comparable", R.render_markdown(current))

    def test_source_state_hash_includes_untracked_file_contents(self) -> None:
        with tempfile.TemporaryDirectory() as raw_temp:
            repo = Path(raw_temp)
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "tracked.txt").write_text("tracked\n")
            subprocess.run(["git", "add", "tracked.txt"], cwd=repo, check=True)
            subprocess.run(
                ["git", "-c", "user.name=test", "-c", "user.email=test@example.invalid", "commit", "-qm", "initial"],
                cwd=repo, check=True,
            )
            untracked = repo / "untracked.txt"
            untracked.write_text("first")
            with mock.patch.object(R, "REPO", repo):
                first = R.source_state_sha256()
                untracked.write_text("second")
                second = R.source_state_sha256()
            self.assertNotEqual(first, second)

    def test_run_components_uses_one_selected_binary(self) -> None:
        with tempfile.TemporaryDirectory() as raw_temp:
            root = Path(raw_temp)
            binary = root / "tokenzero"
            binary.write_bytes(b"selected binary")
            binary.chmod(0o755)
            calls: list[tuple[list[str], dict[str, str] | None]] = []

            def fake_run(command: list[str], *, env: dict[str, str] | None = None) -> str:
                calls.append((command, env))
                if command[0] == "sh":
                    return '{"workload":"TOTAL","raw_tokens":10,"tokenzero_visible_tokens":1,"savings_pct":90}\n'
                return ""

            boot = {"corpora": []}
            expand = {"table": []}
            with mock.patch.object(R, "run_checked", side_effect=fake_run), mock.patch.object(
                R, "load_json", side_effect=lambda path: boot if path == R.BOOT else expand
            ):
                R.run_components(binary, root)
            selected = str(binary.resolve())
            self.assertEqual(calls[0][0][-1], selected)
            for _, env in calls:
                self.assertIsNotNone(env)
                self.assertEqual(env["TOKENZERO_BOOT_BENCH_BIN"], selected)
                self.assertEqual(env["TOKENZERO_EXPAND_BENCH_BIN"], selected)
            provenance = R.binary_provenance(binary)
            self.assertEqual(provenance["sha256"], __import__("hashlib").sha256(binary.read_bytes()).hexdigest())

    def test_reuse_existing_fails_closed(self) -> None:
        with mock.patch.object(sys, "argv", ["rebaseline.py", "--reuse-existing"]):
            with self.assertRaises(SystemExit) as raised:
                R.main()
        self.assertEqual(raised.exception.code, 2)

    def test_gate_failures_return_nonzero(self) -> None:
        benches = R.REPO / "crates/tokenzero-mcp/benches"
        for name in ("literal_search_evidence", "parallel_walker_evidence"):
            spec = importlib.util.spec_from_file_location(name, benches / f"{name}.py")
            assert spec is not None and spec.loader is not None
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            self.assertEqual(module.gate_exit_code({"all_gates_pass": False}), 1)
            self.assertEqual(module.gate_exit_code({"all_gates_pass": True}), 0)

    def test_hot_path_marks_unretained_baseline_non_comparable(self) -> None:
        benches = R.REPO / "crates/tokenzero-mcp/benches"
        spec = importlib.util.spec_from_file_location("find_crates_hot_path", benches / "find_crates_hot_path.py")
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as raw_temp:
            root = Path(raw_temp)
            binary = root / "tokenzero"
            binary.write_bytes(b"binary")
            binary.chmod(0o755)
            sample = {"elapsed_ms": 10.0, "output_sha256": "same", "search_backend": "internal",
                      "matches": 2, "visited_files": 3}
            completed = mock.Mock(stdout="abc123\n")
            with mock.patch.object(module, "BIN", binary), mock.patch.object(
                module, "run_sample", return_value=sample
            ), mock.patch.object(module, "source_state_sha256", return_value="state"), mock.patch.object(
                module.subprocess, "run", return_value=completed
            ), mock.patch.object(module.platform, "platform", return_value="test-os"):
                evidence = module.run(3, 1, root / "evidence.json")
        self.assertFalse(evidence["prior_evidence"]["verified"])
        self.assertFalse(evidence["prior_evidence"]["comparable"])
        self.assertIsNone(evidence["acceptance"]["performance_gate"])
        self.assertNotIn("improvement_pct", evidence["acceptance"])

    def test_report_has_no_trailing_whitespace(self) -> None:
        snapshot = self.snapshot("clean", 90.0, 21, 3.0)
        snapshot["trend"] = {"previous_snapshot": None, "comparable": None,
                             "non_comparable_reasons": [], "deltas": {}, "losses": []}
        self.assertTrue(all(line == line.rstrip() for line in R.render_markdown(snapshot).splitlines()))


if __name__ == "__main__":
    raise SystemExit(unittest.main(verbosity=2))
