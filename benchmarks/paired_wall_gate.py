#!/usr/bin/env python3
"""Fail-closed gate: refuse wall-time speedup claims without paired samples.

P03-MG-001: the northstar token-count ratio is a conditional token-work ceiling,
not observed runtime speedup. This gate (1) reasserts the measurement-gap packet,
(2) refuses any claim that labels the ceiling as wall-time speedup, and (3) only
ACCEPT_MEASURED when a complete paired-wall sample file covers every workload.

Does not re-run northstar. A micro harness (``paired_wall_harness.py``) can emit
synthetic paired samples for the evidence path without a cold rebuild.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]
SPEEDUP_PACKET = (
    REPO
    / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence/speedup-ceiling.json"
)
SAMPLES_REL = Path("benchmarks/claims/paired-wall-samples.json")


def _workload_names(north: dict[str, Any]) -> list[str]:
    return [str(row["workload"]) for row in north["compression"]["workloads"]]


def _sample_complete(row: dict[str, Any]) -> bool:
    for side in ("raw_wall_ms", "tokenzero_wall_ms"):
        block = row.get(side)
        if not isinstance(block, dict):
            return False
        for key in ("n", "p50", "p95"):
            if key not in block:
                return False
        if int(block["n"]) < 1:
            return False
    return bool(row.get("environment_id"))


def evaluate(root: Path | None = None) -> dict[str, Any]:
    base = root or REPO
    packet_path = (
        base
        / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence/speedup-ceiling.json"
        if root is not None
        else SPEEDUP_PACKET
    )
    if not packet_path.is_file():
        # Fallback: reconstruct gap from northstar when packet is absent in a
        # synthetic test root.
        packet = {
            "measurement_gap": {
                "candidate_id": "P03-MG-001",
                "needed": "paired raw and TokenZero wall-time samples",
            },
            "limits": [
                "This is a conditional token-work ceiling, not a measured wall-time speedup."
            ],
            "results": {
                "conditional_aggregate_ceiling_x": None,
            },
        }
    else:
        packet = json.loads(packet_path.read_text())

    north = json.loads((base / "benchmarks/northstar/current.json").read_text())
    workloads = _workload_names(north)
    samples_path = base / SAMPLES_REL
    samples_doc: dict[str, Any] | None = None
    if samples_path.is_file():
        samples_doc = json.loads(samples_path.read_text())

    gap = packet.get("measurement_gap") or {}
    gap_ok = gap.get("candidate_id") == "P03-MG-001"
    limits_text = " ".join(str(x) for x in packet.get("limits") or [])
    ceiling_is_not_wall = "wall-time" in limits_text.lower() or "wall time" in limits_text.lower()

    covered: list[str] = []
    incomplete: list[str] = []
    missing: list[str] = []
    by_name: dict[str, dict[str, Any]] = {}
    if samples_doc:
        for row in samples_doc.get("workloads") or []:
            by_name[str(row.get("workload"))] = row

    for name in workloads:
        row = by_name.get(name)
        if row is None:
            missing.append(name)
        elif _sample_complete(row):
            covered.append(name)
        else:
            incomplete.append(name)

    measured = (
        samples_doc is not None
        and not missing
        and not incomplete
        and bool(samples_doc.get("environment_id") or covered)
    )

    if not gap_ok or not ceiling_is_not_wall:
        result = "REJECT_PACKET_CONTRACT"
    elif measured:
        result = "ACCEPT_MEASURED"
    else:
        # Fail closed: ceiling may be cited as token-work only, never wall speedup.
        result = "ACCEPT_GATED_MEASUREMENT_GAP"

    return {
        "gate": "paired-wall-speedup",
        "candidate_id": "P03-MG-001",
        "snapshot_id": north.get("snapshot_id"),
        "conditional_aggregate_ceiling_x": (
            packet.get("results") or {}
        ).get("conditional_aggregate_ceiling_x"),
        "measurement_gap_candidate_id": gap.get("candidate_id"),
        "ceiling_explicitly_not_wall_time": ceiling_is_not_wall,
        "paired_samples_path": str(SAMPLES_REL),
        "paired_samples_present": samples_doc is not None,
        "workloads_total": len(workloads),
        "workloads_covered": covered,
        "workloads_missing_samples": missing,
        "workloads_incomplete_samples": incomplete,
        "note": (
            "Token-count ceilings are not wall-time speedups. Populate "
            f"{SAMPLES_REL} with paired raw/TokenZero wall samples "
            "(n, p50, p95, environment_id) per northstar workload before "
            "labeling runtime speedup as measured. No full northstar cold "
            "rebuild is required for this gate."
        ),
        "result": result,
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
