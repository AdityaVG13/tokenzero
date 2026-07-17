#!/usr/bin/env python3
"""Narrow regression for P16-001: gzip bombs must not materialize."""

from __future__ import annotations

import gzip
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATERIALIZE = ROOT / "test-corpus" / "materialize.py"


def main() -> int:
    bomb = b"0" * (16 * 1024 * 1024)
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        (t / "materialize.py").write_text(MATERIALIZE.read_text())
        # Tiny legitimate member must still expand.
        (t / "ok.rs.gz").write_bytes(gzip.compress(b"fn main() {}\n", mtime=0))
        (t / "bomb.rs.gz").write_bytes(gzip.compress(bomb, 9, mtime=0))
        proc = subprocess.run(
            [sys.executable, "materialize.py"],
            cwd=t,
            capture_output=True,
            text=True,
        )
        if proc.returncode == 0:
            print("expected non-zero exit for gzip bomb", file=sys.stderr)
            return 1
        if (t / "expanded" / "bomb.rs").exists():
            print("bomb output must not be committed", file=sys.stderr)
            return 1
        # Re-run with only the legitimate member.
        (t / "bomb.rs.gz").unlink()
        ok = subprocess.run(
            [sys.executable, "materialize.py"],
            cwd=t,
            capture_output=True,
            text=True,
        )
        if ok.returncode != 0:
            print(ok.stderr, file=sys.stderr)
            return 1
        expanded = t / "expanded" / "ok.rs"
        if expanded.read_bytes() != b"fn main() {}\n":
            print("legitimate member failed to expand exactly", file=sys.stderr)
            return 1
    print("p16_corpus_bound_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
