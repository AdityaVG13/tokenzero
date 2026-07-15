#!/usr/bin/env python3
"""Deterministic 100k-file origin/main versus candidate walker evidence.

Builds both revisions identically, generates shared shallow and deep corpora,
alternates fresh-process runs, verifies exact visible-output hashes, and writes
p50/p95/peak-RSS evidence only after every run succeeds.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import shlex
import statistics
import subprocess
import tempfile
import time

OPERATIONS = {
    "tree": lambda root, count: ["tree", str(root), "--depth", "16", "--max-files", str(count + 2000)],
    "glob": lambda root, count: ["glob", "*.txt", str(root), "--max-files", str(count + 100)],
    "search": lambda root, count: ["find", "TZ_ABSENT_8f31c9", str(root), "--max-files", str(count + 100)],
}


def run(command: list[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(command, cwd=cwd, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
    if completed.returncode:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {completed.stderr.strip()}"
        )
    return completed


def make_corpus(root: Path, files: int, deep: bool) -> None:
    rng = random.Random(0xA11CE_2026 + int(deep))
    alphabet = "abcdefghijklmnopqrstuvwxyz0123456789_-"
    for index in range(files):
        bucket = index // 100
        if deep:
            directory = root / f"d{bucket:04d}" / "a" / "b" / "c" / "d"
        else:
            directory = root / f"d{bucket:04d}"
        directory.mkdir(parents=True, exist_ok=True)
        payload = "".join(rng.choices(alphabet, k=192))
        if index % 997 == 0:
            payload += " café 東京"
        (directory / f"f{index:06d}.txt").write_text(payload + "\n", encoding="utf-8")
    (root / ".hidden.txt").write_text("TZ_ABSENT_8f31c9\n")
    (root / "target").mkdir()
    (root / "target" / "ignored.txt").write_text("TZ_ABSENT_8f31c9\n")


def timed(binary: Path, arguments: list[str], cache: Path,
          cold_command: list[str] | None, allowed_root: Path) -> tuple[float, int | None, str]:
    if cold_command:
        subprocess.run(cold_command, check=True, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    command = [str(binary), *arguments, "--max-visible-tokens", "1000000000",
               "--mode", "passthrough", "--json", "--cache-path", str(cache),
               "--allowed-root", str(allowed_root)]
    memory_pattern = None
    if Path("/usr/bin/time").exists() and os.uname().sysname == "Darwin":
        command = ["/usr/bin/time", "-l", *command]
        memory_pattern = re.compile(r"(\d+)\s+maximum resident set size")
    elif Path("/usr/bin/time").exists():
        command = ["/usr/bin/time", "-v", *command]
        memory_pattern = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")
    env = os.environ.copy()
    env["TOKENZERO_SEARCH_BACKEND"] = "internal"
    env["TOKENZERO_SESSION_DEDUP"] = "0"
    started = time.perf_counter()
    completed = subprocess.run(command, env=env, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
    if completed.returncode:
        raise RuntimeError(
            f"benchmark command failed ({completed.returncode}): "
            f"stdout={completed.stdout.strip()} stderr={completed.stderr.strip()}"
        )
    elapsed = (time.perf_counter() - started) * 1000.0
    rss = None
    if memory_pattern:
        match = memory_pattern.search(completed.stderr)
        if match:
            rss = int(match.group(1)) * (1024 if os.uname().sysname != "Darwin" else 1)
    payload = json.loads(completed.stdout)
    visible = (payload.get("visible") or {}).get("text", "")
    return elapsed, rss, hashlib.sha256(visible.encode()).hexdigest()


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def summarize(samples: list[tuple[float, int | None, str]]) -> dict[str, object]:
    latency = [sample[0] for sample in samples]
    rss = [sample[1] for sample in samples if sample[1] is not None]
    return {"p50_ms": round(statistics.median(latency), 3),
            "p95_ms": round(percentile(latency, 0.95), 3),
            "samples_ms": [round(value, 3) for value in latency],
            "peak_rss_bytes": max(rss) if rss else None,
            "output_sha256": samples[0][2]}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--files", type=int, default=100_000)
    parser.add_argument("--runs", type=int, default=12)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--baseline", default="origin/main")
    parser.add_argument(
        "--cold-command",
        help="command run before every sample to evict filesystem caches (for example: purge)",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.files != 100_000:
        parser.error("--files must be exactly 100000 for acceptance evidence")
    if args.runs < 5 or args.warmups < 1:
        parser.error("--runs must be >=5 and --warmups must be >=1")

    repo = Path(run(["git", "rev-parse", "--show-toplevel"], cwd=Path.cwd()).stdout.strip())
    cold_command = shlex.split(args.cold_command) if args.cold_command else None
    with tempfile.TemporaryDirectory(prefix="tokenzero-parallel-walker-") as name:
        temp = Path(name)
        baseline_tree = temp / "baseline"
        run(["git", "worktree", "add", "--detach", str(baseline_tree), args.baseline], cwd=repo)
        try:
            corpora = {"shallow": temp / "shallow", "deep": temp / "deep"}
            make_corpus(corpora["shallow"], args.files, False)
            make_corpus(corpora["deep"], args.files, True)
            targets = {"candidate": temp / "candidate-target", "baseline": temp / "baseline-target"}
            run(["cargo", "build", "--locked", "--profile", "release-perf", "-p", "tokenzero",
                 "--target-dir", str(targets["candidate"])], cwd=repo)
            run(["cargo", "build", "--locked", "--profile", "release-perf", "-p", "tokenzero",
                 "--target-dir", str(targets["baseline"])], cwd=baseline_tree)
            binaries = {key: target / "release-perf" / "tokenzero" for key, target in targets.items()}
            measured: dict[str, dict[str, list[tuple[float, int | None, str]]]] = {}
            for shape, corpus in corpora.items():
                for operation, make_arguments in OPERATIONS.items():
                    scenario = f"{shape}_{operation}"
                    measured[scenario] = {"baseline": [], "candidate": []}
                    arguments = make_arguments(corpus, args.files)
                    for warmup in range(args.warmups):
                        for revision in ("baseline", "candidate"):
                            timed(binaries[revision], arguments,
                                  temp / f"warm-{scenario}-{revision}-{warmup}.json", cold_command, corpus)
                    for iteration in range(args.runs):
                        order = ("baseline", "candidate") if iteration % 2 == 0 else ("candidate", "baseline")
                        for revision in order:
                            sample = timed(binaries[revision], arguments,
                                           temp / f"run-{scenario}-{revision}-{iteration}.json",
                                           cold_command, corpus)
                            measured[scenario][revision].append(sample)
                    hashes = {sample[2] for samples in measured[scenario].values() for sample in samples}
                    if len(hashes) != 1:
                        raise RuntimeError(f"visible output mismatch for {scenario}: {sorted(hashes)}")

            scenarios: dict[str, object] = {}
            losses: list[dict[str, object]] = []
            gates: dict[str, bool] = {}
            rss_gates: dict[str, bool | None] = {}
            for name, revisions in measured.items():
                baseline = summarize(revisions["baseline"])
                candidate = summarize(revisions["candidate"])
                changes: dict[str, float] = {}
                for metric in ("p50_ms", "p95_ms"):
                    change = (float(candidate[metric]) / float(baseline[metric]) - 1.0) * 100.0
                    changes[metric] = round(change, 2)
                    if change > 0:
                        losses.append({"scenario": name, "metric": metric,
                                       "regression_pct": round(change, 2)})
                rss_ok = True
                if baseline["peak_rss_bytes"] and candidate["peak_rss_bytes"]:
                    ratio = float(candidate["peak_rss_bytes"]) / float(baseline["peak_rss_bytes"])
                    changes["peak_rss_bytes"] = round((ratio - 1.0) * 100.0, 2)
                    rss_ok = ratio <= 1.25
                    if ratio > 1.0:
                        losses.append({"scenario": name, "metric": "peak_rss_bytes",
                                       "regression_pct": changes["peak_rss_bytes"]})
                scenarios[name] = {"baseline": baseline, "candidate": candidate,
                                   "change_pct": changes}
                rss_gates[name] = rss_ok if baseline["peak_rss_bytes"] and candidate["peak_rss_bytes"] else None
                gates[name] = changes["p50_ms"] <= -20.0 and changes["p95_ms"] <= 0.0 and rss_ok

            result = {"schema": "tokenzero.parallel-walker-benchmark.v1",
                      "methodology": {"files_per_shape": args.files, "seed": "0xA11CE2026",
                                      "runs": args.runs, "warmups": args.warmups,
                                      "profile": "release-perf", "backend": "internal",
                                      "same_corpus": True, "fresh_process_each_sample": True,
                                      "alternating_revision_order": True,
                                      "os_cache_state": "evicted before every sample" if cold_command else "uncontrolled; fresh-process only",
                                      "cold_command": args.cold_command,
                                      "output_parity_required": True, "baseline_ref": args.baseline,
                                      "baseline_commit": run(["git", "rev-parse", "HEAD"], cwd=baseline_tree).stdout.strip(),
                                      "candidate_commit": run(["git", "rev-parse", "HEAD"], cwd=repo).stdout.strip()},
                      "scenarios": scenarios, "losses": losses,
                      "acceptance": {"p50_at_least_20pct_faster": all(v["change_pct"]["p50_ms"] <= -20.0 for v in scenarios.values()),
                                     "p95_not_worse": all(v["change_pct"]["p95_ms"] <= 0.0 for v in scenarios.values()),
                                     "rss_at_most_1_25x": all(value is True for value in rss_gates.values()) if all(value is not None for value in rss_gates.values()) else None,
                                     "all_gates_pass": all(gates.values()) and all(value is True for value in rss_gates.values())}}
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
            print(json.dumps(result["acceptance"], sort_keys=True))
        finally:
            run(["git", "worktree", "remove", "--force", str(baseline_tree)], cwd=repo)


if __name__ == "__main__":
    main()
