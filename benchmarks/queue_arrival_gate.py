#!/usr/bin/env python3
"""Fail-closed gate: refuse queue stability claims without arrival/service dists.

P03-MG-002: expand p50 percentiles cannot establish queue stability. This gate
(1) reasserts the ESTIMATE_ONLY_MEASUREMENT_GAP packet, (2) refuses treating
estimated arrival rates or p50-as-E[S] as observed, and (3) only ACCEPT_MEASURED
when a complete arrival/service sample file reports lambda, E[S], and Var[S].

A micro harness (``queue_arrival_harness.py``) emits synthetic timestamped
arrivals + service samples for the evidence path without a long campaign.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]
QUEUE_PACKET = (
    REPO
    / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence/queue-bound.json"
)
SAMPLES_REL = Path("benchmarks/claims/queue-arrival-samples.json")


def _dist_complete(block: dict[str, Any]) -> bool:
    required = (
        "arrival_rate_ops_per_second",
        "service_mean_seconds",
        "service_variance_seconds2",
        "sample_count",
        "concurrency",
    )
    if any(k not in block for k in required):
        return False
    if int(block["sample_count"]) < 1:
        return False
    # Provenance must not pretend estimate-only proxies are measured.
    for key in ("arrival_rate_ops_per_second", "service_mean_seconds"):
        meta = block.get(f"{key}_provenance") or block.get("provenance")
        if meta in ("estimated_scenario", "estimated_from_measured_p50"):
            return False
    return True


def evaluate(root: Path | None = None) -> dict[str, Any]:
    base = root or REPO
    packet_path = (
        base
        / ".math-review/runs/20260716T073818Z-tokenzero/passes/pass-03/evidence/queue-bound.json"
        if root is not None
        else QUEUE_PACKET
    )
    if packet_path.is_file():
        packet = json.loads(packet_path.read_text())
    else:
        packet = {
            "status": "ESTIMATE_ONLY_MEASUREMENT_GAP",
            "measurement_gap": {"candidate_id": "P03-MG-002"},
            "rows": [],
        }

    samples_path = base / SAMPLES_REL
    samples_doc: dict[str, Any] | None = None
    if samples_path.is_file():
        samples_doc = json.loads(samples_path.read_text())

    status = packet.get("status")
    gap = packet.get("measurement_gap") or {}
    gap_ok = gap.get("candidate_id") == "P03-MG-002"
    estimate_only = status == "ESTIMATE_ONLY_MEASUREMENT_GAP"

    rows = packet.get("rows") or []
    estimated_arrivals = all(
        (r.get("arrival_rate_ops_per_second") or {}).get("provenance")
        == "estimated_scenario"
        for r in rows
    ) if rows else True
    estimated_service = all(
        str((r.get("service_mean_seconds_proxy") or {}).get("provenance", "")).startswith(
            "estimated"
        )
        for r in rows
    ) if rows else True

    size_classes = [
        str(r.get("size_class"))
        for r in (samples_doc or {}).get("size_classes") or []
    ]
    complete_classes = [
        str(r["size_class"])
        for r in (samples_doc or {}).get("size_classes") or []
        if _dist_complete(r)
    ]
    incomplete = [c for c in size_classes if c not in complete_classes]

    north_expand = []
    north_path = base / "benchmarks/northstar/current.json"
    if north_path.is_file():
        north = json.loads(north_path.read_text())
        north_expand = [str(r["size_class"]) for r in north.get("expand") or []]

    missing = [c for c in north_expand if c not in complete_classes]
    measured = (
        samples_doc is not None
        and not missing
        and not incomplete
        and bool(complete_classes)
        and samples_doc.get("observed") is True
    )

    if not gap_ok or (rows and not (estimate_only and estimated_arrivals and estimated_service)):
        # Packet must keep the estimate-only contract while samples are absent.
        if measured:
            result = "ACCEPT_MEASURED"
        elif not gap_ok:
            result = "REJECT_PACKET_CONTRACT"
        elif rows and not estimate_only:
            result = "REJECT_FALSE_STABILITY_CLAIM"
        else:
            result = "ACCEPT_GATED_MEASUREMENT_GAP"
    elif measured:
        result = "ACCEPT_MEASURED"
    else:
        result = "ACCEPT_GATED_MEASUREMENT_GAP"

    return {
        "gate": "queue-arrival-service",
        "candidate_id": "P03-MG-002",
        "packet_status": status,
        "measurement_gap_candidate_id": gap.get("candidate_id"),
        "estimate_only_packet": estimate_only,
        "estimated_arrival_provenance": estimated_arrivals,
        "estimated_service_provenance": estimated_service,
        "samples_path": str(SAMPLES_REL),
        "samples_present": samples_doc is not None,
        "size_classes_required": north_expand,
        "size_classes_complete": complete_classes,
        "size_classes_missing": missing,
        "size_classes_incomplete": incomplete,
        "note": (
            "Queue stability requires observed arrival rate, service mean, and "
            "service variance under controlled concurrency. p50 proxies and "
            "invented 1 op/s arrivals stay ESTIMATE_ONLY. Populate "
            f"{SAMPLES_REL} via the micro harness or a real capture before "
            "claiming rho<1 stability."
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
