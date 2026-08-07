from __future__ import annotations

import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from benchmarks.harness import first_blob_ref, measure_median

BLOB = "tz://blob/" + "a" * 64
ORDINAL = "tz://o/2/1"


class FirstBlobRefTests(unittest.TestCase):
    def test_full_envelope_selects_blob_kind(self) -> None:
        self.assertEqual(
            first_blob_ref(
                {
                    "refs": [
                        {"kind": "file", "ref": "tz://file/f123"},
                        {"kind": "blob", "ref": BLOB},
                    ]
                }
            ),
            BLOB,
        )

    def test_slim_envelope_accepts_only_first_durable_primary_ref(self) -> None:
        self.assertEqual(
            first_blob_ref({"refs": [ORDINAL, "tz://file/f123", "tz://search/h123"]}),
            ORDINAL,
        )
        self.assertEqual(first_blob_ref({"refs": ["tz://file/f123", ORDINAL]}), "")
        self.assertEqual(first_blob_ref({"refs": ["https://invalid", ORDINAL]}), "")
        self.assertEqual(first_blob_ref({"refs": ["tz://o/0/1", ORDINAL]}), "")

    def test_invalid_or_mixed_shapes_fail_closed(self) -> None:
        self.assertEqual(
            first_blob_ref({"refs": [ORDINAL, {"kind": "blob", "ref": BLOB}]}),
            "",
        )
        self.assertEqual(first_blob_ref({"refs": "not-a-list"}), "")
        self.assertEqual(first_blob_ref({"refs": [17]}), "")

    def test_legacy_detail_ref_requires_a_durable_primary_shape(self) -> None:
        self.assertEqual(first_blob_ref({"detail_ref": BLOB}), BLOB)
        self.assertEqual(first_blob_ref({"detail_ref": "tz://file/f123"}), "")

    def test_glob_parser_accepts_slim_and_full_visible_shapes(self) -> None:
        from benchmarks.harness import glob_root_and_first

        text = "# root: /work\nsrc/lib.rs\nsrc/main.rs"
        self.assertEqual(
            glob_root_and_first({"visible": text}), ("/work", "src/lib.rs")
        )
        self.assertEqual(
            glob_root_and_first({"visible": {"text": text}}),
            ("/work", "src/lib.rs"),
        )
        self.assertEqual(glob_root_and_first({"visible": 7}), ("", ""))


class MeasurementFailureTests(unittest.TestCase):
    def test_fallback_sample_failure_stdout_never_becomes_measurement(self) -> None:
        command = "printf 'BAD-MEASUREMENT'; printf 'SAMPLE-FAILURE' >&2; exit 7"
        with mock.patch("benchmarks.harness.shutil.which", return_value=None):
            with self.assertRaisesRegex(
                RuntimeError, r"fallback sample 1 failed with 7: SAMPLE-FAILURE"
            ):
                measure_median("failed-sample", command, runs=1, warmup=0)

    def test_fallback_warmup_failure_is_loud(self) -> None:
        command = "printf 'WARMUP-FAILURE' >&2; exit 8"
        with mock.patch("benchmarks.harness.shutil.which", return_value=None):
            with self.assertRaisesRegex(
                RuntimeError, r"fallback warmup 1 failed with 8: WARMUP-FAILURE"
            ):
                measure_median("failed-warmup", command, runs=1, warmup=1)

    def test_captured_byte_probe_failure_stdout_never_becomes_measurement(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            counter = Path(tmp) / "counter"
            counter_arg = shlex.quote(str(counter))
            command = (
                f"n=$(cat {counter_arg} 2>/dev/null || printf 0); n=$((n + 1)); "
                f"""printf '%s' "$n" > {counter_arg}; """
                """if [ "$n" -ge 2 ]; then """
                "printf 'BAD-CAPTURE'; printf 'CAPTURE-FAILURE' >&2; exit 9; fi; "
                "printf 'GOOD-SAMPLE'"
            )
            with mock.patch("benchmarks.harness.shutil.which", return_value=None):
                with self.assertRaisesRegex(
                    RuntimeError,
                    r"captured-byte probe failed with 9: CAPTURE-FAILURE",
                ):
                    measure_median("failed-capture", command, runs=1, warmup=0)
            self.assertEqual(counter.read_text(), "2")

    def test_present_hyperfine_failure_never_selects_fallback(self) -> None:
        calls: list[list[str]] = []

        def fake_run(argv: list[str], **_: object) -> subprocess.CompletedProcess:
            calls.append(argv)
            if argv[0] == "bash":
                return subprocess.CompletedProcess(argv, 0, stdout=b"", stderr=b"")
            self.assertEqual(argv[0], "/fake/hyperfine")
            return subprocess.CompletedProcess(
                argv,
                12,
                stdout="ignored",
                stderr="HYPERFINE-COMMAND-FAILURE",
            )

        with (
            mock.patch(
                "benchmarks.harness.shutil.which", return_value="/fake/hyperfine"
            ),
            mock.patch("benchmarks.harness.subprocess.run", side_effect=fake_run),
        ):
            with self.assertRaisesRegex(
                RuntimeError,
                r"hyperfine execution failed with 12: HYPERFINE-COMMAND-FAILURE",
            ):
                measure_median("failed-hyperfine", "printf bad", runs=1, warmup=0)
        self.assertEqual(len(calls), 2, "failed hyperfine must not enter fallback")

    def test_invalid_hyperfine_samples_never_select_fallback(self) -> None:
        calls: list[list[str]] = []

        def fake_run(argv: list[str], **_: object) -> subprocess.CompletedProcess:
            calls.append(argv)
            if argv[0] == "bash":
                return subprocess.CompletedProcess(argv, 0, stdout=b"", stderr=b"")
            artifact = Path(argv[argv.index("--export-json") + 1])
            artifact.write_text('{"results":[{"times":[]}]}')
            return subprocess.CompletedProcess(argv, 0, stdout="", stderr="")

        with (
            mock.patch(
                "benchmarks.harness.shutil.which", return_value="/fake/hyperfine"
            ),
            mock.patch("benchmarks.harness.subprocess.run", side_effect=fake_run),
        ):
            with self.assertRaisesRegex(
                RuntimeError, r"hyperfine timing artifact has invalid samples"
            ):
                measure_median("invalid-hyperfine", "printf bad", runs=1, warmup=0)
        self.assertEqual(len(calls), 2, "invalid artifact must not enter fallback")

    def test_missing_hyperfine_uses_checked_fallback(self) -> None:
        with mock.patch("benchmarks.harness.shutil.which", return_value=None):
            wall_ms, output_bytes, estimated_units = measure_median(
                "fallback-ok", "printf ok", runs=1, warmup=0
            )
        self.assertGreaterEqual(wall_ms, 0)
        self.assertEqual((output_bytes, estimated_units), (2, 1))

    def test_failure_stderr_is_real_and_bounded(self) -> None:
        command = """python3 -c 'import sys; sys.stderr.write("x" * 5000 + "-TAIL-SENTINEL"); sys.exit(6)'"""
        with mock.patch("benchmarks.harness.shutil.which", return_value=None):
            with self.assertRaises(RuntimeError) as raised:
                measure_median("bounded-stderr", command, runs=1, warmup=0)
        message = str(raised.exception)
        self.assertIn("[... stderr truncated ...]", message)
        self.assertIn("-TAIL-SENTINEL", message)
        self.assertLess(len(message), 4300)


if __name__ == "__main__":
    unittest.main()
