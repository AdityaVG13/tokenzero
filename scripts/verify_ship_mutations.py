#!/usr/bin/env python3
"""Kill each registered product-source mutant and record reproducible receipts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = ROOT / "tests/ship-mutations.json"
RECEIPTS = ROOT / "tests/ship-mutation-receipts.json"
RUNNER = ROOT / "scripts/run_ship_suite.py"


def command_text(command: list[str]) -> str:
    normalized = ["python3" if value == sys.executable else value for value in command]
    return " ".join(value.replace(os.fspath(ROOT), ".") for value in normalized)


def execute(command: list[str], *, check: bool) -> subprocess.CompletedProcess[bytes]:
    print("+", command_text(command), flush=True)
    result = subprocess.run(command, cwd=ROOT, env=os.environ.copy(), capture_output=True)
    if result.returncode != 0:
        excerpt = (result.stdout + result.stderr).decode(errors="replace")[-4_000:]
        print(excerpt, file=sys.stderr)
    if check and result.returncode != 0:
        raise RuntimeError(f"command failed ({result.returncode}): {command_text(command)}")
    return result


def build_command(case: dict[str, Any]) -> list[str] | None:
    package = case.get("build_package")
    binary = case.get("build_binary")
    if package is None:
        return None
    return [
        "cargo",
        "build",
        "--locked",
        "-p",
        package,
        "--bin",
        binary,
        "--no-default-features",
    ]


def test_command(case: dict[str, Any]) -> list[str]:
    return [
        sys.executable,
        os.fspath(RUNNER),
        "--target",
        case["test_target"],
        "--exact",
        case["test"],
        "--skip-build",
    ]


def verify_registry(cases: list[dict[str, Any]]) -> None:
    seen_ids: set[str] = set()
    seen_tests: set[str] = set()
    for case in cases:
        mutation_id = case["mutation_id"]
        test = case["test"]
        if mutation_id in seen_ids or test in seen_tests:
            raise RuntimeError(f"duplicate mutation ID or test: {mutation_id} / {test}")
        seen_ids.add(mutation_id)
        seen_tests.add(test)
        source = (ROOT / case["target"]).read_text()
        preimage = case["from"]
        digest = hashlib.sha256(preimage.encode()).hexdigest()
        if source.count(preimage) != 1:
            raise RuntimeError(f"preimage is not unique for {mutation_id}")
        if digest != case["preimage_sha256"]:
            raise RuntimeError(f"preimage digest mismatch for {mutation_id}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mutation", help="verify one mutation without replacing receipts")
    args = parser.parse_args()
    registry_bytes = REGISTRY.read_bytes()
    registry = json.loads(registry_bytes)
    cases = registry["cases"]
    verify_registry(cases)
    selected_cases = [case for case in cases if args.mutation in (None, case["mutation_id"])]
    if not selected_cases:
        parser.error(f"unknown mutation: {args.mutation}")

    baseline_command = [sys.executable, os.fspath(RUNNER)]
    execute(baseline_command, check=True)
    receipts: list[dict[str, Any]] = []

    try:
        for case in selected_cases:
            target = ROOT / case["target"]
            original = target.read_text()
            mutated = original.replace(case["from"], case["to"], 1)
            target.write_text(mutated)
            try:
                build = build_command(case)
                if build is not None:
                    execute(build, check=True)
                test = test_command(case)
                result = execute(test, check=False)
                if result.returncode == 0:
                    print((result.stdout + result.stderr).decode(errors="replace"), file=sys.stderr)
                    raise RuntimeError(f"mutant survived: {case['mutation_id']}")
                receipts.append(
                    {
                        "mutation_id": case["mutation_id"],
                        "test": case["test"],
                        "baseline_passed": True,
                        "build_command": command_text(build) if build is not None else None,
                        "test_command": command_text(test),
                        "mutant_exit_code": result.returncode,
                    }
                )
                print(f"KILLED: {case['mutation_id']}", flush=True)
            finally:
                target.write_text(original)
    finally:
        # Leave shared target artifacts aligned with restored source, even after a failure.
        execute([sys.executable, os.fspath(RUNNER)], check=True)

    if args.mutation is not None:
        print(f"OK: killed selected product-source mutant {args.mutation}")
        return 0

    document = {
        "schema_version": "tokenzero.ship-mutation-receipts.v1",
        "registry_sha256": hashlib.sha256(registry_bytes).hexdigest(),
        "baseline_command": command_text(baseline_command),
        "cases": receipts,
    }
    RECEIPTS.write_text(json.dumps(document, indent=2) + "\n")
    print(f"OK: killed {len(receipts)} product-source mutants; receipts={RECEIPTS.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
