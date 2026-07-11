#!/usr/bin/env python3
"""Aggregate tokenzero.ledger.v1 JSONL into per-version session cost curves."""
from __future__ import annotations

import argparse
import hashlib
import json
import statistics
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable

REPO = Path(__file__).resolve().parents[3]
DEFAULT_OUTPUT = Path(__file__).with_suffix("").with_name("ledger_curve") / "starter.json"
LEDGER_SCHEMA = "tokenzero.ledger.v1"
CURVE_SCHEMA = "tokenzero.ledger-curve.v1"


def candidate_paths(values: list[str]) -> list[Path]:
    if values:
        seeds = [Path(value).expanduser() for value in values]
    else:
        seeds = [
            REPO / ".zerostack" / "tokenzero" / "ledger.jsonl",
            REPO / ".zerostack" / "ledger.jsonl",
            REPO / "ledger.jsonl",
        ]
    found: list[Path] = []
    for seed in seeds:
        if seed.is_dir():
            found.extend(sorted(seed.rglob("ledger.jsonl")))
        elif seed.name.endswith("recovery-cache.json"):
            found.append(seed.with_name("ledger.jsonl"))
        else:
            found.append(seed)
    expanded: list[Path] = []
    for path in found:
        rotated = path.with_suffix(path.suffix + ".1")
        if rotated.is_file():
            expanded.append(rotated)
        if path.is_file():
            expanded.append(path)
    return list(dict.fromkeys(path.resolve() for path in expanded))


def version_key(version: str) -> tuple:
    pieces = []
    for piece in version.replace("-", ".").split("."):
        pieces.append((0, int(piece)) if piece.isdigit() else (1, piece))
    return tuple(pieces)


def percentile(values: list[int], q: float) -> float:
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    low = int(index)
    high = min(low + 1, len(ordered) - 1)
    return ordered[low] + (ordered[high] - ordered[low]) * (index - low)


def iso_timestamp(timestamp_ms: int) -> str:
    return datetime.fromtimestamp(timestamp_ms / 1000, tz=timezone.utc).isoformat().replace("+00:00", "Z")


def aggregate(paths: Iterable[Path]) -> dict:
    source_rows = []
    records = []
    malformed = 0
    ignored_schema = 0
    for path in paths:
        payload = path.read_bytes()
        valid_here = 0
        for raw_line in payload.splitlines():
            if not raw_line.strip():
                continue
            try:
                record = json.loads(raw_line)
            except (json.JSONDecodeError, UnicodeDecodeError):
                malformed += 1
                continue
            if record.get("schema") != LEDGER_SCHEMA:
                ignored_schema += 1
                continue
            try:
                record["version"]["crate"]
                record["session_id"]
                record["repo"]
                record["token_mass"]["visible_tokens"]
            except (KeyError, TypeError):
                malformed += 1
                continue
            records.append(record)
            valid_here += 1
        source_rows.append({
            "path": str(path),
            "sha256": hashlib.sha256(payload).hexdigest(),
            "bytes": len(payload),
            "valid_records": valid_here,
        })

    sessions: dict[tuple[str, str, str], dict] = {}
    for record in records:
        version = str(record["version"]["crate"])
        key = (version, str(record["repo"]), str(record["session_id"]))
        row = sessions.setdefault(key, {
            "version": version,
            "repo": str(record["repo"]),
            "session_id": str(record["session_id"]),
            "turns": 0,
            "visible_cost_tokens": 0,
            "raw_tokens": 0,
            "prevented_tokens": 0,
            "saved_bytes": 0,
            "first_timestamp_ms": int(record["timestamp_ms"]),
            "last_timestamp_ms": int(record["timestamp_ms"]),
        })
        mass = record["token_mass"]
        row["turns"] += 1
        for field in ("visible_tokens", "raw_tokens", "prevented_tokens", "saved_bytes"):
            destination = "visible_cost_tokens" if field == "visible_tokens" else field
            row[destination] += int(mass.get(field, 0))
        timestamp = int(record["timestamp_ms"])
        row["first_timestamp_ms"] = min(row["first_timestamp_ms"], timestamp)
        row["last_timestamp_ms"] = max(row["last_timestamp_ms"], timestamp)

    by_version: dict[str, list[dict]] = defaultdict(list)
    for row in sessions.values():
        by_version[row["version"]].append(row)
    curve = []
    for version in sorted(by_version, key=version_key):
        rows = by_version[version]
        costs = [row["visible_cost_tokens"] for row in rows]
        curve.append({
            "version": version,
            "sessions_n": len(rows),
            "turns_n": sum(row["turns"] for row in rows),
            "visible_cost_tokens": {
                "total": sum(costs),
                "median_per_session": percentile(costs, 0.5),
                "mean_per_session": statistics.fmean(costs),
                "min_per_session": min(costs),
                "max_per_session": max(costs),
            },
            "raw_tokens_total": sum(row["raw_tokens"] for row in rows),
            "prevented_tokens_total": sum(row["prevented_tokens"] for row in rows),
            "saved_bytes_total": sum(row["saved_bytes"] for row in rows),
            "first_timestamp_ms": min(row["first_timestamp_ms"] for row in rows),
            "last_timestamp_ms": max(row["last_timestamp_ms"] for row in rows),
        })

    newest = max((int(row["timestamp_ms"]) for row in records), default=0)
    losses = []
    if len(curve) < 2:
        losses.append(f"Only {len(curve)} version is represented; no cross-release trend can be inferred.")
    if len(sessions) < 5:
        losses.append(f"Only {len(sessions)} session is represented; distribution statistics are thin.")
    if malformed:
        losses.append(f"Ignored {malformed} malformed JSONL record(s).")
    if ignored_schema:
        losses.append(f"Ignored {ignored_schema} record(s) with a non-{LEDGER_SCHEMA} schema.")
    if not source_rows:
        losses.append("No local ledger.jsonl files were found; the curve is empty.")
    return {
        "schema": CURVE_SCHEMA,
        "generated_from_latest_record_at": iso_timestamp(newest) if newest else None,
        "methodology": {
            "unit": "one session is a unique (version.crate, repo, session_id) tuple",
            "cost": "sum of token_mass.visible_tokens for every valid ledger turn in the session",
            "ordering": "numeric dotted version components first, then lexical components",
            "integrity": "all valid records are retained; malformed and foreign-schema counts are disclosed",
        },
        "sample": {
            "source_files_n": len(source_rows),
            "records_n": len(records),
            "sessions_n": len(sessions),
            "versions_n": len(curve),
            "malformed_records_n": malformed,
            "ignored_schema_records_n": ignored_schema,
        },
        "sources": source_rows,
        "curve": curve,
        "sessions": sorted(sessions.values(), key=lambda row: (version_key(row["version"]), row["repo"], row["session_id"])),
        "losses_disclosed": losses or ["No data-quality or sample-size loss was detected by the pipeline."],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ledgers", nargs="*", help="ledger.jsonl, recovery-cache.json, or directory; rotated .1 is included")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    paths = candidate_paths(args.ledgers)
    result = aggregate(paths)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"output": str(args.output), **result["sample"], "losses_disclosed": result["losses_disclosed"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
