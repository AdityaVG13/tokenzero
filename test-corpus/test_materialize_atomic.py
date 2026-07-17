#!/usr/bin/env python3
"""Narrow regression for P16-002: late gzip failure must not refresh earlier outputs."""

from __future__ import annotations

import gzip
import hashlib
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATERIALIZE = ROOT / "test-corpus" / "materialize.py"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        (t / "materialize.py").write_text(MATERIALIZE.read_text())
        (t / "aa_first.rs.gz").write_bytes(gzip.compress(b"first-v1\n", mtime=0))
        (t / "bb_second.rs.gz").write_bytes(gzip.compress(b"second-v1\n", mtime=0))
        first = subprocess.run(
            [sys.executable, "materialize.py"],
            cwd=t,
            capture_output=True,
            text=True,
        )
        if first.returncode != 0:
            print(first.stderr, file=sys.stderr)
            return 1
        sentinels = {
            "aa_first.rs": sha256(t / "expanded" / "aa_first.rs"),
            "bb_second.rs": sha256(t / "expanded" / "bb_second.rs"),
        }
        # Refresh earlier members, then append a sorted-last truncated member.
        (t / "aa_first.rs.gz").write_bytes(gzip.compress(b"first-v2\n", mtime=0))
        (t / "bb_second.rs.gz").write_bytes(gzip.compress(b"second-v2\n", mtime=0))
        (t / "zz_truncated.rs.gz").write_bytes(
            gzip.compress(b"truncated", mtime=0)[:-4]
        )
        fail = subprocess.run(
            [sys.executable, "materialize.py"],
            cwd=t,
            capture_output=True,
            text=True,
        )
        if fail.returncode == 0:
            print("expected non-zero exit for truncated trailing member", file=sys.stderr)
            return 1
        after = {
            "aa_first.rs": sha256(t / "expanded" / "aa_first.rs"),
            "bb_second.rs": sha256(t / "expanded" / "bb_second.rs"),
        }
        if after != sentinels:
            print(
                f"partial refresh committed: before={sentinels} after={after}",
                file=sys.stderr,
            )
            return 1
        if (t / "expanded" / "zz_truncated.rs").exists():
            print("truncated output must not be published", file=sys.stderr)
            return 1
        # Staging leftovers must not remain after failure.
        leftovers = list(t.glob(".expanded.staging.*")) + list(t.glob(".expanded.bak.*"))
        if leftovers:
            print(f"staging leftovers present: {leftovers}", file=sys.stderr)
            return 1
    print("p16_corpus_atomic_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
