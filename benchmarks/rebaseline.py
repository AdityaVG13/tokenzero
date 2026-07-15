#!/usr/bin/env python3
"""Run or assemble the northstar benchmark suite and retain trend history."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]
DEFAULT_BINARY = REPO / "target/release/tokenzero"
DEFAULT_OUTPUT = REPO / "benchmarks/northstar"
DEMO = REPO / "demo/demo_results.json"
BOOT = REPO / "benchmarks/boot-cost/candidate.json"
EXPAND = REPO / "crates/tokenzero-mcp/benches/expand_latency/evidence.json"


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def run_checked(command: list[str]) -> str:
    completed = subprocess.run(
        command,
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    )
    return completed.stdout


def compression_from_jsonl(text: str) -> dict[str, Any]:
    rows = [json.loads(line) for line in text.splitlines() if line.strip().startswith("{")]
    if not rows or rows[-1].get("workload") != "TOTAL":
        raise RuntimeError("compression benchmark did not emit a TOTAL row")
    total = rows.pop()
    workloads = [
        {
            "workload": row["workload"],
            "raw_tokens": int(row["raw_tokens"]),
            "visible_tokens": int(row["tokenzero_visible_tokens"]),
            "savings_pct": float(row["savings_pct"]),
        }
        for row in rows
    ]
    return {
        "workloads": workloads,
        "totals": {
            "raw_tokens": int(total["raw_tokens"]),
            "visible_tokens": int(total["tokenzero_visible_tokens"]),
            "savings_pct": float(total["savings_pct"]),
        },
    }


def compression_from_demo(data: dict[str, Any]) -> dict[str, Any]:
    workloads = [
        {
            "workload": row["workload"],
            "raw_tokens": int(row["raw_tokens"]),
            "visible_tokens": int(row["visible_tokens"]),
            "savings_pct": float(row["savings_pct"]),
        }
        for row in data["workloads"]
    ]
    totals = data["totals"]
    return {
        "workloads": workloads,
        "totals": {
            "raw_tokens": int(totals["raw_tokens"]),
            "visible_tokens": int(totals["visible_tokens"]),
            "savings_pct": float(totals["savings_pct"]),
        },
    }


def normalize_boot(data: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "corpus": row["label"],
            "files": int(row["file_count"]),
            "boot_tokens": int(row["boot_tokens"]),
            "components": row["components"],
        }
        for row in data["corpora"]
    ]


def normalize_expand(data: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "size_class": row["size_class"],
            "samples": int(row["n"]),
            "p50_ms": float(row["expand_p50_ms"]),
            "p95_ms": float(row["expand_p95_ms"]),
            "p99_ms": float(row["expand_p99_ms"]),
        }
        for row in data["table"]
    ]


def run_components(binary: Path, temp: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"release binary is missing or not executable: {binary}")
    debug = REPO / "target/debug/tokenzero"
    if not debug.is_file() or not os.access(debug, os.X_OK):
        raise SystemExit(f"debug binary is missing or not executable: {debug}")

    compression_text = run_checked(["sh", "scripts/benchmark_tokens.sh", str(binary)])
    run_checked(["uv", "run", "python", "benchmarks/boot-cost.py", "--label", "candidate"])
    expand_output = temp / "expand.json"
    run_checked(
        [
            "uv",
            "run",
            "python",
            "crates/tokenzero-mcp/benches/expand_latency.py",
            "--output",
            str(expand_output),
        ]
    )
    return compression_from_jsonl(compression_text), load_json(BOOT), load_json(expand_output)


def reused_components() -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    return compression_from_demo(load_json(DEMO)), load_json(BOOT), load_json(EXPAND)


def read_commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def source_state_sha256() -> str:
    status = subprocess.run(
        ["git", "status", "--short", "--untracked-files=all"],
        cwd=REPO,
        capture_output=True,
        check=True,
    ).stdout
    diff = subprocess.run(
        ["git", "diff", "--binary", "HEAD"],
        cwd=REPO,
        capture_output=True,
        check=True,
    ).stdout
    return hashlib.sha256(status + diff).hexdigest()


def trend(previous: dict[str, Any] | None, current: dict[str, Any]) -> dict[str, Any]:
    if previous is None:
        return {"previous_snapshot": None, "deltas": {}, "losses": []}
    deltas: dict[str, Any] = {
        "headline_savings_pct": round(
            float(current["compression"]["totals"]["savings_pct"])
            - float(previous["compression"]["totals"]["savings_pct"]),
            6,
        ),
        "boot_tokens": {},
        "expand_p50_ms": {},
    }
    losses: list[str] = []
    if deltas["headline_savings_pct"] < 0:
        losses.append(
            f"headline savings fell {abs(deltas['headline_savings_pct']):.3f} percentage points"
        )

    previous_boot = {row["corpus"]: row for row in previous["boot"]}
    for row in current["boot"]:
        prior = previous_boot.get(row["corpus"])
        if prior is None:
            continue
        delta = int(row["boot_tokens"]) - int(prior["boot_tokens"])
        deltas["boot_tokens"][row["corpus"]] = delta
        if delta > 0:
            losses.append(f"{row['corpus']} boot cost increased {delta} tokens")

    previous_expand = {row["size_class"]: row for row in previous["expand"]}
    for row in current["expand"]:
        prior = previous_expand.get(row["size_class"])
        if prior is None:
            continue
        delta = round(float(row["p50_ms"]) - float(prior["p50_ms"]), 6)
        deltas["expand_p50_ms"][row["size_class"]] = delta
        if delta > 0:
            losses.append(f"{row['size_class']} expand p50 increased {delta:.6f} ms")
    return {
        "previous_snapshot": previous["snapshot_id"],
        "deltas": deltas,
        "losses": losses,
    }


def render_markdown(snapshot: dict[str, Any]) -> str:
    compression = snapshot["compression"]
    total = compression["totals"]
    lines = [
        "# TokenZero Northstar",
        "",
        f"Snapshot: `{snapshot['snapshot_id']}`  ",
        f"Commit: `{snapshot['environment']['commit']}`  ",
        f"Mode: `{snapshot['environment']['mode']}`",
        "",
        "## Headline vs raw",
        "",
        "| Raw tokens | TokenZero visible | Savings |",
        "| ---: | ---: | ---: |",
        f"| {total['raw_tokens']:,} | {total['visible_tokens']:,} | **{total['savings_pct']:.1f}%** |",
        "",
        "## Per-operation compression",
        "",
        "| Workload | Raw tokens | TokenZero visible | Savings |",
        "| --- | ---: | ---: | ---: |",
    ]
    lines.extend(
        f"| {row['workload']} | {row['raw_tokens']:,} | {row['visible_tokens']:,} | {row['savings_pct']:.1f}% |"
        for row in compression["workloads"]
    )
    lines.extend(
        [
            "",
            "## Boot cost",
            "",
            "| Corpus | Files | Boot tokens |",
            "| --- | ---: | ---: |",
        ]
    )
    lines.extend(
        f"| {row['corpus']} | {row['files']:,} | {row['boot_tokens']} |"
        for row in snapshot["boot"]
    )
    lines.extend(
        [
            "",
            "## Expand latency",
            "",
            "| Size | Samples | p50 | p95 | p99 |",
            "| --- | ---: | ---: | ---: | ---: |",
        ]
    )
    lines.extend(
        f"| {row['size_class']} | {row['samples']} | {row['p50_ms']:.3f} ms | {row['p95_ms']:.3f} ms | {row['p99_ms']:.3f} ms |"
        for row in snapshot["expand"]
    )
    lines.extend(["", "## Trend", ""])
    if snapshot["trend"]["previous_snapshot"] is None:
        lines.append("Initial stored northstar snapshot; no prior trend exists.")
    elif snapshot["trend"]["losses"]:
        lines.extend(f"- {loss}" for loss in snapshot["trend"]["losses"])
    else:
        lines.append("No regression against the previous stored snapshot.")
    lines.append("")
    return "\n".join(lines)


def previous_snapshot(history: Path) -> dict[str, Any] | None:
    files = sorted(history.glob("*.json")) if history.is_dir() else []
    return load_json(files[-1]) if files else None


def write_outputs(snapshot: dict[str, Any], output_root: Path) -> tuple[Path, Path, Path]:
    history = output_root / "history"
    history.mkdir(parents=True, exist_ok=True)
    history_path = history / f"{snapshot['snapshot_id']}.json"
    current_json = output_root / "current.json"
    current_markdown = output_root / "current.md"
    encoded = json.dumps(snapshot, sort_keys=True, separators=(",", ":")) + "\n"
    history_path.write_text(encoded)
    current_json.write_text(encoded)
    current_markdown.write_text(render_markdown(snapshot))
    return history_path, current_json, current_markdown


def build_snapshot(
    compression: dict[str, Any],
    boot_data: dict[str, Any],
    expand_data: dict[str, Any],
    mode: str,
    prior: dict[str, Any] | None,
) -> dict[str, Any]:
    generated = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    commit = read_commit()
    snapshot: dict[str, Any] = {
        "schema": "tokenzero.northstar.v1",
        "snapshot_id": f"{generated.replace(':', '').replace('-', '')}-{commit[:12]}",
        "environment": {
            "generated_at_utc": generated,
            "commit": commit,
            "mode": mode,
            "machine": platform.machine(),
            "os": platform.platform(),
            "python": platform.python_version(),
            "source_state_sha256": source_state_sha256(),
        },
        "compression": compression,
        "boot": normalize_boot(boot_data),
        "expand": normalize_expand(expand_data),
        "integrity": {
            "all_compression_rows_retained": True,
            "all_boot_corpora_retained": True,
            "all_expand_size_classes_retained": True,
            "losses_published": True,
            "sources": [
                str(DEMO.relative_to(REPO)) if mode == "reuse-existing" else "scripts/benchmark_tokens.sh stdout",
                str(BOOT.relative_to(REPO)),
                str(EXPAND.relative_to(REPO)) if mode == "reuse-existing" else "fresh temporary expand evidence",
            ],
        },
    }
    snapshot["trend"] = trend(prior, snapshot)
    return snapshot


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reuse-existing", action="store_true")
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--output-root", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    history = args.output_root / "history"
    prior = previous_snapshot(history)
    with tempfile.TemporaryDirectory(prefix="tokenzero-rebaseline-") as raw_temp:
        if args.reuse_existing:
            compression, boot, expand = reused_components()
            mode = "reuse-existing"
        else:
            compression, boot, expand = run_components(args.binary, Path(raw_temp))
            mode = "run-components"
        snapshot = build_snapshot(compression, boot, expand, mode, prior)
    paths = write_outputs(snapshot, args.output_root)
    print(json.dumps({"snapshot_id": snapshot["snapshot_id"], "outputs": [str(path) for path in paths]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
