#!/usr/bin/env python3
"""Shard cli_release_claim_audits.rs into tests/cli_release_claim_audits/ thematic modules."""
from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/tokenzero/tests"
SRC = ROOT / "cli_release_claim_audits.rs"
lines = SRC.read_text().splitlines(keepends=True)

HEADER_END = 0
for i, line in enumerate(lines):
    if line.strip() == "#[test]":
        HEADER_END = i
        break
HEADER = lines[:HEADER_END]

TEST_ATTR = re.compile(r"^\s*#\[test\]")
FN_LINE = re.compile(r"^fn (\w+)")


def balanced(start: int, end: int) -> bool:
    chunk = "".join(lines[start:end])
    return chunk.count("{") == chunk.count("}")


def block_start(i: int) -> int:
    start = i
    j = i - 1
    while j >= 0:
        s = lines[j].strip()
        if s == "" or s.startswith("#["):
            start = j
            j -= 1
        else:
            break
    return start


def find_test_end(fn_start: int, fn_i: int, seg_end: int) -> int:
    end = fn_i + 1
    while end < seg_end and not balanced(fn_start, end):
        end += 1
    return end


def test_blocks() -> list[tuple[str, int, int]]:
    attr_lines = [i for i, l in enumerate(lines) if TEST_ATTR.match(l)]
    tests: list[tuple[str, int, int]] = []
    for idx, attr in enumerate(attr_lines):
        seg_end = attr_lines[idx + 1] if idx + 1 < len(attr_lines) else len(lines)
        j = attr + 1
        while j < seg_end and not lines[j].startswith("fn "):
            j += 1
        if j >= seg_end:
            continue
        fn_start = block_start(j)
        end = find_test_end(fn_start, j, seg_end)
        m = FN_LINE.match(lines[j])
        name = m.group(1) if m else f"test_{attr}"
        tests.append((name, fn_start, end))
    return tests


def module_for(name: str) -> str:
    rules: list[tuple[str, list[str]]] = [
        ("eval", ["one_shot", "release_claim", "claim_audit_passes"]),
        ("benchmark", ["benchmark"]),
        ("artifact", ["source_artifact", "artifact_handoff", "currency"]),
        ("adapter", ["adapter"]),
        ("gates", ["gate", "doctor", "reach"]),
        ("misc", []),
    ]
    for mod, prefixes in rules:
        if mod == "misc":
            continue
        if any(p in name for p in prefixes):
            return mod
    return "misc"


blocks = test_blocks()
out_dir = ROOT / "cli_release_claim_audits"
out_dir.mkdir(exist_ok=True)

by_mod: dict[str, list[tuple[str, int, int]]] = defaultdict(list)
for name, s, e in blocks:
    by_mod[module_for(name)].append((name, s, e))

PREAMBLE = """use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[path = "../common/mod.rs"]
mod common;
use common::*;

"""

mod_names = sorted(by_mod.keys())
for mod in mod_names:
    body = PREAMBLE + "".join("".join(lines[s:e]) for _, s, e in by_mod[mod])
    (out_dir / f"{mod}.rs").write_text(body)
    print(f"wrote cli_release_claim_audits/{mod}.rs: {len(by_mod[mod])} tests")

main_rs = (
    "".join(HEADER).replace(
        "mod common;\nuse common::*;\n\n",
        "",
    )
    + "\n"
    + "".join(f"mod {m};\n" for m in mod_names)
)
(out_dir / "main.rs").write_text(main_rs)
SRC.unlink()
print(f"done: {len(blocks)} tests, modules: {mod_names}")
