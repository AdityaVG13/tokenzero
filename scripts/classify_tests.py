#!/usr/bin/env python3
"""Inventory every source-level Rust test attribute and record its ship role."""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEST_ATTR = re.compile(r"^\s*#\[(?:tokio::)?test(?:\([^]]*\))?\]\s*$")
TEST_FN = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_$]+)")


def source_files() -> list[Path]:
    return sorted(ROOT.glob("crates/**/*.rs")) + sorted(ROOT.glob("tests/*.rs"))


def discover_tests() -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    seen: defaultdict[tuple[str, str], int] = defaultdict(int)
    for path in source_files():
        relative = path.relative_to(ROOT).as_posix()
        lines = path.read_text().splitlines()
        for index, line in enumerate(lines):
            if not TEST_ATTR.match(line):
                continue
            name = f"generated_at_{index + 1}"
            for candidate in lines[index + 1 : index + 16]:
                match = TEST_FN.match(candidate)
                if match:
                    name = match.group(1)
                    break
            key = (relative, name)
            seen[key] += 1
            suffix = f"@{seen[key]}" if seen[key] > 1 else ""
            rows.append(
                {
                    "id": f"{relative}::{name}{suffix}",
                    "path": relative,
                    "line": index + 1,
                    "name": name,
                }
            )
    return rows


def ship_case_for(text: str) -> str | None:
    mappings = [
        (("read", "expand", "recovery", "zeroref"), "ship_read_expand_round_trips_exact_bytes"),
        (("find", "search", "grep", "glob"), "ship_find_returns_bounded_visible_evidence_and_refs"),
        (("hook", "passthrough"), "ship_hook_rewrites_actionable_input_and_fails_open_otherwise"),
        (("install", "package", "sbom", "uninstall"), "ship_installer_dry_run_is_canonical_and_non_mutating"),
        (("doctor", "store_root", "isolation"), "ship_doctor_reports_isolated_store_resolution"),
        (("run", "shell", "process", "exit"), "ship_run_preserves_child_stdout_and_failure_status"),
        (("path", "root", "confin", "leak"), "ship_absolute_path_rejection_does_not_leak_bytes"),
        (("capabil", "help", "robot", "agent"), "ship_help_and_capabilities_are_machine_usable"),
    ]
    for needles, case in mappings:
        if any(needle in text for needle in needles):
            return case
    return None


def classify(row: dict[str, object]) -> dict[str, object]:
    path = str(row["path"])
    name = str(row["name"])
    text = f"{path} {name}".lower()
    result = dict(row)
    if path.startswith("tests/"):
        result.update(
            classification="SHIP",
            ship_case=name,
            rationale="Top-level observable release proof paired with a killed product-source mutant receipt.",
        )
    elif any(
        marker in text
        for marker in (
            "zeroref",
            "raw_worker_v2",
            "operation_abi",
            "conformance",
            "zero_result",
        )
    ):
        result.update(
            classification="SHARED",
            ship_case=ship_case_for(text),
            rationale="Engine adapter coverage backed by ZeroStack contract, codec, or shared-testkit authority.",
        )
    elif any(
        marker in text
        for marker in ("retirement", "migration", "legacy", "static_evidence", "extraction")
    ):
        result.update(
            classification="SCAFFOLDING",
            ship_case=ship_case_for(text),
            rationale="Retained development guard for a completed migration or extraction boundary.",
        )
    else:
        case = ship_case_for(text)
        result.update(
            classification="DEV-ONLY",
            ship_case=case,
            rationale=(
                "Development coverage; its user-visible guarantee is represented by the named ship case."
                if case
                else "Development-only internal invariant with no separate public release claim."
            ),
        )
    return result


def main() -> int:
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "docs/test-classification-v1.jsonl"
    rows = [classify(row) for row in discover_tests()]
    target.write_text("".join(f"{json.dumps(row, sort_keys=True)}\n" for row in rows))
    counts: defaultdict[str, int] = defaultdict(int)
    for row in rows:
        counts[str(row["classification"])] += 1
    print(json.dumps({"tests": len(rows), "classifications": counts}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
