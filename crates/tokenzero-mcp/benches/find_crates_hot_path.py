#!/usr/bin/env python3
"""Reproduce the S3 crates find hot-path measurement without building."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import statistics
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[3]
BIN = Path(os.environ.get("TOKENZERO_FIND_BENCH_BIN", REPO / "target/release/tokenzero"))
EVIDENCE = Path(__file__).with_suffix("") / "evidence.json"
QUERY = "fn "
PRIOR_P95_MS = 33.0


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, int(len(ordered) * fraction + 0.999999) - 1))
    return ordered[index]


def source_diff_sha256() -> str:
    diff = subprocess.run(
        ["git", "diff", "--binary"],
        cwd=REPO,
        capture_output=True,
        check=True,
    ).stdout
    return hashlib.sha256(diff).hexdigest()


def run_sample(temp: Path, index: int) -> dict[str, Any]:
    cache = temp / f"cache-{index}.json"
    env = os.environ.copy()
    env.update(
        {
            "TOKENZERO_MCP_DEDUP": "0",
            "TOKENZERO_REF_INDEX_PATH": str(temp / f"ref-index-{index}"),
            "TOKENZERO_SEARCH_BACKEND": "auto",
        }
    )
    command = [
        str(BIN),
        "find",
        QUERY,
        "crates",
        "--max-files",
        "200",
        "--max-visible-tokens",
        "1000000",
        "--mode",
        "passthrough",
        "--json",
        "--allowed-root",
        str(REPO),
        "--cache-path",
        str(cache),
    ]
    started = time.perf_counter_ns()
    completed = subprocess.run(
        command,
        cwd=REPO,
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    payload = json.loads(completed.stdout)
    if payload.get("status") != "ok":
        raise RuntimeError(f"find failed: {payload}")
    visible = payload.get("visible") or {}
    telemetry = payload.get("telemetry") or {}
    return {
        "elapsed_ms": round(elapsed_ms, 6),
        "output_sha256": hashlib.sha256(str(visible.get("text", "")).encode()).hexdigest(),
        "search_backend": telemetry.get("search_backend"),
        "matches": telemetry.get("matches"),
        "visited_files": telemetry.get("visited_files"),
    }


def run(runs: int, warmups: int, output: Path) -> dict[str, Any]:
    if not BIN.is_file() or not os.access(BIN, os.X_OK):
        raise SystemExit(f"benchmark binary is missing or not executable: {BIN}")
    if runs < 3 or warmups < 1:
        raise SystemExit("runs must be >= 3 and warmups must be >= 1")

    with tempfile.TemporaryDirectory(prefix="tokenzero-find-crates-") as raw_temp:
        temp = Path(raw_temp)
        for index in range(warmups):
            run_sample(temp, -index - 1)
        samples = [run_sample(temp, index) for index in range(runs)]

    output_hashes = {sample["output_sha256"] for sample in samples}
    backends = {sample["search_backend"] for sample in samples}
    matches = {sample["matches"] for sample in samples}
    if len(output_hashes) != 1 or len(backends) != 1 or len(matches) != 1:
        raise RuntimeError("benchmark output, backend, or match count changed between samples")

    elapsed = [float(sample["elapsed_ms"]) for sample in samples]
    p50_ms = statistics.median(elapsed)
    p95_ms = percentile(elapsed, 0.95)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    evidence = {
        "schema": "tokenzero.find-crates-hot-path.v1",
        "environment": {
            "binary": str(BIN.relative_to(REPO)) if BIN.is_relative_to(REPO) else str(BIN),
            "binary_sha256": hashlib.sha256(BIN.read_bytes()).hexdigest(),
            "commit": commit,
            "machine": platform.machine(),
            "os": platform.platform(),
            "python": platform.python_version(),
            "source_diff_sha256": source_diff_sha256(),
        },
        "methodology": {
            "command": "tokenzero find 'fn ' crates --max-files 200 --mode passthrough --json",
            "cache": "fresh isolated recovery cache and ref index per sample",
            "backend": "auto; the resolved backend is recorded and required to remain stable",
            "warmups": warmups,
            "runs": runs,
            "percentile": "nearest rank",
            "output_parity": "visible output SHA-256 and match count must be identical across samples",
        },
        "prior_evidence": {"p95_ms": PRIOR_P95_MS, "source": "hyperfine-S3_find.json"},
        "result": {
            "p50_ms": round(p50_ms, 6),
            "p95_ms": round(p95_ms, 6),
            "min_ms": round(min(elapsed), 6),
            "max_ms": round(max(elapsed), 6),
            "search_backend": samples[0]["search_backend"],
            "matches": samples[0]["matches"],
            "output_sha256": samples[0]["output_sha256"],
            "samples_ms": elapsed,
        },
        "acceptance": {
            "p95_below_prior_33ms": p95_ms < PRIOR_P95_MS,
            "improvement_pct": round(100 * (PRIOR_P95_MS - p95_ms) / PRIOR_P95_MS, 3),
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence["acceptance"], sort_keys=True))
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=20)
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--output", type=Path, default=EVIDENCE)
    args = parser.parse_args()
    evidence = run(args.runs, args.warmups, args.output)
    return 0 if evidence["acceptance"]["p95_below_prior_33ms"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
