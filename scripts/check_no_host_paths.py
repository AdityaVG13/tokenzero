#!/usr/bin/env python3
"""Fail when a tracked file records an absolute host path.

Usage:
    python3 scripts/check_no_host_paths.py [path ...]
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
MAX_BYTES = 8 << 20
PATTERNS = (
    re.compile(r'/Users/[^/\s"\'<>*`,;:)\]}]+/'),
    re.compile(r'/home/[^/\s"\'<>*`,;:)\]}]+/'),
    re.compile(r'[A-Za-z]:[\\/]+Users[\\/]+[^\\/\s"\'<>*`,;:)\]}]+[\\/]'),
)
ALLOWLIST: dict[str, tuple[tuple[str, ...], str]] = {
    "crates/tokenzero-install/src/package_audit/tests/tar.rs": (
        ("C:/Users/example/", "/Users/example/", "/home/example/"),
        "synthetic path-traversal attack fixtures for the package auditor",
    ),
    "crates/tokenzero-core/src/tests/misc.rs": (
        (r"C:\\Users\\Ada",),
        "synthetic Windows path fixture",
    ),
    "crates/tokenzero-recovery/src/embedded_store_tests.rs": (
        (r"C:\Users\x",),
        "synthetic Windows path fixture",
    ),
    "docs/routing.md": (
        ("/home/.tokenzero/",),
        "documented placeholder home directory in a PATH example",
    ),
    "docs/windows-systemwide.md": (
        (r"C:\Users\you",),
        "documented placeholder user directory in Windows examples",
    ),
    "crates/tokenzero-recovery/benches/perf_hotspots/baseline-shell.sample.txt": (
        ("/Users/USER/", "/Users/*/"),
        "captured sample output whose user component is already masked",
    ),
    "docs/install.md": (("/Users/you/",), "documented placeholder in an install example"),
    "docs/mcp.md": (("/Users/.../",), "documented masked path pattern"),
    ".beads/issues.jsonl": ((), "issue-tracker export owned by the br tracker; tracked separately"),
}


def tracked_files() -> list[str]:
    listing = subprocess.run(
        ["git", "ls-files", "-z"], cwd=REPO, capture_output=True, check=True
    ).stdout
    return [name for name in listing.decode().split("\0") if name]


def readable_text(path: Path) -> str | None:
    try:
        if path.stat().st_size > MAX_BYTES:
            return None
        raw = path.read_bytes()
    except OSError:
        return None
    if b"\0" in raw:
        return None
    try:
        return raw.decode()
    except UnicodeDecodeError:
        return None


def findings(relative: str, text: str) -> list[tuple[int, str]]:
    allowed, _ = ALLOWLIST.get(relative, ((), ""))
    hits: list[tuple[int, str]] = []
    for number, line in enumerate(text.splitlines(), start=1):
        for pattern in PATTERNS:
            for match in pattern.finditer(line):
                found = match.group(0)
                if relative in ALLOWLIST and not allowed:
                    continue
                if any(found.startswith(prefix) for prefix in allowed):
                    continue
                hits.append((number, found))
    return hits


def main(argv: list[str]) -> int:
    names = argv or tracked_files()
    reported = 0
    for relative in names:
        path = REPO / relative
        if not path.is_file():
            continue
        text = readable_text(path)
        if text is None:
            continue
        for number, found in findings(relative, text):
            print(f"{relative}:{number}: host path {found!r}")
            reported += 1
    if reported:
        skipped = ", ".join(sorted(ALLOWLIST))
        print(
            f"\n{reported} host path(s) found in tracked files.\n"
            "Emit repository-relative paths, '<tmp>' for temporary directories, and '<home>' for\n"
            "paths outside the repository. benchmarks/bench_common.py provides portable_path,\n"
            "portable_argv, portable_command, portable_text, and portable_tree for harnesses.\n"
            f"Allowlisted files (with documented reasons): {skipped}",
            file=sys.stderr,
        )
        return 1
    print(f"no host paths in {len(names)} tracked file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
