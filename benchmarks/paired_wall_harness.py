#!/usr/bin/env python3
"""Minimal paired wall-time harness (P03-MG-001 evidence path).

Times a tiny synthetic workload twice (raw vs TokenZero-shaped no-op path)
and writes a sample document schema. This is intentionally NOT a northstar
rebuild -- it only proves the measurement path and schema for the gate.

Usage:
  python3 benchmarks/paired_wall_harness.py --output benchmarks/claims/paired-wall-samples.micro.json
"""
from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path
from typing import Any


def _p(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, int(round(q * (len(ordered) - 1)))))
    return ordered[idx]


def _time_side(fn, n: int) -> dict[str, Any]:
    samples: list[float] = []
    for _ in range(n):
        t0 = time.perf_counter()
        fn()
        samples.append((time.perf_counter() - t0) * 1000.0)
    return {
        "n": n,
        "p50": round(_p(samples, 0.50), 6),
        "p95": round(_p(samples, 0.95), 6),
        "mean": round(statistics.fmean(samples), 6),
    }


def run_micro(n: int = 5) -> dict[str, Any]:
    payload = ("x" * 4096 + "\n") * 8

    def raw() -> None:
        # Stand-in for uncompressed scan / token walk.
        _ = sum(1 for _ in payload.splitlines())

    def tokenzero_shaped() -> None:
        # Stand-in for capsule path: touch a short visible view only.
        visible = payload[:64]
        _ = sum(1 for _ in visible.splitlines())

    return {
        "schema": "tokenzero.paired-wall-samples.v1",
        "candidate_id": "P03-MG-001",
        "environment_id": "micro-harness-local",
        "note": (
            "Synthetic micro samples for the evidence path only. "
            "Not a northstar paired-wall measurement; do not publish as "
            "runtime speedup for the fixed suite."
        ),
        "workloads": [
            {
                "workload": "micro synthetic line-count",
                "environment_id": "micro-harness-local",
                "raw_wall_ms": _time_side(raw, n),
                "tokenzero_wall_ms": _time_side(tokenzero_shaped, n),
            }
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--n", type=int, default=5)
    args = parser.parse_args()
    doc = run_micro(n=args.n)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"wrote": str(args.output), "workloads": len(doc["workloads"])}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
