#!/usr/bin/env python3
"""Shard tokenzero-core/src/tests.rs into tests/ by #[test] boundaries + name routing."""
from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/tokenzero-core/src"
SRC = ROOT / "tests.rs"
lines = SRC.read_text().splitlines(keepends=True)
HEADER = lines[:3]

TEST_ATTR = re.compile(r"^\s*#\[test\]")
FN_LINE = re.compile(r"^(?:pub )?fn (\w+)")


def fn_end(fn_line: int) -> int:
    depth = 0
    for k in range(fn_line, len(lines)):
        depth += lines[k].count("{") - lines[k].count("}")
        if depth == 0 and k > fn_line:
            return k + 1
    return len(lines)


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


def balanced(start: int, end: int) -> bool:
    chunk = "".join(lines[start:end])
    return chunk.count("{") == chunk.count("}")


def find_test_end(fn_start: int, fn_i: int, seg_end: int) -> int:
    end = fn_i + 1
    while end < seg_end and not balanced(fn_start, end):
        end += 1
    return end


def find_helper_end(fn_start: int, fn_i: int, bound: int) -> int:
    end = fn_i + 1
    while end < bound and not balanced(fn_start, end):
        end += 1
    return end


def test_blocks() -> tuple[list[tuple[str, int, int]], list[tuple[int, int]]]:
    attr_lines = [i for i, l in enumerate(lines) if TEST_ATTR.match(l)]
    tests: list[tuple[str, int, int]] = []
    helpers: list[tuple[int, int]] = []

    for idx, attr in enumerate(attr_lines):
        seg_end = attr_lines[idx + 1] if idx + 1 < len(attr_lines) else len(lines)
        seg_start = block_start(attr)
        fns: list[tuple[str, int, int]] = []
        i = seg_start
        while i < seg_end:
            m = FN_LINE.match(lines[i])
            if m:
                fn_start = block_start(i)
                end = find_test_end(fn_start, i, seg_end)
                fns.append((m.group(1), fn_start, end))
                i = end
            else:
                i += 1
        if not fns:
            continue
        tests.append((fns[0][0], fns[0][1], fns[0][2]))
        for _, hs, he in fns[1:]:
            helpers.append((hs, he))
    return tests, helpers


def gap_blocks(test_spans: list[tuple[int, int]]) -> list[tuple[int, int]]:
    gaps: list[tuple[int, int]] = []
    header_end = len(HEADER)
    if test_spans and test_spans[0][0] > header_end:
        gaps.append((header_end, test_spans[0][0]))
    for (_, e1), (s2, _) in zip(test_spans, test_spans[1:]):
        if e1 < s2:
            gaps.append((e1, s2))
    if test_spans and test_spans[-1][1] < len(lines):
        gaps.append((test_spans[-1][1], len(lines)))
    return gaps


def gap_helpers(test_spans: list[tuple[int, int]]) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    for gap_s, gap_e in gap_blocks(test_spans):
        i = gap_s
        while i < gap_e:
            m = FN_LINE.match(lines[i])
            if m:
                hs = block_start(i)
                he = find_helper_end(hs, i, gap_e)
                out.append((hs, he))
                i = he
            else:
                i += 1
    return out


def module_for(name: str) -> str:
    rules: list[tuple[str, list[str]]] = [
        ("capsule", ["token_count", "capsule_", "auto_capsule", "budget_truncation"]),
        (
            "shell",
            [
                "shell_",
                "rg_",
                "windows_shell",
                "path_qualified_rg",
                "real_shell_",
                "short_",
                "noisy_shell",
                "explicit_or_failing_shell",
                "tiny_success_shell",
                "long_success",
                "wide_success",
                "failing_cargo_test",
            ],
        ),
        ("repeat_render", ["repeat_render_"]),
        (
            "toolchain",
            [
                "cargo_",
                "pytest_",
                "passing_tests",
                "git_clone",
                "npm_",
            ],
        ),
        (
            "render_util",
            [
                "summarize_tokens",
                "structural_dedupe",
                "classifier_",
                "diff_structured",
                "repo_inventory",
            ],
        ),
    ]
    for mod, prefixes in rules:
        if any(name.startswith(p) or p in name for p in prefixes):
            return mod
    return "misc"


blocks, inline_helpers = test_blocks()
test_spans = [(s, e) for _, s, e in blocks]
seen_helper: set[int] = set()
helpers: list[tuple[int, int]] = []
for hs, he in inline_helpers + gap_helpers(test_spans):
    if hs in seen_helper:
        continue
    seen_helper.add(hs)
    helpers.append((hs, he))

PREAMBLE = """use super::*;
use proptest::prelude::*;

use super::support::*;

"""

SUPPORT = "use super::*;\nuse proptest::prelude::*;\n\n"

tests_dir = ROOT / "tests"
tests_dir.mkdir(exist_ok=True)

support_body = SUPPORT
proptest_body = ""
for s, e in helpers:
    chunk = "".join(lines[s:e])
    if "proptest!" in chunk:
        proptest_body += chunk
        continue
    chunk = re.sub(r"^fn ", "pub(crate) fn ", chunk, flags=re.M)
    chunk = re.sub(r"^pub fn ", "pub(crate) fn ", chunk, flags=re.M)
    support_body += chunk
(tests_dir / "support.rs").write_text(support_body)
if proptest_body:
    (tests_dir / "proptest.rs").write_text(
        "use super::*;\nuse proptest::prelude::*;\n\nuse super::support::*;\n\n"
        + proptest_body
    )

by_mod: dict[str, list[tuple[str, int, int]]] = defaultdict(list)
for name, s, e in blocks:
    by_mod[module_for(name)].append((name, s, e))

mod_names = sorted(by_mod.keys())
if proptest_body:
    mod_names.append("proptest")
    mod_names = sorted(set(mod_names))

for mod in mod_names:
    if mod == "proptest":
        continue
    body = PREAMBLE + "".join("".join(lines[s:e]) for _, s, e in by_mod[mod])
    (tests_dir / f"{mod}.rs").write_text(body)
    print(f"wrote tests/{mod}.rs: {len(by_mod[mod])} tests")

mod_rs = (
    "".join(HEADER)
    + "\nmod support;\n"
    + "".join(f"mod {m};\n" for m in mod_names)
)
(tests_dir / "mod.rs").write_text(mod_rs)
SRC.unlink()
print(f"done: {len(blocks)} tests, {len(helpers)} helpers, modules: {mod_names}")
