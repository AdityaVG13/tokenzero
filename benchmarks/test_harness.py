#!/usr/bin/env python3
"""Parity tests for benchmarks.harness (stdlib only; no TokenZero runtime)."""
from __future__ import annotations
import json, os, subprocess, sys, tempfile, unittest; from pathlib import Path; ROOT = Path(__file__).resolve().parents[1]; sys.path.insert(0, str(ROOT))
from benchmarks import harness as H

class TestMath(unittest.TestCase):

    def test_token_estimate(self) -> None:
        self.assertEqual(H.token_estimate(0), 0); self.assertEqual(H.token_estimate(1), 1); self.assertEqual(H.token_estimate(4), 1); self.assertEqual(H.token_estimate(5), 2)
        self.assertEqual(H.token_estimate(b'abcd'), 1); self.assertEqual(H.token_estimate('ééé'), 2)

    def test_percentiles_match_shell_formula(self) -> None:
        times = [0.01 * i for i in range(1, 11)]; p50, p90, p99 = H.percentiles_ms(times); n = len(times); xs = sorted(times)

        def p(q: float) -> int:
            return int(round(xs[min(n - 1, int(q * (n - 1)))] * 1000))
        self.assertEqual((p50, p90, p99), (p(0.5), p(0.9), p(0.99))); self.assertEqual(H.percentiles_ms([]), [0, 0, 0]); self.assertEqual(H.median_ms([0.1, 0.2, 0.3]), 200)

    def test_quality_and_accounting(self) -> None:
        self.assertEqual(H.quality_check('read_file', json.dumps({'visible': {'text': '[workspace]\n'}})), 'PASS'); self.assertEqual(H.quality_check('edit_verify', json.dumps({'value': {'text': 'alpha\nBETA\ngamma\n'}})), 'PASS'); self.assertEqual(H.quality_check('edit_verify', json.dumps({'value': {'text': 'alpha\nbeta\ngamma\n'}})), 'FAIL'); self.assertEqual(H.quality_check('edit_verify', json.dumps({'visible': {'text': 'BETA'}})), 'PASS')
        self.assertEqual(H.accounting_tokens({'accounting': {'raw_tokens': 12}}), 12); self.assertEqual(H.accounting_tokens({'value': {'raw_tokens': 3}}), 3)

    def test_mcp_schema_tokens(self) -> None:
        cap = {'commands_by_name': {'read': {'name': 'read'}, 'find': {'name': 'find'}}, 'commands': []}
        with tempfile.NamedTemporaryFile('w', suffix='.json', delete=False) as tf:
            json.dump(cap, tf); path = tf.name
        try:
            expected = H.token_estimate(json.dumps({'read': cap['commands_by_name']['read'], 'find': cap['commands_by_name']['find']}, separators=(',', ':'), ensure_ascii=False).encode()); self.assertEqual(H.mcp_schema_tokens(path, 'read,find'), expected)
        finally:
            os.unlink(path)

    def test_synthetic_and_million(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw) / 'boot'; H.synthetic_tree(root, 2500); self.assertEqual(H.file_count(root), 2500); self.assertEqual((root / 'd000' / 'f0000.txt').read_text(), '000:0000\n')
            mroot = Path(raw) / 'mil'; H.million_line_repo(mroot, 1, 1, 520, 'NEEDLE_X'); lines = (mroot / 'mod_0000' / 'file_0000_000.rs').read_text().splitlines(); self.assertIn('NEEDLE_X', lines[499])

    def test_json_pickers(self) -> None:
        self.assertEqual(H.first_blob_ref({'refs': [{'kind': 'blob', 'ref': 'tz://blob/a'}]}), 'tz://blob/a')
        root, rel = H.glob_root_and_first({'visible': {'text': '# root: /tmp/r\nmod/a.rs\n'}})
        self.assertEqual((root, rel), ('/tmp/r', 'mod/a.rs'))

    def test_cli_surface(self) -> None:
        env = {**os.environ, 'PYTHONPATH': str(ROOT)}; out = subprocess.run([sys.executable, '-m', 'benchmarks.harness', 'tok', '--bytes', '5'], cwd=ROOT, capture_output=True, text=True, env=env, check=True); self.assertEqual(out.stdout.strip(), '2'); q = subprocess.run([sys.executable, '-m', 'benchmarks.harness', 'quality', 'read_file'], cwd=ROOT, capture_output=True, text=True, env=env, input=json.dumps({'visible': {'text': '[workspace]\n'}}), check=True)
        self.assertEqual(q.stdout.strip(), 'PASS')
if __name__ == '__main__':
    raise SystemExit(unittest.main(verbosity=2))
