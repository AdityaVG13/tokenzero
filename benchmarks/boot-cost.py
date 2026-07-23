#!/usr/bin/env python3
"""Measure and gate manifest+delta boot cost across repository sizes."""
from __future__ import annotations

import argparse
import json
import os
import tempfile
from pathlib import Path
from typing import Any

try:
    from benchmarks import harness as H
except ModuleNotFoundError:
    import harness as H

REPO = H.REPO
BIN = Path(os.environ.get("TOKENZERO_BOOT_BENCH_BIN", REPO / "target/debug/tokenzero"))
EVIDENCE = Path(__file__).with_suffix("")
BASELINE = EVIDENCE / "baseline.json"
SYNTHETIC_CORPORA = {"synthetic-23k": 23_000, "synthetic-100k": 100_000}
COUNT_EXCLUDES = {".git", "target", ".zerostack"}
COMPONENT_NAMES = ("manifest", "delta", "toc_working_set", "other")


def masked_argv(arguments: list[str], root: Path, cache: Path) -> list[str]:
    return [
        "<root>" if value == str(root) else "<temp-cache>" if value == str(cache) else value
        for value in arguments
    ]


def measure(label: str, root: Path, cache_dir: Path) -> dict[str, object]:
    cache = cache_dir / "recovery-cache.json"
    command = [
        str(BIN),
        "session-open",
        "--root",
        str(root),
        "--cache-path",
        str(cache),
        "--json",
    ]
    initialized = H.run_json(command)
    recorded = H.run_json(command)
    boot = recorded["raw_json"]
    components = boot["telemetry"]
    component_sum = sum(int(components[name]) for name in COMPONENT_NAMES)
    total = int(components["total"])
    if component_sum != total:
        raise RuntimeError(f"{label} components {component_sum} != total {total}")
    if boot.get("mode") != "manifest_delta":
        raise RuntimeError(f"{label} did not use manifest+delta boot: {boot}")
    if boot.get("demand_paging", {}).get("working_set_loaded") is not False:
        raise RuntimeError(f"{label} eagerly loaded the working set: {boot}")
    initialized["argv"] = masked_argv(command, root, cache)
    recorded["argv"] = masked_argv(command, root, cache)
    for blob in (initialized, recorded):
        blob.pop("returncode", None)
        blob.pop("stdout", None)
    return {
        "label": label,
        "root": str(root),
        "file_count": H.file_count(root, COUNT_EXCLUDES),
        "file_count_excludes": sorted(COUNT_EXCLUDES),
        "metadata_initialization": initialized,
        "recorded_manifest_delta_boot": recorded,
        "boot_tokens": total,
        "components": components,
        "component_sum": component_sum,
    }


def _attributed_component(
    actual: dict[str, Any], baseline: dict[str, Any]
) -> tuple[str, int]:
    deltas = {
        name: int(actual.get(name, 0)) - int(baseline.get(name, 0))
        for name in COMPONENT_NAMES
    }
    return max(deltas.items(), key=lambda item: (item[1], item[0]))


def evaluate_gate(
    corpora: list[dict[str, object]], baseline: dict[str, Any]
) -> dict[str, object]:
    by_label = {str(corpus["label"]): corpus for corpus in corpora}
    thresholds = baseline["thresholds"]
    small_label = str(thresholds["small_corpus"])
    large_label = str(thresholds["large_corpus"])
    missing = [label for label in (small_label, large_label) if label not in by_label]
    if missing:
        raise RuntimeError(f"boot-cost gate missing corpora: {', '.join(missing)}")

    max_tokens = int(thresholds["max_visible_boot_tokens_exclusive"])
    baseline_components = baseline["components"]
    for label in (small_label, large_label):
        corpus = by_label[label]
        actual = corpus["components"]
        total = int(corpus["boot_tokens"])
        if total >= max_tokens:
            component, delta = _attributed_component(actual, baseline_components[label])
            raise RuntimeError(
                f"boot-cost lock failed: corpus={label} visible_tokens={total} "
                f"limit_exclusive={max_tokens} component={component} "
                f"component_tokens={actual[component]} baseline_delta={delta:+d}"
            )

    small = by_label[small_label]
    large = by_label[large_label]
    growth = int(large["boot_tokens"]) - int(small["boot_tokens"])
    epsilon = int(thresholds["max_repo_size_growth_tokens"])
    if growth > epsilon:
        component_growth = {
            name: int(large["components"][name]) - int(small["components"][name])
            for name in COMPONENT_NAMES
        }
        component, delta = max(
            component_growth.items(), key=lambda item: (item[1], item[0])
        )
        raise RuntimeError(
            f"boot-cost growth lock failed: {small_label}->{large_label} "
            f"growth_tokens={growth} epsilon={epsilon} component={component} "
            f"component_growth={delta:+d}"
        )
    return {
        "max_visible_boot_tokens_exclusive": max_tokens,
        "repo_size_growth_epsilon_tokens": epsilon,
        "measured_repo_size_growth_tokens": growth,
        "small_corpus": small_label,
        "large_corpus": large_label,
        "component_attribution": True,
        "baseline": str(BASELINE.relative_to(REPO)),
        "all_passed": True,
    }


def _write_rebaseline(corpora: list[dict[str, object]], baseline: dict[str, Any]) -> None:
    labels = (
        str(baseline["thresholds"]["small_corpus"]),
        str(baseline["thresholds"]["large_corpus"]),
    )
    by_label = {str(corpus["label"]): corpus for corpus in corpora}
    baseline["components"] = {label: by_label[label]["components"] for label in labels}
    BASELINE.write_text(json.dumps(baseline, indent=2) + "\n")


def run(label: str, *, rebaseline: bool = False) -> Path:
    if not BIN.is_file():
        raise SystemExit(
            f"benchmark binary missing: {BIN}; build or select it with TOKENZERO_BOOT_BENCH_BIN"
        )
    baseline = json.loads(BASELINE.read_text())
    with H.heavy_guard(f"python3 benchmarks/boot-cost.py --label {label}"):
        with tempfile.TemporaryDirectory(prefix="tokenzero-boot-cost-") as raw:
            tmp = Path(raw)
            corpora = [measure("repository", REPO, tmp / "repository-cache")]
            for name, files in SYNTHETIC_CORPORA.items():
                synthetic = tmp / name
                synthetic.mkdir()
                H.synthetic_tree(synthetic, files)
                corpora.append(measure(name, synthetic, tmp / f"{name}-cache"))
            if rebaseline:
                _write_rebaseline(corpora, baseline)
            gate = evaluate_gate(corpora, baseline)
            result = {
                "schema": "tokenzero.boot-cost.v2",
                "environment": H.capture_environment(
                    BIN,
                    f"python3 benchmarks/boot-cost.py --label {label}",
                    extra={
                        "synthetic_generator": "deterministic 8-byte text files sharded 1000 per directory"
                    },
                ),
                "corpora": corpora,
                "assertions": {
                    **gate,
                    "component_totals_match": True,
                    "working_set_demand_paged": True,
                },
            }
            EVIDENCE.mkdir(parents=True, exist_ok=True)
            destination = EVIDENCE / f"{label}.json"
            destination.write_text(json.dumps(result, indent=2) + "\n")
            return destination


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", default="candidate")
    parser.add_argument(
        "--rebaseline",
        action="store_true",
        help="refresh tracked component attribution; commit baseline.json explicitly",
    )
    args = parser.parse_args()
    print(run(args.label, rebaseline=args.rebaseline).relative_to(REPO))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
