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
BOOT = REPO / "benchmarks/boot-cost/candidate.json"
METHODOLOGY = {
    "compression": "benchmark_tokens.v1",
    "boot": "boot-cost.v1",
    "expand": "expand-latency.v1",
    "binary_shared_across_legs": True,
}


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def run_checked(command: list[str], *, env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(
        command,
        cwd=REPO,
        env=env,
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


def binary_provenance(binary: Path) -> dict[str, str]:
    resolved = binary.expanduser().resolve()
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise SystemExit(f"benchmark binary is missing or not executable: {resolved}")
    return {
        "path": str(resolved),
        "sha256": hashlib.sha256(resolved.read_bytes()).hexdigest(),
        "selected_via": "--binary",
    }


def run_components(binary: Path, temp: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    selected = binary_provenance(binary)["path"]
    env = os.environ.copy()
    env["TOKENZERO_BOOT_BENCH_BIN"] = selected
    env["TOKENZERO_EXPAND_BENCH_BIN"] = selected
    compression_text = run_checked(["sh", "scripts/benchmark_tokens.sh", selected], env=env)
    run_checked(
        ["uv", "run", "python", "benchmarks/boot-cost.py", "--label", "candidate"], env=env
    )
    expand_output = temp / "expand.json"
    run_checked(
        ["uv", "run", "python", "crates/tokenzero-mcp/benches/expand_latency.py", "--output", str(expand_output)],
        env=env,
    )
    return compression_from_jsonl(compression_text), load_json(BOOT), load_json(expand_output)


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
    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"],
        cwd=REPO,
        capture_output=True,
        check=True,
    ).stdout.split(b"\0")
    digest = hashlib.sha256(status + diff)
    for encoded in sorted(path for path in untracked if path):
        content = (REPO / os.fsdecode(encoded)).read_bytes()
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return digest.hexdigest()


def comparison_reasons(previous: dict[str, Any], current: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    previous_environment = previous.get("environment", {})
    current_environment = current.get("environment", {})
    for key in ("mode", "machine", "machine_conditions", "os", "python"):
        if key not in previous_environment or key not in current_environment:
            reasons.append(f"environment.{key} is missing")
        elif previous_environment[key] != current_environment[key]:
            reasons.append(
                f"environment.{key} differs: {previous_environment.get(key)!r} != {current_environment.get(key)!r}"
            )
    if previous.get("methodology") != current.get("methodology"):
        reasons.append("benchmark methodology differs or is missing")
    return reasons


def trend(previous: dict[str, Any] | None, current: dict[str, Any]) -> dict[str, Any]:
    if previous is None:
        return {"previous_snapshot": None, "comparable": None, "non_comparable_reasons": [], "deltas": {}, "losses": []}
    reasons = comparison_reasons(previous, current)
    if reasons:
        return {"previous_snapshot": previous["snapshot_id"], "comparable": False,
                "non_comparable_reasons": reasons, "deltas": {}, "losses": []}
    deltas: dict[str, Any] = {
        "headline_savings_pct": round(
            float(current["compression"]["totals"]["savings_pct"])
            - float(previous["compression"]["totals"]["savings_pct"]), 6),
        "boot_tokens": {}, "expand_p50_ms": {},
    }
    losses: list[str] = []
    if deltas["headline_savings_pct"] < 0:
        losses.append(f"headline savings fell {abs(deltas['headline_savings_pct']):.3f} percentage points")
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
    return {"previous_snapshot": previous["snapshot_id"], "comparable": True,
            "non_comparable_reasons": [], "deltas": deltas, "losses": losses}


def render_markdown(snapshot: dict[str, Any]) -> str:
    compression = snapshot["compression"]
    total = compression["totals"]
    lines = [
        "# TokenZero Northstar",
        "",
        f"Snapshot: `{snapshot['snapshot_id']}`",
        f"Commit: `{snapshot['environment']['commit']}`",
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
    report_trend = snapshot["trend"]
    if report_trend["previous_snapshot"] is None:
        lines.append("Initial stored northstar snapshot; no prior trend exists.")
    elif report_trend["comparable"] is False:
        lines.append("Trend is not comparable to the previous stored snapshot:")
        lines.extend(f"- {reason}" for reason in report_trend["non_comparable_reasons"])
    elif report_trend["losses"]:
        lines.extend(f"- {loss}" for loss in report_trend["losses"])
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
    rendered = render_markdown(snapshot)
    with history_path.open("x", encoding="utf-8") as handle:
        handle.write(encoded)
    current_json.write_text(encoded)
    current_markdown.write_text(rendered)
    return history_path, current_json, current_markdown


def build_snapshot(
    compression: dict[str, Any],
    boot_data: dict[str, Any],
    expand_data: dict[str, Any],
    mode: str,
    prior: dict[str, Any] | None,
    binary: dict[str, str],
) -> dict[str, Any]:
    generated = datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")
    commit = read_commit()
    snapshot: dict[str, Any] = {
        "schema": "tokenzero.northstar.v1",
        "snapshot_id": f"{generated.replace(':', '').replace('-', '')}-{commit[:12]}",
        "environment": {
            "generated_at_utc": generated,
            "commit": commit,
            "mode": mode,
            "machine": platform.machine(),
            "machine_conditions": {
                "architecture": platform.machine(),
                "processor": platform.processor(),
                "node": platform.node(),
                "cpu_count": os.cpu_count(),
            },
            "os": platform.platform(),
            "python": platform.python_version(),
            "source_state_sha256": source_state_sha256(),
            "binary": binary,
        },
        "methodology": dict(METHODOLOGY),
        "compression": compression,
        "boot": normalize_boot(boot_data),
        "expand": normalize_expand(expand_data),
        "integrity": {
            "all_compression_rows_retained": True,
            "all_boot_corpora_retained": True,
            "all_expand_size_classes_retained": True,
            "losses_published": True,
            "sources": [
                "scripts/benchmark_tokens.sh stdout",
                str(BOOT.relative_to(REPO)),
                "fresh temporary expand evidence",
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
    if args.reuse_existing:
        parser.error("--reuse-existing is disabled because mixed stale evidence is not authoritative")
    provenance = binary_provenance(args.binary)
    history = args.output_root / "history"
    prior = previous_snapshot(history)
    with tempfile.TemporaryDirectory(prefix="tokenzero-rebaseline-") as raw_temp:
        compression, boot, expand = run_components(Path(provenance["path"]), Path(raw_temp))
        snapshot = build_snapshot(compression, boot, expand, "run-components", prior, provenance)
    paths = write_outputs(snapshot, args.output_root)
    print(json.dumps({"snapshot_id": snapshot["snapshot_id"], "outputs": [str(path) for path in paths]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
