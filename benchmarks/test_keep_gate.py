from __future__ import annotations

import inspect
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import keep_gate

SCRIPT = Path(__file__).with_name("keep_gate.py")
HISTORY = (
    Path(__file__).resolve().parents[1]
    / ".bench-history"
    / "tokenzero-core.hotpaths.latest.json"
)


def _doc(groups: list[dict], **extra: object) -> dict:
    body: dict = {
        "schema": keep_gate.SCHEMA,
        "benchmark_id": "tokenzero-core.hotpaths",
        "primary": "count_tokens",
        "label": "fixture-seed",
        "note": (
            "Synthetic fixture-seed baseline for the keep-gate ratchet. "
            "Not a live unlabeled measurement percentage."
        ),
        "groups": groups,
    }
    body.update(extra)
    return body


def _write(path: Path, document: dict) -> None:
    path.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


class KeepGateUnitTests(unittest.TestCase):
    def test_named_constants(self) -> None:
        self.assertEqual(keep_gate.KEEP_GATE_GEOMEAN_PCT, 3.0)
        self.assertEqual(keep_gate.KEEP_GATE_PASS_PCT, 5.0)
        self.assertEqual(keep_gate.CV_PCT_QUARANTINE, 5.0)
        self.assertEqual(
            keep_gate.ALLOWED_LABELS, frozenset({"fixture-seed", "live"})
        )
        # persist + keep share KEEP_GATE_GEOMEAN_PCT (not a leftover 25%).
        params = inspect.signature(keep_gate.persist_gate).parameters
        self.assertEqual(
            params["geomean_band_pct"].default, keep_gate.KEEP_GATE_GEOMEAN_PCT
        )
        params_c = inspect.signature(keep_gate.compare_to_history).parameters
        self.assertEqual(
            params_c["geomean_band_pct"].default, keep_gate.KEEP_GATE_GEOMEAN_PCT
        )

    def test_cv_pct_and_quarantine(self) -> None:
        stable = {"name": "stable", "samples": [100.0, 101.0, 99.0]}
        noisy = {"name": "noisy", "samples": [100.0, 200.0, 50.0]}
        self.assertLessEqual(keep_gate.cv_pct(stable["samples"]), 5.0)
        self.assertGreater(keep_gate.cv_pct(noisy["samples"]), 5.0)
        kept, quarantined = keep_gate.quarantine_groups([stable, noisy])
        self.assertEqual([g["name"] for g in kept], ["stable"])
        self.assertEqual([g["name"] for g in quarantined], ["noisy"])

    def test_all_quarantined_fails_closed(self) -> None:
        noisy = {"name": "only_noisy", "cv_pct": 31.0, "mean": 100.0}
        with self.assertRaises(keep_gate.KeepGateError) as ctx:
            keep_gate.quarantine_groups([noisy])
        self.assertIn("all primary groups quarantined", str(ctx.exception))

    def test_seed_history_compare_passes(self) -> None:
        history = keep_gate.load_history(HISTORY)
        self.assertEqual(history.get("label"), "fixture-seed")
        passed, messages = keep_gate.compare_to_history(history, history)
        self.assertTrue(passed, messages)
        self.assertTrue(any(line.startswith("PASS geomean") for line in messages))

    def test_plus_ten_percent_fails_compare_and_persist(self) -> None:
        history = keep_gate.load_history(HISTORY)
        worse_groups = []
        for group in history["groups"]:
            samples = [float(v) * 1.10 for v in group["samples"]]
            worse_groups.append(
                {
                    "name": group["name"],
                    "samples": samples,
                    "mean_ns": sum(samples) / len(samples),
                    "cv_pct": keep_gate.cv_pct(samples),
                }
            )
        current = _doc(worse_groups)
        compare_ok, compare_msgs = keep_gate.compare_to_history(current, history)
        persist_ok, persist_msgs = keep_gate.persist_gate(current, history)
        self.assertFalse(compare_ok, compare_msgs)
        self.assertFalse(persist_ok, persist_msgs)
        self.assertTrue(any("FAIL geomean" in line for line in compare_msgs))
        self.assertTrue(any("FAIL geomean" in line for line in persist_msgs))

    def test_quarantined_group_excluded_from_compare(self) -> None:
        history = _doc(
            [
                {"name": "stable", "samples": [100.0, 100.0, 100.0]},
                {"name": "noisy", "samples": [100.0, 100.0, 100.0]},
            ]
        )
        current = _doc(
            [
                # within pass band vs history stable
                {"name": "stable", "samples": [101.0, 101.0, 101.0]},
                # would be a huge regression if averaged in, but cv quarantines it
                {"name": "noisy", "samples": [100.0, 400.0, 50.0]},
            ]
        )
        passed, messages = keep_gate.compare_to_history(current, history)
        self.assertTrue(passed, messages)
        self.assertTrue(any("quarantined" in line for line in messages))
        self.assertTrue(any("PASS pass stable" in line for line in messages))
        self.assertFalse(any("pass noisy" in line for line in messages))

    def test_omitted_history_group_fails_closed(self) -> None:
        history = _doc(
            [
                {"name": "stable", "samples": [100.0, 100.0, 100.0]},
                {"name": "render_shell", "samples": [200.0, 200.0, 200.0]},
            ]
        )
        current = _doc(
            [
                {"name": "stable", "samples": [101.0, 101.0, 101.0]},
            ]
        )
        with self.assertRaises(keep_gate.KeepGateError) as ctx:
            keep_gate.compare_to_history(current, history)
        self.assertIn("history groups missing from current", str(ctx.exception))
        self.assertIn("render_shell", str(ctx.exception))

    def test_unlabeled_history_refuses_as_live(self) -> None:
        groups = [{"name": "stable", "samples": [100.0, 100.0, 100.0]}]
        labeled = _doc(groups)
        unlabeled = _doc(groups)
        del unlabeled["label"]
        unlabeled.pop("note", None)
        with self.assertRaises(keep_gate.KeepGateError) as persist_ctx:
            keep_gate.persist_gate(unlabeled, labeled)
        persist_msg = str(persist_ctx.exception).lower()
        self.assertIn("unlabeled", persist_msg)
        self.assertIn("live", persist_msg)
        with self.assertRaises(keep_gate.KeepGateError) as compare_ctx:
            keep_gate.compare_to_history(unlabeled, labeled)
        self.assertIn("unlabeled", str(compare_ctx.exception).lower())

        missing_note = _doc(groups)
        missing_note["note"] = ""
        with self.assertRaises(keep_gate.KeepGateError) as note_ctx:
            keep_gate.persist_gate(missing_note, labeled)
        self.assertIn("missing note", str(note_ctx.exception).lower())

    def test_q99_identity_refuses(self) -> None:
        groups = [{"name": "stable", "samples": [100.0, 100.0, 100.0]}]
        q99 = _doc(groups, note="Q99-Input estimator disguised as latency")
        labeled = _doc(groups)
        with self.assertRaises(keep_gate.KeepGateError) as ctx:
            keep_gate.compare_to_history(q99, labeled)
        self.assertIn("Q99", str(ctx.exception))

    def test_persist_refuses_live_over_fixture_seed(self) -> None:
        groups = [{"name": "stable", "samples": [100.0, 100.0, 100.0]}]
        history = _doc(groups)
        current = _doc(
            groups,
            label="live",
            note="live Criterion release-perf sibling; not fixture-seed",
        )
        with self.assertRaises(keep_gate.KeepGateError) as ctx:
            keep_gate.persist_gate(current, history)
        message = str(ctx.exception)
        self.assertIn("fixture-seed", message)
        self.assertIn("sibling", message)

    def test_benchmark_id_mismatch_fails_closed(self) -> None:
        history = _doc([{"name": "stable", "samples": [100.0, 100.0, 100.0]}])
        current = _doc(
            [{"name": "stable", "samples": [100.0, 100.0, 100.0]}],
            benchmark_id="other.bench",
        )
        with self.assertRaises(keep_gate.KeepGateError) as ctx:
            keep_gate.compare_to_history(current, history)
        self.assertIn("benchmark_id mismatch", str(ctx.exception))

    def test_detect_binary_os_magic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            elf = root / "elf.bin"
            macho = root / "macho.bin"
            elf.write_bytes(b"\x7fELF" + b"\x00" * 12)
            macho.write_bytes(b"\xcf\xfa\xed\xfe" + b"\x00" * 12)
            self.assertEqual(keep_gate.detect_binary_os(elf), "linux")
            self.assertEqual(keep_gate.detect_binary_os(macho), "darwin")

    def test_resolve_bin_refuses_os_mismatch(self) -> None:
        host = keep_gate.host_os()
        wrong_magic = (
            b"\x7fELF" + b"\x00" * 12
            if host == "darwin"
            else b"\xcf\xfa\xed\xfe" + b"\x00" * 12
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tokenzero"
            path.write_bytes(wrong_magic)
            env = os.environ.copy()
            env["TOKENZERO_BIN"] = str(path)
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "resolve-bin"],
                text=True,
                capture_output=True,
                check=False,
                env=env,
            )
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn("refuse", result.stderr.lower())
            self.assertIn("mixup", result.stderr.lower())


class KeepGateCliTests(unittest.TestCase):
    def test_help(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--help"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("compare", result.stdout)
        self.assertIn("persist", result.stdout)
        self.assertIn("resolve-bin", result.stdout)

    def test_dry_run(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--dry-run"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("KEEP_GATE_GEOMEAN_PCT=3.0", result.stdout)
        self.assertIn("CARGO_TARGET_DIR=/tmp/rch_target_tokenzero", result.stdout)

    def test_cli_compare_seed_pass_and_worse_fail(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            history = keep_gate.load_history(HISTORY)
            same = root / "same.json"
            worse = root / "worse.json"
            _write(same, history)
            worse_doc = json.loads(json.dumps(history))
            for group in worse_doc["groups"]:
                group["samples"] = [float(v) * 1.10 for v in group["samples"]]
                group["mean_ns"] = sum(group["samples"]) / len(group["samples"])
                group["cv_pct"] = keep_gate.cv_pct(group["samples"])
            _write(worse, worse_doc)

            ok = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "compare",
                    "--current",
                    str(same),
                    "--history",
                    str(HISTORY),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(ok.returncode, 0, ok.stderr + ok.stdout)
            self.assertIn("Result: PASS", ok.stdout)

            bad = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "compare",
                    "--current",
                    str(worse),
                    "--history",
                    str(HISTORY),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(bad.returncode, 1, bad.stderr + bad.stdout)
            self.assertIn("Result: FAIL", bad.stdout)

            persist_bad = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "persist",
                    "--current",
                    str(worse),
                    "--history",
                    str(HISTORY),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(
                persist_bad.returncode, 1, persist_bad.stderr + persist_bad.stdout
            )
            self.assertIn("KEEP_GATE_GEOMEAN_PCT", persist_bad.stdout)

    def test_cli_persist_unlabeled_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            unlabeled = json.loads(json.dumps(keep_gate.load_history(HISTORY)))
            unlabeled.pop("label", None)
            unlabeled.pop("note", None)
            path = root / "unlabeled.json"
            _write(path, unlabeled)
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "persist",
                    "--current",
                    str(path),
                    "--history",
                    str(HISTORY),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 2, result.stderr + result.stdout)
            self.assertIn("unlabeled", result.stderr.lower())
            self.assertIn("live", result.stderr.lower())


if __name__ == "__main__":
    unittest.main()
