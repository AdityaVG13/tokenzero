#!/usr/bin/env python3
"""Fail closed when the bounded ship suite or full classification drifts."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

from classify_tests import ROOT, classify, discover_tests

MAX_TESTS = 50
MAX_LINES = 2_500
ALLOWED = {"DUPLICATE", "SCAFFOLDING", "SHARED", "DEV-ONLY", "SHIP"}


def fail(message: str) -> int:
    print(f"FAIL: {message}")
    return 1


def main() -> int:
    ship_files = sorted((ROOT / "tests").glob("*.rs"))
    ship_rows = [row for row in discover_tests() if str(row["path"]).startswith("tests/")]
    ship_lines = sum(len(path.read_text().splitlines()) for path in ship_files)
    if len(ship_rows) > MAX_TESTS:
        return fail(f"ship suite has {len(ship_rows)} tests; max is {MAX_TESTS}")
    if ship_lines > MAX_LINES:
        return fail(f"ship suite has {ship_lines} Rust lines; max is {MAX_LINES}")

    mutation_path = ROOT / "tests/ship-mutations.json"
    mutation_data = json.loads(mutation_path.read_text())
    cases = mutation_data["cases"]
    registered = {row["test"] for row in cases}
    ship_names = {str(row["name"]) for row in ship_rows}
    if registered != ship_names:
        return fail(f"mutation registry drift: tests={sorted(ship_names)} registry={sorted(registered)}")
    if len(cases) != len(registered):
        return fail("mutation registry must contain exactly one case per ship test")
    mutation_ids = {row["mutation_id"] for row in cases}
    if len(mutation_ids) != len(cases):
        return fail("mutation IDs must be unique")
    for case in cases:
        target = ROOT / case["target"]
        if not target.is_file():
            return fail(f"mutation target does not exist: {case['target']}")
        preimage = case["from"]
        if target.read_text().count(preimage) != 1:
            return fail(f"mutation preimage is not unique: {case['mutation_id']}")
        digest = hashlib.sha256(preimage.encode()).hexdigest()
        if case.get("preimage_sha256") != digest:
            return fail(f"mutation preimage digest drift: {case['mutation_id']}")

    receipt_path = ROOT / "tests/ship-mutation-receipts.json"
    receipts = json.loads(receipt_path.read_text())
    registry_digest = hashlib.sha256(mutation_path.read_bytes()).hexdigest()
    if receipts.get("registry_sha256") != registry_digest:
        return fail("mutation receipts do not match the current registry")
    receipt_cases = receipts.get("cases", [])
    if {row.get("mutation_id") for row in receipt_cases} != mutation_ids:
        return fail("mutation receipt case set does not match the registry")
    if any(row.get("mutant_exit_code") == 0 for row in receipt_cases):
        return fail("a registered product-source mutant survived")
    if any(not row.get("baseline_passed") for row in receipt_cases):
        return fail("mutation verification lacks a passing baseline")

    classification_path = ROOT / "docs/test-classification-v1.jsonl"
    recorded = [json.loads(line) for line in classification_path.read_text().splitlines() if line]
    current = [classify(row) for row in discover_tests()]
    recorded_by_id = {row["id"]: row for row in recorded}
    current_by_id = {row["id"]: row for row in current}
    if recorded_by_id.keys() != current_by_id.keys():
        missing = sorted(current_by_id.keys() - recorded_by_id.keys())
        stale = sorted(recorded_by_id.keys() - current_by_id.keys())
        return fail(f"classification drift: missing={missing} stale={stale}")
    if any(row.get("classification") not in ALLOWED for row in recorded):
        return fail("classification contains an unknown category")
    if recorded_by_id != current_by_id:
        return fail("classification content drifted; regenerate and review the report")

    print(
        f"OK: ship suite {len(ship_rows)}/{MAX_TESTS} tests, "
        f"{ship_lines}/{MAX_LINES} Rust lines; {len(current)} source tests classified; "
        f"{len(receipt_cases)} product-source mutants killed"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
