#!/usr/bin/env python3
"""Measure the internal-versus-rg find crossover without building."""
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
FILE_COUNTS = (100, 1_000, 5_000)
QUERY = "TZ_CROSSOVER_ABSENT_7f4c2d"


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, int(len(ordered) * fraction + 0.999999) - 1))
    return ordered[index]


def make_corpus(root: Path, files: int) -> None:
    line = "0123456789abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ\n"
    body = line * 8
    for index in range(files):
        directory = root / f"d{index // 100:04d}"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / f"f{index:06d}.txt").write_text(body)


def run_sample(
    corpus: Path,
    backend: str,
    max_files: int,
    state: Path,
) -> dict[str, Any]:
    env = os.environ.copy()
    env.update(
        {
            "TOKENZERO_MCP_DEDUP": "0",
            "TOKENZERO_REF_INDEX_PATH": str(state / "ref-index"),
            "TOKENZERO_SEARCH_BACKEND": backend,
        }
    )
    command = [
        str(BIN),
        "find",
        QUERY,
        str(corpus),
        "--max-files",
        str(max_files),
        "--max-visible-tokens",
        "1000000",
        "--mode",
        "passthrough",
        "--json",
        "--allowed-root",
        str(corpus),
        "--cache-path",
        str(state / "cache.json"),
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
    telemetry = payload.get("telemetry") or {}
    visible = payload.get("visible") or {}
    if telemetry.get("search_backend") != backend:
        raise RuntimeError(f"requested {backend}, got {telemetry.get('search_backend')}")
    return {
        "elapsed_ms": round(elapsed_ms, 6),
        "output_sha256": hashlib.sha256(str(visible.get("text", "")).encode()).hexdigest(),
        "matches": telemetry.get("matches"),
        "visited_files": telemetry.get("visited_files"),
        "truncated_by_visit": telemetry.get("truncated_by_visit"),
    }


def summarize(samples: list[dict[str, Any]]) -> dict[str, Any]:
    elapsed = [float(sample["elapsed_ms"]) for sample in samples]
    return {
        "p50_ms": round(statistics.median(elapsed), 6),
        "p95_ms": round(percentile(elapsed, 0.95), 6),
        "min_ms": round(min(elapsed), 6),
        "max_ms": round(max(elapsed), 6),
        "samples_ms": elapsed,
        "visited_files": samples[0]["visited_files"],
        "truncated_by_visit": samples[0]["truncated_by_visit"],
    }


def run(runs: int, warmups: int, output: Path) -> dict[str, Any]:
    if not BIN.is_file() or not os.access(BIN, os.X_OK):
        raise SystemExit(f"benchmark binary is missing or not executable: {BIN}")
    if runs < 3 or warmups < 1:
        raise SystemExit("runs must be >= 3 and warmups must be >= 1")

    rows: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="tokenzero-find-crossover-") as raw_temp:
        temp = Path(raw_temp)
        for files in FILE_COUNTS:
            corpus = temp / f"corpus-{files}"
            make_corpus(corpus, files)
            for warmup in range(warmups):
                order = ("internal", "rg") if warmup % 2 == 0 else ("rg", "internal")
                for backend in order:
                    run_sample(corpus, backend, files + 1, temp / f"warm-{files}-{warmup}-{backend}")

            measured: dict[str, list[dict[str, Any]]] = {"internal": [], "rg": []}
            for iteration in range(runs):
                order = ("internal", "rg") if iteration % 2 == 0 else ("rg", "internal")
                for backend in order:
                    sample = run_sample(
                        corpus,
                        backend,
                        files + 1,
                        temp / f"run-{files}-{iteration}-{backend}",
                    )
                    measured[backend].append(sample)

            hashes = {
                sample["output_sha256"]
                for backend_samples in measured.values()
                for sample in backend_samples
            }
            matches = {
                sample["matches"]
                for backend_samples in measured.values()
                for sample in backend_samples
            }
            if len(hashes) != 1 or matches != {0}:
                raise RuntimeError(f"backend output mismatch at {files} files")

            internal = summarize(measured["internal"])
            rg = summarize(measured["rg"])
            winner = "internal" if internal["p50_ms"] < rg["p50_ms"] else "rg"
            rows.append(
                {
                    "files": files,
                    "bytes_per_file": 512,
                    "internal": internal,
                    "rg": rg,
                    "p50_winner": winner,
                    "winner_margin_pct": round(
                        100
                        * abs(float(internal["p50_ms"]) - float(rg["p50_ms"]))
                        / max(float(internal["p50_ms"]), float(rg["p50_ms"])),
                        3,
                    ),
                    "output_sha256": hashes.pop(),
                }
            )

    crossover = next((row["files"] for row in rows if row["p50_winner"] == "rg"), None)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    evidence = {
        "schema": "tokenzero.find-backend-crossover.v1",
        "environment": {
            "binary": str(BIN.relative_to(REPO)) if BIN.is_relative_to(REPO) else str(BIN),
            "binary_sha256": hashlib.sha256(BIN.read_bytes()).hexdigest(),
            "commit": commit,
            "machine": platform.machine(),
            "os": platform.platform(),
            "python": platform.python_version(),
        },
        "methodology": {
            "query": QUERY,
            "scenario": "absent literal forces a complete scan without output-persistence bias",
            "corpus": "deterministic 512-byte text files in directories of 100 files each",
            "state": "fresh isolated recovery cache and ref index per sample",
            "run_order": "alternating internal/rg and rg/internal",
            "runs_per_backend_and_size": runs,
            "warmups_per_backend_and_size": warmups,
            "percentile": "nearest rank",
            "output_parity": "both backends must report zero matches and identical visible output",
        },
        "results": rows,
        "crossover_first_rg_win_files": crossover,
        "decision": {
            "auto_directory_backend": "retain rg",
            "internal_backend": "retain explicit selection and the existing auto single-file fast path",
            "reason": "Internal wins below the observed crossover, but selecting by exact tree size requires a pre-scan that consumes the same directory-I/O advantage. A shallow heuristic would be incorrect for deeply nested trees.",
        },
        "acceptance": {
            "internal_wins_small": rows[0]["p50_winner"] == "internal",
            "rg_wins_large": rows[-1]["p50_winner"] == "rg",
            "crossover_observed": crossover is not None,
            "all_scans_complete": all(
                not backend["truncated_by_visit"]
                for row in rows
                for backend in (row["internal"], row["rg"])
            ),
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence["acceptance"], sort_keys=True))
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=8)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--output", type=Path, default=EVIDENCE)
    args = parser.parse_args()
    evidence = run(args.runs, args.warmups, args.output)
    return 0 if all(evidence["acceptance"].values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
