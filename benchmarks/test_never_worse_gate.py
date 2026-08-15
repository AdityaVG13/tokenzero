from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("never_worse_gate.py")


def receipt(
    *rows: str,
    unit_id: str = "estimator:bytes-ceil-div4/v1",
    surface_id: str = "captured-stdout-bytes/v1",
    suite: str = "test-suite",
) -> str:
    return "\n".join(
        [
            "schema_version\tnever-worse/v1",
            f"suite\t{suite}",
            f"surface_id\t{surface_id}",
            f"unit_id\t{unit_id}",
            "task\tcandidate_bytes\traw_bytes\tcandidate_units\traw_units",
            *rows,
            "",
        ]
    )


class NeverWorseGateTests(unittest.TestCase):
    def run_gate(self, content: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.tsv"
            path.write_text(content, encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(SCRIPT), str(path)],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_equal_and_better_rows_pass(self) -> None:
        result = self.run_gate(receipt("read\t4\t8\t1\t2", "edit\t5\t5\t2\t2"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Result: PASS", result.stdout)

    def test_worse_row_fails(self) -> None:
        result = self.run_gate(receipt("read\t9\t8\t3\t2"))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("**FAIL**", result.stdout)

    def test_count_or_unit_mismatch_fails_closed(self) -> None:
        wrong_count = self.run_gate(receipt("read\t8\t8\t3\t2"))
        self.assertEqual(wrong_count.returncode, 2)
        self.assertIn("count mismatch", wrong_count.stderr)
        wrong_unit = self.run_gate(
            receipt("read\t8\t8\t2\t2", unit_id="provider:unverified")
        )
        self.assertEqual(wrong_unit.returncode, 2)
        self.assertIn("unit_id mismatch", wrong_unit.stderr)
        q99 = self.run_gate(receipt("read\t8\t8\t2\t2", unit_id="Q99-Input"))
        self.assertEqual(q99.returncode, 2)
        self.assertIn("Q99-Input is not a TokenZero product unit", q99.stderr)
        q99_suite = self.run_gate(
            receipt("read\t8\t8\t2\t2", suite="Q99-Input-bakeoff")
        )
        self.assertEqual(q99_suite.returncode, 2)
        self.assertIn("Q99-Input is not a TokenZero product unit", q99_suite.stderr)
        empty = self.run_gate(receipt("read\t0\t8\t0\t2"))
        self.assertEqual(empty.returncode, 2)
        self.assertIn("empty candidate", empty.stderr)

    def test_visible_payload_surface_is_accepted(self) -> None:
        result = self.run_gate(
            receipt(
                "grep_expand_edit_verify\t40\t80\t10\t20",
                surface_id="visible-payload-bytes/v1",
            )
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("visible-payload-bytes/v1", result.stdout)

    def test_unknown_surface_fails_closed(self) -> None:
        result = self.run_gate(
            receipt("read\t4\t8\t1\t2", surface_id="envelope-inclusive-stdout/v0")
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("not a never-worse denominator", result.stderr)
        wrong_suite_surface = self.run_gate(
            receipt(
                "read_50_lines\t4\t8\t1\t2",
                suite="million-line-nav",
                surface_id="captured-stdout-bytes/v1",
            )
        )
        self.assertEqual(wrong_suite_surface.returncode, 2)
        self.assertIn("must use surface", wrong_suite_surface.stderr)

    def test_duplicate_or_missing_rows_fail_closed(self) -> None:
        duplicate = self.run_gate(receipt("read\t4\t8\t1\t2", "read\t4\t8\t1\t2"))
        self.assertEqual(duplicate.returncode, 2)
        self.assertIn("duplicate task", duplicate.stderr)
        missing = self.run_gate(receipt())
        self.assertEqual(missing.returncode, 2)
        self.assertIn("task rows", missing.stderr)


if __name__ == "__main__":
    unittest.main()
