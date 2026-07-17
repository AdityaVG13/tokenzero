#!/usr/bin/env python3
"""Gate universal token-work savings claims against per-workload expansion.

P03-001: aggregate northstar savings must not be read as "every workload
reduces token work." The checked-in cargo-test row expands (233 -> 257).
This gate fails closed if any *other* workload expands, and always reports
that universal savings are falsified while any expander remains.

Does not re-run northstar; it only audits ``benchmarks/northstar/current.json``.
Fixing the cargo-test compressor itself requires a fresh northstar suite and
is intentionally out of scope here.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]

# Known expander retained in the fixed northstar suite until a compressor fix
# lands via a full northstar rebaseline (not this gate).
ALLOWED_EXPANDERS = (
    "cargo test run (tokenzero-filters suite)",
)


def evaluate(root: Path | None = None) -> dict[str, Any]:
    base = root or REPO
    north = json.loads((base / "benchmarks/northstar/current.json").read_text())
    expanders: list[dict[str, Any]] = []
    for row in north["compression"]["workloads"]:
        raw = int(row["raw_tokens"])
        visible = int(row["visible_tokens"])
        if visible > raw:
            expanders.append(
                {
                    "workload": row["workload"],
                    "raw_tokens": raw,
                    "visible_tokens": visible,
                    "conditional_token_work_speedup_ceiling_x": round(raw / visible, 6),
                }
            )

    unexpected = [
        row for row in expanders if row["workload"] not in ALLOWED_EXPANDERS
    ]
    missing_allowed = [
        name
        for name in ALLOWED_EXPANDERS
        if not any(row["workload"] == name for row in expanders)
    ]

    universal = len(expanders) == 0
    # Accept while the known cargo-test counterexample is the only expander.
    ok = not unexpected and (
        universal
        or (
            len(expanders) == len(ALLOWED_EXPANDERS)
            and not missing_allowed
        )
    )

    return {
        "gate": "workload-token-regression",
        "candidate_id": "P03-001",
        "snapshot_id": north.get("snapshot_id"),
        "universal_token_work_savings": universal,
        "expanders": expanders,
        "allowed_expanders": list(ALLOWED_EXPANDERS),
        "unexpected_expanders": unexpected,
        "missing_allowed_expanders": missing_allowed,
        "note": (
            "cargo-test visible>raw is a known fixed-suite counterexample; "
            "eliminating it needs a compressor change plus a full northstar "
            "rebaseline (not run by this gate)."
        ),
        "result": "ACCEPT_GATED" if ok else "REJECT_UNEXPECTED_EXPANSION",
    }


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    out = evaluate()
    text = json.dumps(out, indent=2, sort_keys=True) + "\n"
    if "--output" in args:
        Path(args[args.index("--output") + 1]).write_text(text)
    print(text, end="")
    return 0 if out["result"].startswith("ACCEPT_") else 1


if __name__ == "__main__":
    raise SystemExit(main())
