#!/usr/bin/env python3
"""Fail closed when benchmark output exceeds its same-task raw baseline."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

SCHEMA_VERSION = "never-worse/v1"
SURFACE_ID = "captured-stdout-bytes/v1"
UNIT_ID = "estimator:bytes-ceil-div4/v1"
_TASK_RE = re.compile(r"[A-Za-z0-9_.:-]+")


class ReceiptError(ValueError):
    pass


@dataclass(frozen=True)
class Row:
    task: str
    candidate_bytes: int
    raw_bytes: int
    candidate_units: int
    raw_units: int


def _nonnegative(raw: str, field: str, line: int) -> int:
    try:
        value = int(raw)
    except ValueError as error:
        raise ReceiptError(f"line {line}: {field} must be an integer") from error
    if value < 0:
        raise ReceiptError(f"line {line}: {field} must be nonnegative")
    return value


def parse_receipt(path: Path) -> tuple[str, list[Row]]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise ReceiptError(f"cannot read receipt {path}: {error}") from error
    if len(lines) < 6:
        raise ReceiptError("receipt is missing metadata, header, or task rows")
    expected_metadata = [
        ("schema_version", SCHEMA_VERSION),
        ("suite", None),
        ("surface_id", SURFACE_ID),
        ("unit_id", UNIT_ID),
    ]
    metadata: dict[str, str] = {}
    for line_number, ((expected_key, expected_value), line) in enumerate(
        zip(expected_metadata, lines[:4], strict=True), start=1
    ):
        fields = line.split("\t")
        if len(fields) != 2 or fields[0] != expected_key or not fields[1]:
            raise ReceiptError(f"line {line_number}: expected {expected_key}<TAB>value")
        if expected_value is not None and fields[1] != expected_value:
            raise ReceiptError(
                f"line {line_number}: {expected_key} mismatch: {fields[1]!r} != {expected_value!r}"
            )
        metadata[fields[0]] = fields[1]
    expected_header = "task\tcandidate_bytes\traw_bytes\tcandidate_units\traw_units"
    if lines[4] != expected_header:
        raise ReceiptError(f"line 5: expected header {expected_header!r}")

    rows: list[Row] = []
    seen: set[str] = set()
    for line_number, line in enumerate(lines[5:], start=6):
        fields = line.split("\t")
        if len(fields) != 5:
            raise ReceiptError(
                f"line {line_number}: expected exactly 5 tab-separated fields"
            )
        task = fields[0]
        if _TASK_RE.fullmatch(task) is None:
            raise ReceiptError(f"line {line_number}: invalid task id {task!r}")
        if task in seen:
            raise ReceiptError(f"line {line_number}: duplicate task {task!r}")
        seen.add(task)
        candidate_bytes = _nonnegative(fields[1], "candidate_bytes", line_number)
        raw_bytes = _nonnegative(fields[2], "raw_bytes", line_number)
        candidate_units = _nonnegative(fields[3], "candidate_units", line_number)
        raw_units = _nonnegative(fields[4], "raw_units", line_number)
        expected_candidate = (candidate_bytes + 3) // 4
        expected_raw = (raw_bytes + 3) // 4
        if candidate_units != expected_candidate or raw_units != expected_raw:
            raise ReceiptError(
                f"line {line_number}: {UNIT_ID} count mismatch for exact captured bytes"
            )
        rows.append(Row(task, candidate_bytes, raw_bytes, candidate_units, raw_units))
    if not rows:
        raise ReceiptError("receipt has no task rows")
    return metadata["suite"], rows


def render(suite: str, rows: list[Row]) -> tuple[str, bool]:
    passed = all(row.candidate_units <= row.raw_units for row in rows)
    output = [
        "## Never-worse estimated-token budget assertion",
        "",
        f"Suite: `{suite}`. Surface: `{SURFACE_ID}`. Unit: `{UNIT_ID}`. This is a heuristic estimate, not Q99.",
        "",
        "| task | TokenZero bytes | raw-cli bytes | TokenZero est_tokens | raw-cli est_tokens | delta | result |",
        "|---|---:|---:|---:|---:|---:|---|",
    ]
    for row in rows:
        delta = row.raw_units - row.candidate_units
        result = "PASS" if delta >= 0 else "FAIL"
        output.append(
            f"| `{row.task}` | {row.candidate_bytes} | {row.raw_bytes} | "
            f"{row.candidate_units} | {row.raw_units} | {delta} | **{result}** |"
        )
    verdict = "PASS" if passed else "FAIL"
    output.extend(
        [
            "",
            f"> **Result: {verdict}** -- every TokenZero row must be <= its same-task raw-cli baseline in `{UNIT_ID}` units.",
        ]
    )
    return "\n".join(output), passed


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("receipt", type=Path)
    args = parser.parse_args()
    try:
        suite, rows = parse_receipt(args.receipt)
    except ReceiptError as error:
        print(f"never-worse gate: invalid receipt: {error}", file=sys.stderr)
        return 2
    rendered, passed = render(suite, rows)
    print(rendered)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
