"""Narrow regression: public 99% northstar claim stays gated without population CI."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from benchmarks import claim_public_gate as G


class ClaimPublicGateTests(unittest.TestCase):
    def test_checked_in_tree_accepts_gated_public_claims(self) -> None:
        out = G.evaluate()
        self.assertTrue(out["northstar"]["claim_is_conservative_lower_bound"])
        self.assertFalse(out["statistical_scope"]["population_ci_available"])
        self.assertFalse(out["publication_gate"]["public_claims_approved"])
        self.assertFalse(out["publication_gate"]["release_publication_allowed"])
        self.assertTrue(out["readme_gate"]["gated"])
        self.assertEqual(out["result"], "ACCEPT_GATED_PUBLIC_CLAIMS")

    def test_rejects_when_readme_omits_gate_language(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._copy_artifacts(root)
            readme = (G.REPO / "README.md").read_text()
            for marker in G.REQUIRED_README_GATE_MARKERS:
                readme = readme.replace(marker, "REDACTED")
            (root / "README.md").write_text(readme)
            out = G.evaluate(root)
            self.assertEqual(out["result"], "REJECT_PUBLIC_CLAIMS")
            self.assertFalse(out["readme_gate"]["gated"])
            self.assertTrue(out["readme_gate"]["missing_markers"])

    def test_rejects_premature_public_approval_without_population_ci(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._copy_artifacts(root)
            audit_path = root / "results/current/tokenzero_claim_audit.json"
            audit = json.loads(audit_path.read_text())
            audit["public_claims_approved"] = True
            audit["release_publication_allowed"] = True
            audit["claim_status"] = "approved"
            audit_path.write_text(json.dumps(audit))
            out = G.evaluate(root)
            self.assertEqual(out["result"], "REJECT_PUBLIC_CLAIMS")

    def _copy_artifacts(self, root: Path) -> None:
        for rel in (
            "benchmarks/northstar/current.json",
            "demo/demo_results.json",
            "results/current/tokenzero_claim_audit.json",
            "README.md",
        ):
            src = G.REPO / rel
            dst = root / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_text(src.read_text())


if __name__ == "__main__":
    unittest.main()
