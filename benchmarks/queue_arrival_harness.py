#!/usr/bin/env python3
"""Minimal queue arrival/service harness (P03-MG-002 evidence path).

Generates timestamped arrivals and per-request service durations for a
synthetic single-server loop, then reports observed lambda, E[S], Var[S],
and utilization. Not a production load test -- schema + instrumentation only.

Usage:
  python3 benchmarks/queue_arrival_harness.py --output benchmarks/claims/queue-arrival-samples.micro.json
"""
from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path
from typing import Any


def _service_once(payload_bytes: int) -> float:
    payload = b"x" * payload_bytes
    t0 = time.perf_counter()
    _ = sum(payload)  # cheap CPU stand-in for expand work
    return time.perf_counter() - t0


def capture(size_class: str, payload_bytes: int, n: int, concurrency: int = 1) -> dict[str, Any]:
    arrivals: list[float] = []
    services: list[float] = []
    t_start = time.perf_counter()
    for i in range(n):
        arrivals.append(time.perf_counter())
        # Controlled single-server: no overlapping service when concurrency=1.
        services.append(_service_once(payload_bytes))
        # Small inter-arrival gap so lambda is observable and finite.
        time.sleep(0.001)
    wall = time.perf_counter() - t_start
    inter = [arrivals[i] - arrivals[i - 1] for i in range(1, len(arrivals))]
    mean_inter = statistics.fmean(inter) if inter else wall
    lam = (1.0 / mean_inter) if mean_inter > 0 else 0.0
    mean_s = statistics.fmean(services)
    var_s = statistics.pvariance(services) if len(services) > 1 else 0.0
    return {
        "size_class": size_class,
        "concurrency": concurrency,
        "sample_count": n,
        "arrival_timestamps_unix_perf": [round(a, 9) for a in arrivals],
        "service_seconds": [round(s, 9) for s in services],
        "arrival_rate_ops_per_second": round(lam, 6),
        "arrival_rate_ops_per_second_provenance": "observed_harness",
        "service_mean_seconds": round(mean_s, 9),
        "service_mean_seconds_provenance": "observed_harness",
        "service_variance_seconds2": round(var_s, 12),
        "utilization_rho": round(lam * mean_s, 9),
        "provenance": "observed_harness",
        "payload_bytes": payload_bytes,
    }


def run_micro(n: int = 8) -> dict[str, Any]:
    classes = [
        ("1KB", 1024),
        ("100KB", 100 * 1024),
    ]
    rows = [capture(name, size, n=n, concurrency=1) for name, size in classes]
    return {
        "schema": "tokenzero.queue-arrival-samples.v1",
        "candidate_id": "P03-MG-002",
        "observed": True,
        "note": (
            "Synthetic micro capture for the instrumentation/evidence path. "
            "Covers only two size classes; not a stability claim for the "
            "full northstar expand matrix."
        ),
        "size_classes": rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--n", type=int, default=8)
    args = parser.parse_args()
    doc = run_micro(n=args.n)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
    print(
        json.dumps(
            {
                "wrote": str(args.output),
                "size_classes": [r["size_class"] for r in doc["size_classes"]],
                "rho": [r["utilization_rho"] for r in doc["size_classes"]],
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
