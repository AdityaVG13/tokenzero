#!/usr/bin/env python3
"""Deterministic origin/main versus candidate literal-search benchmark.

Builds both revisions with the same release-perf profile, generates one shared
10k-100k-file corpus, verifies visible-output parity, and records p50/p95 wall
latency plus peak RSS when /usr/bin/time exposes it. No result is written unless
all requested runs complete successfully.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import random
import re
import shutil
import statistics
import subprocess
import tempfile
import time

SCENARIOS = {
    "absent_ascii": "TZ_ABSENT_7f4c2d",
    "rare_ascii": "TZ_RARE_91b7",
    "common_ascii": "TZ_COMMON",
    "unicode": "æ±äº¬ã«ãã§",
}


def run(command: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, env=env, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, check=True)


def source_tree_fingerprint(repo: Path) -> dict[str, object]:
    """Hash every tracked or unignored source byte used by the candidate."""
    paths = sorted(filter(None, run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"], cwd=repo
    ).stdout.splitlines()))
    digest = hashlib.sha256()
    count = 0
    for relative in paths:
        path = repo / relative
        if not path.is_file():
            continue
        encoded = relative.encode()
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        content = path.read_bytes()
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
        count += 1
    return {"sha256": digest.hexdigest(), "files": count}


def make_corpus(root: Path, files: int) -> None:
    rng = random.Random(0x5EED_2026)
    alphabet = "abcdefghijklmnopqrstuvwxyz0123456789_-"
    for index in range(files):
        directory = root / f"d{index // 1000:03d}"
        directory.mkdir(parents=True, exist_ok=True)
        lines = ["".join(rng.choices(alphabet, k=96)) for _ in range(8)]
        lines[2] += " TZ_COMMON"
        if index % 997 == 0:
            lines[5] += " TZ_RARE_91b7"
        if index % 17 == 0:
            lines[6] += " æ±äº¬ã«ãã§"
        (directory / f"f{index:06d}.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def timed_command(binary: Path, corpus: Path, query: str, cache: Path, max_results: int) -> tuple[float, int | None, str]:
    command = [str(binary), "find", query, str(corpus), "--max-files", str(max_results),
               "--max-visible-tokens", "100000000", "--mode", "passthrough", "--json",
               "--allowed-root", str(corpus), "--cache-path", str(cache)]
    time_bin = Path("/usr/bin/time")
    memory_pattern = None
    if time_bin.exists() and os.uname().sysname == "Darwin":
        command = [str(time_bin), "-l", *command]
        memory_pattern = re.compile(r"(\d+)\s+maximum resident set size")
    elif time_bin.exists():
        command = [str(time_bin), "-v", *command]
        memory_pattern = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")
    env = os.environ.copy()
    env["TOKENZERO_SEARCH_BACKEND"] = "internal"
    env["TOKENZERO_SESSION_DEDUP"] = "0"
    started = time.perf_counter()
    completed = subprocess.run(command, env=env, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, check=True)
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    memory = None
    if memory_pattern is not None:
        match = memory_pattern.search(completed.stderr)
        if match:
            memory = int(match.group(1))
            if os.uname().sysname != "Darwin":
                memory *= 1024
    payload = json.loads(completed.stdout)
    visible = payload.get("visible") or {}
    exact = visible.get("text", "")
    return elapsed_ms, memory, hashlib.sha256(exact.encode()).hexdigest()


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def summarize(samples: list[tuple[float, int | None, str]]) -> dict[str, object]:
    latency = [sample[0] for sample in samples]
    memory = [sample[1] for sample in samples if sample[1] is not None]
    return {
        "p50_ms": round(statistics.median(latency), 3),
        "p95_ms": round(percentile(latency, 0.95), 3),
        "samples_ms": [round(value, 3) for value in latency],
        "peak_rss_bytes": max(memory) if memory else None,
        "output_sha256": samples[0][2],
    }


def gate_exit_code(acceptance: dict[str, bool]) -> int:
    return 0 if acceptance.get("all_gates_pass") is True else 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--files", type=int, default=10_000)
    parser.add_argument("--runs", type=int, default=12)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--baseline", default="origin/main")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 10_000 <= args.files <= 100_000:
        parser.error("--files must be between 10000 and 100000")
    if args.runs < 3 or args.warmups < 1:
        parser.error("--runs must be >=3 and --warmups must be >=1")

    repo = Path(run(["git", "rev-parse", "--show-toplevel"], cwd=Path.cwd()).stdout.strip())
    with tempfile.TemporaryDirectory(prefix="tokenzero-literal-bench-") as temp_name:
        temp = Path(temp_name)
        baseline_tree = temp / "baseline"
        corpus = temp / "corpus"
        run(["git", "worktree", "add", "--detach", str(baseline_tree), args.baseline], cwd=repo)
        try:
            make_corpus(corpus, args.files)
            candidate_source = source_tree_fingerprint(repo)
            candidate_target = temp / "candidate-target"
            baseline_target = temp / "baseline-target"
            run(["cargo", "build", "--locked", "--profile", "release-perf", "-p", "tokenzero",
                 "--target-dir", str(candidate_target)], cwd=repo)
            run(["cargo", "build", "--locked", "--profile", "release-perf", "-p", "tokenzero",
                 "--target-dir", str(baseline_target)], cwd=baseline_tree)
            binaries = {
                "baseline": baseline_target / "release-perf" / "tokenzero",
                "candidate": candidate_target / "release-perf" / "tokenzero",
            }
            measured: dict[str, dict[str, list[tuple[float, int | None, str]]]] = {}
            max_results = args.files + 100
            for scenario, query in SCENARIOS.items():
                measured[scenario] = {"baseline": [], "candidate": []}
                # Alternate AB/BA order so cache and thermal drift cannot
                # systematically favor either revision.
                for warmup in range(args.warmups):
                    order = (("baseline", "candidate") if warmup % 2 == 0
                             else ("candidate", "baseline"))
                    for revision in order:
                        timed_command(binaries[revision], corpus, query,
                                      temp / f"warm-{scenario}-{revision}-{warmup}.json", max_results)
                for iteration in range(args.runs):
                    order = (("baseline", "candidate") if iteration % 2 == 0
                             else ("candidate", "baseline"))
                    for revision in order:
                        sample = timed_command(
                            binaries[revision], corpus, query,
                            temp / f"run-{scenario}-{revision}-{iteration}.json", max_results)
                        measured[scenario][revision].append(sample)
                hashes = {sample[2] for revision in measured[scenario].values() for sample in revision}
                if len(hashes) != 1:
                    raise RuntimeError(f"visible output mismatch for {scenario}: {sorted(hashes)}")

            scenarios: dict[str, object] = {}
            losses: list[dict[str, object]] = []
            gates: dict[str, bool] = {}
            for name, revisions in measured.items():
                baseline = summarize(revisions["baseline"])
                candidate = summarize(revisions["candidate"])
                changes = {}
                for metric in ("p50_ms", "p95_ms"):
                    change = (float(candidate[metric]) / float(baseline[metric]) - 1.0) * 100.0
                    changes[metric] = round(change, 2)
                    if change > 0:
                        losses.append({"scenario": name, "metric": metric, "regression_pct": round(change, 2)})
                if baseline["peak_rss_bytes"] and candidate["peak_rss_bytes"]:
                    memory_change = (float(candidate["peak_rss_bytes"]) / float(baseline["peak_rss_bytes"]) - 1.0) * 100.0
                    changes["peak_rss_bytes"] = round(memory_change, 2)
                    if memory_change > 0:
                        losses.append({"scenario": name, "metric": "peak_rss_bytes", "regression_pct": round(memory_change, 2)})
                scenarios[name] = {"baseline": baseline, "candidate": candidate, "change_pct": changes}
                limit = -25.0 if name in ("absent_ascii", "rare_ascii") else 10.0
                gates[name] = all(float(changes[metric]) <= limit for metric in ("p50_ms", "p95_ms"))

            result = {
                "schema": "tokenzero.literal-search-benchmark.v1",
                "methodology": {
                    "files": args.files, "lines_per_file": 8, "seed": "0x5EED2026",
                    "runs": args.runs, "warmups": args.warmups, "profile": "release-perf",
                    "backend": "internal", "same_corpus": True, "output_parity_required": True,
                    "baseline_ref": args.baseline,
                    "baseline_commit": run(["git", "rev-parse", "HEAD"], cwd=baseline_tree).stdout.strip(),
                    "candidate_head": run(["git", "rev-parse", "HEAD"], cwd=repo).stdout.strip(),
                    "candidate_source": candidate_source,
                    "candidate_dirty": bool(run(["git", "status", "--porcelain"], cwd=repo).stdout),
                    "machine": platform.machine(),
                    "os": platform.platform(),
                    "python": platform.python_version(),
                    "baseline_binary_sha256": hashlib.sha256(binaries["baseline"].read_bytes()).hexdigest(),
                    "candidate_binary_sha256": hashlib.sha256(binaries["candidate"].read_bytes()).hexdigest(),
                    "run_order": "alternating AB/BA per scenario iteration",
                },
                "scenarios": scenarios,
                "losses": losses,
                "acceptance": {
                    "rare_absent_p50_p95_at_least_25pct_faster": gates["rare_ascii"] and gates["absent_ascii"],
                    "common_unicode_p50_p95_regression_at_most_10pct": gates["common_ascii"] and gates["unicode"],
                    "all_gates_pass": all(gates.values()),
                },
            }
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
            print(json.dumps(result["acceptance"], sort_keys=True))
            return gate_exit_code(result["acceptance"])
        finally:
            run(["git", "worktree", "remove", "--force", str(baseline_tree)], cwd=repo)


if __name__ == "__main__":
    raise SystemExit(main())
