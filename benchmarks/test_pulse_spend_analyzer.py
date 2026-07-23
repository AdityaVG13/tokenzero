from __future__ import annotations

import importlib.util
import json
import math
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("pulse_spend_analyzer.py")
SPEC = importlib.util.spec_from_file_location("pulse_spend_analyzer", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class PulseSpendAnalyzerTests(unittest.TestCase):
    def test_classification_is_exhaustive_and_deterministic(self) -> None:
        cases = {
            "tz_expand": "history",
            "tokenzero_read": "content",
            "tz_shell": "plan",
            "tokenzero_compact": "prose",
            "ack": "protocol",
        }
        self.assertEqual(
            {name: MODULE.classify_operation(name) for name in cases}, cases
        )

    def test_analysis_deduplicates_and_accounts_for_every_token(self) -> None:
        events = [
            {"event_id": "1", "tool_kind": "tz_read", "visible_tokens": 30, "capsule_id": "tz://blob/a"},
            {"event_id": "2", "tool_kind": "tz_expand", "visible_tokens": 10, "capsule_id": "tz://blob/a"},
            {"event_id": "3", "tool_kind": "ack", "visible_tokens": 5},
            {"event_id": "1", "tool_kind": "tz_shell", "visible_tokens": 999},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            path.write_text("".join(json.dumps(event) + "\n" for event in events) + "not-json\n")
            report = MODULE.analyze([path])
        self.assertEqual(report["corpus"]["events_analyzed"], 3)
        self.assertEqual(report["corpus"]["duplicate_event_ids_skipped"], 1)
        self.assertEqual(report["corpus"]["malformed_records"], 1)
        self.assertEqual(report["spend"]["total"], 45)
        self.assertEqual(sum(report["spend"]["fractions"].values()), 1.0)
        self.assertEqual(report["rank_frequency"]["refs"]["observations"], 2)

    def test_log_log_fit_recovers_exact_power_law(self) -> None:
        counts = {str(rank): round(1_000_000 / rank**2) for rank in range(1, 21)}
        fit = MODULE.log_log_fit(counts)
        self.assertIsNotNone(fit)
        assert fit is not None
        self.assertAlmostEqual(fit.exponent, 2.0, places=3)
        self.assertLessEqual(fit.confidence_interval_95[0], fit.exponent)
        self.assertGreaterEqual(fit.confidence_interval_95[1], fit.exponent)

    def test_hill_fit_uses_declared_conversion_and_interval(self) -> None:
        counts = {"a": 64, "b": 32, "c": 16, "d": 8, "e": 4}
        fit = MODULE.hill_fit(counts, tail_size=3)
        self.assertIsNotNone(fit)
        assert fit is not None
        expected_alpha = 3 / sum(math.log(value / 8) for value in (64, 32, 16))
        self.assertAlmostEqual(fit.exponent, 1 / expected_alpha)
        self.assertLess(fit.confidence_interval_95[0], fit.exponent)
        self.assertGreater(fit.confidence_interval_95[1], fit.exponent)


if __name__ == "__main__":
    unittest.main()
