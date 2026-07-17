#!/usr/bin/env python3
"""Retained compatibility entry point for the completed source split."""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE = ROOT / "crates/tokenzero-mcp/src/tests"
MONOLITH = ROOT / "crates/tokenzero-mcp/src/tests.rs"


def main() -> int:
    if not MODULE.is_dir():
        print(
            f"incomplete split: missing module directory {MODULE.relative_to(ROOT)}",
            file=sys.stderr,
        )
        return 1
    if MONOLITH.is_file():
        print(
            f"incomplete split: monolithic file still present at {MONOLITH.relative_to(ROOT)}",
            file=sys.stderr,
        )
        return 1
    if not (MODULE / "mod.rs").is_file():
        print(
            f"incomplete split: missing {MODULE.relative_to(ROOT)}/mod.rs",
            file=sys.stderr,
        )
        return 1
    print(f"ok: completed split module {MODULE.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
