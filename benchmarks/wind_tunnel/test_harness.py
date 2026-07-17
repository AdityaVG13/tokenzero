"""Unit tests for the wind-tunnel replay MVP (no cargo, seconds)."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from benchmarks.wind_tunnel import harness as H
from benchmarks.wind_tunnel.policies import POLICIES
from benchmarks.wind_tunnel.types import Action

FIXTURES = Path(__file__).resolve().parent / "fixtures"


class WindTunnelReplayTests(unittest.TestCase):
    def test_extract_actions_from_fixture(self) -> None:
        journal = H.load_journal(FIXTURES / "session-mixed.json")
        actions = H.extract_actions(journal)
        self.assertEqual(
            [a.method for a in actions],
            ["zero.token.tree", "zero.token.read", "zero.token.shell"],
        )

    def test_identity_matches_on_fixtures(self) -> None:
        report = H.run_corpus(
            sorted(FIXTURES.glob("*.json")),
            "identity",
            "identity",
        )
        self.assertTrue(report["match"])
        self.assertEqual(report["divergences_n"], 0)
        self.assertEqual(report["schema"], H.SCHEMA)

    def test_drop_shell_diverges_on_mixed_session(self) -> None:
        diff = H.diff_journal(
            FIXTURES / "session-mixed.json",
            "identity",
            "drop_shell",
        )
        self.assertFalse(diff.match)
        self.assertEqual(diff.first_divergence, 2)
        self.assertEqual(len(diff.baseline), 3)
        self.assertEqual(len(diff.candidate), 2)

    def test_collapse_compact_many_diverges(self) -> None:
        diff = H.diff_journal(
            FIXTURES / "session-compact.json",
            "identity",
            "collapse_compact_many",
        )
        self.assertFalse(diff.match)
        self.assertEqual(diff.candidate[0].method, "zero.token.compact")

    def test_cli_exit_codes(self) -> None:
        self.assertEqual(
            H.main(
                [
                    "--journals",
                    str(FIXTURES),
                    "--baseline",
                    "identity",
                    "--candidate",
                    "identity",
                    "--quiet",
                ]
            ),
            0,
        )
        self.assertEqual(
            H.main(
                [
                    "--journals",
                    str(FIXTURES),
                    "--baseline",
                    "identity",
                    "--candidate",
                    "drop_shell",
                    "--quiet",
                ]
            ),
            1,
        )

    def test_cli_missing_dir_is_usage_error(self) -> None:
        missing = Path(tempfile.mkdtemp()) / "nope"
        self.assertEqual(
            H.main(["--journals", str(missing), "--quiet"]),
            2,
        )

    def test_policy_registry_complete(self) -> None:
        self.assertIn("identity", POLICIES)
        self.assertIn("drop_shell", POLICIES)
        recorded = [
            Action(0, "a", "zero.token.shell"),
            Action(1, "b", "zero.token.read"),
        ]
        self.assertEqual(
            [a.method for a in POLICIES["drop_shell"](recorded)],
            ["zero.token.read"],
        )

    def test_report_roundtrip_json(self) -> None:
        report = H.run_corpus(
            [FIXTURES / "session-compact.json"],
            "identity",
            "identity",
        )
        # Ensure the report is JSON-serializable for --output.
        raw = json.dumps(report)
        self.assertIn("tokenzero.wind-tunnel-replay.v1", raw)


if __name__ == "__main__":
    unittest.main()
