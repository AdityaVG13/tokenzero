#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import ledger_curve
import session_observatory


def record(version: str, session: str, timestamp: int, tool: str, visible: int) -> dict:
    return {
        "schema": "tokenzero.ledger.v1", "timestamp_ms": timestamp,
        "session_id": session, "repo": "/repo", "agent": "test",
        "version": {"crate": version, "git_describe": None}, "tool": tool,
        "token_mass": {"visible_tokens": visible, "raw_tokens": visible * 2,
                       "prevented_tokens": visible // 2, "saved_bytes": visible * 4},
        "cumulative_session_cost_tokens": visible,
        "optimization_tags": ["tool_surface:mcp"],
    }


class LedgerTelemetryTest(unittest.TestCase):
    def test_curve_retains_versions_sessions_and_bad_line_counts(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            path = Path(raw_tmp) / "ledger.jsonl"
            rows = [record("1.0.0", "a", 10, "read", 30),
                    record("1.0.0", "a", 11, "find", 20),
                    record("1.1.0", "b", 20, "read", 40)]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\nnot-json\n{}\n")
            result = ledger_curve.aggregate([path])
            self.assertEqual(result["sample"], {
                "source_files_n": 1, "records_n": 3, "sessions_n": 2,
                "versions_n": 2, "malformed_records_n": 1,
                "ignored_schema_records_n": 1,
            })
            self.assertEqual([row["version"] for row in result["curve"]], ["1.0.0", "1.1.0"])
            self.assertEqual(result["curve"][0]["visible_cost_tokens"]["total"], 50)
            self.assertTrue(any("malformed" in loss for loss in result["losses_disclosed"]))

    def test_observatory_buckets_are_disjoint_and_expand_capture_is_labeled(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            path = Path(raw_tmp) / "ledger.jsonl"
            path.write_text(json.dumps(record("1.2.0", "s", 10, "read", 30)) + "\n")
            capture = {"unledgered_tool_results": [{"tool": "tz_expand", "visible_tokens": 7}]}
            result = session_observatory.observe(path, None, 5, 11, capture)
            self.assertEqual(result["sample"]["turns_n"], 2)
            self.assertEqual(result["totals"]["model_facing_total_tokens"], 53)
            self.assertEqual(result["totals"]["tool_result_visible_tokens"], 37)
            self.assertEqual(result["turns"][1]["cost_buckets"]["expand_materialization_tokens"], 7)
            self.assertEqual(result["turns"][1]["accounting_source"], "mcp_capture")
            for turn in result["turns"]:
                self.assertEqual(sum(turn["cost_buckets"].values()), turn["model_facing_total_tokens"])

    def test_tokenizer_matches_tokenzero_rules(self) -> None:
        self.assertEqual(session_observatory.count_tokens("alpha_beta + 12"), 3)
        self.assertEqual(session_observatory.count_tokens("é"), 1)
        self.assertEqual(session_observatory.count_tokens("a::b"), 4)


if __name__ == "__main__":
    unittest.main()
