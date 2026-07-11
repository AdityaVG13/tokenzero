#!/usr/bin/env python3
"""Measure manifest+delta boot cost on this repo and a 100k-file corpus."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
BIN = REPO / "target/debug/tokenzero"
EVIDENCE = Path(__file__).with_suffix("")
GUARD = Path("/tmp/zerostack-heavy-process.guard")
SYNTHETIC_FILES = 100_000
COUNT_EXCLUDES = {".git", "target", ".zerostack"}


def acquire_guard(command: str) -> None:
    deadline = time.monotonic() + 600
    while True:
        try:
            GUARD.mkdir()
            break
        except FileExistsError:
            try:
                pid = int((GUARD / "pid").read_text().strip())
                os.kill(pid, 0)
            except (FileNotFoundError, ValueError, ProcessLookupError):
                for child in GUARD.iterdir():
                    if child.is_file():
                        child.unlink()
                GUARD.rmdir()
                continue
            except PermissionError as error:
                raise SystemExit(f"cannot inspect heavy-process guard owner: {error}")
            if time.monotonic() >= deadline:
                raise SystemExit(f"heavy-process guard still held by live pid {pid}")
            time.sleep(2)
    (GUARD / "pid").write_text(f"{os.getpid()}\n")
    (GUARD / "repository").write_text(f"{REPO}\n")
    (GUARD / "command").write_text(f"{command}\n")
    (GUARD / "started_at").write_text(
        datetime.now(timezone.utc).isoformat().replace("+00:00", "Z") + "\n"
    )


def release_guard() -> None:
    try:
        if (GUARD / "pid").read_text().strip() != str(os.getpid()):
            return
    except FileNotFoundError:
        return
    for child in GUARD.iterdir():
        if child.is_file():
            child.unlink()
    GUARD.rmdir()


def generate_synthetic(root: Path, count: int) -> None:
    remaining = count
    for shard in range((count + 999) // 1000):
        directory = root / f"d{shard:03d}"
        directory.mkdir()
        batch = min(remaining, 1000)
        for index in range(batch):
            (directory / f"f{index:04d}.txt").write_text(f"{shard:03d}:{index:04d}\n")
        remaining -= batch


def file_count(root: Path) -> int:
    total = 0
    for _, directories, files in os.walk(root):
        directories[:] = [name for name in directories if name not in COUNT_EXCLUDES]
        total += len(files)
    return total


def command_json(arguments: list[str]) -> dict[str, object]:
    started = time.perf_counter()
    process = subprocess.run(
        arguments, cwd=REPO, capture_output=True, text=True, check=True
    )
    elapsed_ms = round((time.perf_counter() - started) * 1000, 3)
    return {
        "argv": [str(value) for value in arguments],
        "elapsed_ms": elapsed_ms,
        "stdout_bytes": len(process.stdout.encode("utf-8")),
        "raw_json": json.loads(process.stdout),
        "stderr": process.stderr,
    }


def masked_argv(arguments: list[str], root: Path, cache: Path) -> list[str]:
    return [
        "<root>" if value == str(root) else "<temp-cache>" if value == str(cache) else value
        for value in arguments
    ]


def measure(label: str, root: Path, cache_dir: Path) -> dict[str, object]:
    cache = cache_dir / "recovery-cache.json"
    command = [
        str(BIN), "session-open", "--root", str(root),
        "--cache-path", str(cache), "--json",
    ]
    initialized = command_json(command)
    recorded = command_json(command)
    boot = recorded["raw_json"]
    components = boot["telemetry"]
    component_sum = sum(
        int(components[name])
        for name in ("manifest", "delta", "toc_working_set", "other")
    )
    total = int(components["total"])
    if component_sum != total:
        raise RuntimeError(f"{label} components {component_sum} != total {total}")
    if total >= 100:
        raise RuntimeError(f"{label} boot cost {total} is not below 100: {boot}")
    if boot.get("mode") != "manifest_delta":
        raise RuntimeError(f"{label} did not use manifest+delta boot: {boot}")
    if boot.get("demand_paging", {}).get("working_set_loaded") is not False:
        raise RuntimeError(f"{label} eagerly loaded the working set: {boot}")
    initialized["argv"] = masked_argv(command, root, cache)
    recorded["argv"] = masked_argv(command, root, cache)
    return {
        "label": label,
        "root": str(root),
        "file_count": file_count(root),
        "file_count_excludes": sorted(COUNT_EXCLUDES),
        "metadata_initialization": initialized,
        "recorded_manifest_delta_boot": recorded,
        "boot_tokens": total,
        "components": components,
        "component_sum": component_sum,
    }


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def environment(label: str) -> dict[str, object]:
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True,
        text=True, check=True,
    ).stdout.strip()
    return {
        "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "harness_command": f"python3 benchmarks/boot-cost.py --label {label}",
        "cwd": str(REPO),
        "os": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "commit": commit,
        "worktree_note": "Measurements intentionally include the uncommitted bead implementation.",
        "binary": str(BIN.relative_to(REPO)),
        "binary_sha256": sha256(BIN),
        "binary_mtime_ns": BIN.stat().st_mtime_ns,
        "cargo_build_jobs": os.environ.get("CARGO_BUILD_JOBS"),
        "cargo_incremental": os.environ.get("CARGO_INCREMENTAL"),
        "synthetic_generator": "100 directories x 1000 deterministic 8-byte text files",
    }


def run(label: str) -> Path:
    if not BIN.is_file():
        raise SystemExit("target/debug/tokenzero missing; build the focused tokenzero package first")
    acquire_guard(f"python3 benchmarks/boot-cost.py --label {label}")
    try:
        with tempfile.TemporaryDirectory(prefix="tokenzero-boot-cost-") as raw:
            tmp = Path(raw)
            synthetic = tmp / "synthetic-100k"
            synthetic.mkdir()
            generate_synthetic(synthetic, SYNTHETIC_FILES)
            result = {
                "schema": "tokenzero.boot-cost.v1",
                "environment": environment(label),
                "corpora": [
                    measure("repository", REPO, tmp / "repository-cache"),
                    measure("synthetic-100k", synthetic, tmp / "synthetic-cache"),
                ],
                "assertions": {
                    "max_boot_tokens_exclusive": 100,
                    "component_totals_match": True,
                    "working_set_demand_paged": True,
                    "all_passed": True,
                },
            }
            EVIDENCE.mkdir(parents=True, exist_ok=True)
            destination = EVIDENCE / f"{label}.json"
            destination.write_text(json.dumps(result, indent=2) + "\n")
            return destination
    finally:
        release_guard()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", default="candidate")
    args = parser.parse_args()
    print(run(args.label).relative_to(REPO))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
