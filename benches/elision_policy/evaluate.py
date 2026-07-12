#!/usr/bin/env python3
"""Fit and evaluate a transparent per-content-class elision policy.

Only explicit recovery observations are labels.  Repetition, cache hits, and
compression wins are deliberately not treated as evidence that an agent asked
for elided bytes.  This makes an evidence-sparse result a valid negative result
instead of manufacturing training labels.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import subprocess
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable

SCHEMA = "tokenzero.elision-policy-evaluation.v1"
STATIC_PREVIEW_TOKENS = 256
TRAIN_FRACTION = 0.70
QUANTILE = 0.75
MIN_TRAIN_PER_CLASS = 4
MIN_PROMOTION_HOLDOUT = 30
MAX_CLASS_REGRESSION_PCT = 2.0


def walk(value: Any, path: tuple[str, ...] = ()) -> Iterable[tuple[tuple[str, ...], dict[str, Any]]]:
    if isinstance(value, dict):
        yield path, value
        for key, child in value.items():
            yield from walk(child, path + (str(key),))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk(child, path + (str(index),))


def canonical_class(row: dict[str, Any]) -> str | None:
    value = next(
        (
            row[key]
            for key in ("content_class", "operation", "tool", "family")
            if isinstance(row.get(key), str) and row[key].strip()
        ),
        None,
    )
    if value is None:
        return None
    value = value.strip().lower().replace("_", "-")
    aliases = {
        "find": "search",
        "grep": "search",
        "list": "tree",
        "glob": "tree",
        "inventory": "tree",
        "run": "shell",
        "command": "shell",
    }
    return aliases.get(value, value)


def is_nonnegative_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0


def discover_inputs(root: Path, output: Path) -> list[Path]:
    candidates = set(root.glob("benches/**/*.json"))
    candidates.update(root.glob("crates/**/benches/**/*.json"))
    return sorted(
        path
        for path in candidates
        if path.is_file() and path.resolve() != output.resolve()
    )


def load_corpus(root: Path, output: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    labels: list[dict[str, Any]] = []
    replay: list[dict[str, Any]] = []
    inputs: list[dict[str, Any]] = []
    for source in discover_inputs(root, output):
        raw = source.read_bytes()
        try:
            document = json.loads(raw)
        except json.JSONDecodeError:
            continue
        relative = source.relative_to(root).as_posix()
        inputs.append(
            {
                "path": relative,
                "sha256": hashlib.sha256(raw).hexdigest(),
                "bytes": len(raw),
            }
        )
        schema = document.get("schema") or document.get("schema_version")
        for node_path, row in walk(document):
            content_class = canonical_class(row)
            if content_class == "expand":
                # This is the recovery call, not the original serve decision.
                continue
            if (
                content_class is not None
                and is_nonnegative_number(row.get("raw_tokens"))
                and is_nonnegative_number(row.get("visible_tokens"))
                and is_nonnegative_number(row.get("recovery_tokens"))
            ):
                labels.append(
                    {
                        "content_class": content_class,
                        "raw_tokens": int(row["raw_tokens"]),
                        "visible_tokens": int(row["visible_tokens"]),
                        "recovery_tokens": int(row["recovery_tokens"]),
                        "expanded": row["recovery_tokens"] > 0,
                        "remaining_turns": max(1, int(row.get("remaining_turns", 1))),
                        "source": relative,
                        "source_path": ".".join(node_path),
                    }
                )
            if (
                schema == "tokenzero.session-delta-headline.v1"
                and "samples" in node_path
                and content_class is not None
                and is_nonnegative_number(row.get("baseline_bytes"))
                and is_nonnegative_number(row.get("raw_bytes"))
                and is_nonnegative_number(row.get("turn"))
            ):
                turns = int(document.get("fixture", {}).get("turns", row["turn"]))
                replay.append(
                    {
                        "content_class": content_class,
                        "baseline_bytes": int(row["baseline_bytes"]),
                        "raw_bytes": int(row["raw_bytes"]),
                        "remaining_turns": max(1, turns - int(row["turn"]) + 1),
                        "source": relative,
                        "source_path": ".".join(node_path),
                    }
                )
    labels.sort(key=lambda row: (row["content_class"], row["source"], row["source_path"]))
    replay.sort(key=lambda row: (row["content_class"], row["source"], row["source_path"]))
    return labels, replay, inputs


def split_by_class(rows: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[row["content_class"]].append(row)
    train: list[dict[str, Any]] = []
    holdout: list[dict[str, Any]] = []
    for content_class in sorted(grouped):
        class_rows = grouped[content_class]
        if len(class_rows) == 1:
            train.extend(class_rows)
            continue
        cut = min(len(class_rows) - 1, max(1, math.floor(len(class_rows) * TRAIN_FRACTION)))
        train.extend(class_rows[:cut])
        holdout.extend(class_rows[cut:])
    return train, holdout


def nearest_rank(values: list[int], quantile: float) -> int:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(quantile * len(ordered)) - 1)]


def fit_predictor(train: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in train:
        grouped[row["content_class"]].append(row)
    table: dict[str, dict[str, Any]] = {}
    for content_class in sorted(grouped):
        rows = grouped[content_class]
        expanded = [row for row in rows if row["expanded"]]
        rate = len(expanded) / len(rows)
        learned = len(rows) >= MIN_TRAIN_PER_CLASS
        if not learned:
            preview = STATIC_PREVIEW_TOKENS
            reason = "insufficient-training-observations"
        elif not expanded:
            preview = 0
            reason = "no-recorded-expansions"
        elif len(expanded) == len(rows):
            preview = max(row["raw_tokens"] for row in expanded)
            reason = "all-recorded-rows-expanded"
        else:
            preview = nearest_rank([row["raw_tokens"] for row in expanded], QUANTILE)
            reason = "expanded-length-q75"
        table[content_class] = {
            "training_observations": len(rows),
            "recorded_expansions": len(expanded),
            "expansion_rate": round(rate, 6),
            "learned": learned,
            "preview_tokens": preview,
            "reason": reason,
        }
    return table


def policy_cost(row: dict[str, Any], preview_tokens: int) -> tuple[int, bool]:
    raw = row["raw_tokens"]
    visible = min(raw, preview_tokens)
    missed = bool(row["expanded"] and visible < raw)
    recovery = raw if missed else 0
    return visible * row["remaining_turns"] + recovery, missed


def evaluate_holdout(rows: list[dict[str, Any]], table: dict[str, dict[str, Any]]) -> dict[str, Any]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[row["content_class"]].append(row)
    per_class: dict[str, Any] = {}
    total_static = total_candidate = total_static_misses = total_candidate_misses = 0
    for content_class in sorted(grouped):
        class_rows = grouped[content_class]
        learned = table.get(content_class, {}).get("learned", False)
        candidate_preview = table.get(content_class, {}).get("preview_tokens", STATIC_PREVIEW_TOKENS) if learned else STATIC_PREVIEW_TOKENS
        static_cost = candidate_cost = static_misses = candidate_misses = 0
        for row in class_rows:
            cost, missed = policy_cost(row, STATIC_PREVIEW_TOKENS)
            static_cost += cost
            static_misses += int(missed)
            cost, missed = policy_cost(row, candidate_preview)
            candidate_cost += cost
            candidate_misses += int(missed)
        delta = candidate_cost - static_cost
        regression_pct = (100.0 * delta / static_cost) if static_cost else 0.0
        per_class[content_class] = {
            "holdout_observations": len(class_rows),
            "learned_policy_applied": learned,
            "outcome": "win" if delta < 0 else ("loss" if delta > 0 else "tie"),
            "static_ledger_token_turns": static_cost,
            "candidate_ledger_token_turns": candidate_cost,
            "candidate_minus_static_token_turns": delta,
            "candidate_vs_static_pct": round(regression_pct, 6),
            "static_expansion_misses": static_misses,
            "candidate_expansion_misses": candidate_misses,
            "candidate_expansion_miss_rate": round(candidate_misses / len(class_rows), 6),
        }
        total_static += static_cost
        total_candidate += candidate_cost
        total_static_misses += static_misses
        total_candidate_misses += candidate_misses
    count = len(rows)
    return {
        "observations": count,
        "per_class": per_class,
        "overall": {
            "static_ledger_token_turns": total_static,
            "candidate_ledger_token_turns": total_candidate,
            "candidate_minus_static_token_turns": total_candidate - total_static,
            "static_expansion_miss_rate": None if not count else round(total_static_misses / count, 6),
            "candidate_expansion_miss_rate": None if not count else round(total_candidate_misses / count, 6),
        },
    }


def replay_fallback(rows: list[dict[str, Any]], table: dict[str, dict[str, Any]]) -> dict[str, Any]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[row["content_class"]].append(row)
    per_class: dict[str, Any] = {}
    total = 0
    for content_class in sorted(grouped):
        class_rows = grouped[content_class]
        mass = sum(row["baseline_bytes"] * row["remaining_turns"] for row in class_rows)
        learned = table.get(content_class, {}).get("learned", False)
        per_class[content_class] = {
            "recorded_samples": len(class_rows),
            "raw_byte_turns": sum(row["raw_bytes"] * row["remaining_turns"] for row in class_rows),
            "static_visible_byte_turns": mass,
            "candidate_visible_byte_turns": None if learned else mass,
            "candidate_minus_static_byte_turns": None if learned else 0,
            "candidate_status": "not-comparable-token-vs-byte-units" if learned else "static-fallback",
            "outcome": "not-evaluated" if learned else "tie",
        }
        total += mass
    return {
        "observations": len(rows),
        "per_class": per_class,
        "static_visible_byte_turns": total,
        "candidate_visible_byte_turns": total if not any(v.get("learned") for v in table.values()) else None,
        "note": "Recorded session-delta evidence is byte-accounted and carries no expansion labels. It verifies fallback equality only; it is not converted to tokens.",
    }


def git_commit(root: Path) -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True, capture_output=True, check=False
    )
    return result.stdout.strip() or None


def build_report(root: Path, output: Path) -> dict[str, Any]:
    labels, replay, inputs = load_corpus(root, output)
    train, holdout = split_by_class(labels)
    table = fit_predictor(train)
    for content_class in sorted({row["content_class"] for row in replay}):
        table.setdefault(
            content_class,
            {
                "training_observations": 0,
                "recorded_expansions": 0,
                "expansion_rate": None,
                "learned": False,
                "preview_tokens": STATIC_PREVIEW_TOKENS,
                "reason": "no-explicit-expansion-labels",
            },
        )
    evaluation = evaluate_holdout(holdout, table)
    replay_report = replay_fallback(replay, table)
    learned_classes = [name for name, row in table.items() if row["learned"]]
    class_regressions = [row["candidate_vs_static_pct"] for row in evaluation["per_class"].values()]
    overall = evaluation["overall"]
    robust_win = bool(
        len(holdout) >= MIN_PROMOTION_HOLDOUT
        and learned_classes
        and overall["candidate_ledger_token_turns"] < overall["static_ledger_token_turns"]
        and (not class_regressions or max(class_regressions) <= MAX_CLASS_REGRESSION_PCT)
        and overall["candidate_expansion_miss_rate"] is not None
        and overall["candidate_expansion_miss_rate"] <= overall["static_expansion_miss_rate"]
    )
    return {
        "schema": SCHEMA,
        "environment": {
            "commit": git_commit(root),
            "python": platform.python_version(),
            "os": platform.platform(),
            "machine": platform.machine(),
            "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        },
        "inputs": inputs,
        "methodology": {
            "feature": "explicit content_class, operation, tool, or family; normalized aliases only",
            "label": "per-call recovery_tokens > 0; expand calls themselves are excluded",
            "rejected_proxies": ["repeat", "cache_hit", "compression win", "latency win"],
            "split": "stable source/path order within class; first 70% train, remaining 30% holdout; singleton has no holdout",
            "predictor": "per-class table: zero when no training row expands, max expanded length when all expand, otherwise nearest-rank q75 of expanded raw_tokens",
            "sparse_class_fallback": f"static {STATIC_PREVIEW_TOKENS}-token preview until at least {MIN_TRAIN_PER_CLASS} training rows",
            "ledger": "visible preview tokens multiplied by remaining turns, plus one full raw-token recovery on an expansion miss",
            "promotion": f"at least {MIN_PROMOTION_HOLDOUT} labeled holdout rows, net ledger win, no class above {MAX_CLASS_REGRESSION_PCT}% regression, and no expansion-miss-rate regression",
        },
        "corpus": {
            "evidence_files": len(inputs),
            "explicitly_labeled_rows": len(labels),
            "training_rows": len(train),
            "holdout_rows": len(holdout),
            "recorded_session_replay_rows": len(replay),
        },
        "predictor": {
            "static_preview_tokens": STATIC_PREVIEW_TOKENS,
            "quantile": QUANTILE,
            "minimum_training_rows_per_class": MIN_TRAIN_PER_CLASS,
            "per_class": table,
        },
        "holdout_evaluation": evaluation,
        "recorded_session_replay": replay_report,
        "findings": {
            "robust_win": robust_win,
            "adaptive_crossover_decision": "promote-behind-default-off-flag" if robust_win else "do-not-implement",
            "learned_classes": learned_classes,
            "negative_result_reasons": [] if robust_win else [
                "recorded evidence contains no eligible per-call raw_tokens/visible_tokens/recovery_tokens rows with a source content class" if not labels else "promotion criteria were not all met",
                "expansion-miss rate is unmeasurable without labeled holdout rows" if not holdout else "holdout did not establish a robust win",
            ],
        },
        "losses_and_limits": [
            "No synthetic labels are introduced when recorded evidence lacks expansion outcomes.",
            "Byte-accounted session-delta rows are not converted to tokens; only fallback equality is reported for them.",
            "recovery_tokens can include internally charged recovery, so future corpora should record an explicit agent_requested_expand field.",
            "This evaluation cannot justify adaptive product behavior when robust_win is false.",
        ],
    }


def self_test() -> None:
    rows = [
        {"content_class": "read", "raw_tokens": size, "visible_tokens": 10, "recovery_tokens": size if expanded else 0, "expanded": expanded, "remaining_turns": 2, "source": "x", "source_path": str(index)}
        for index, (size, expanded) in enumerate([(80, False), (100, True), (120, True), (140, False), (160, True), (180, False), (200, True), (220, False)])
    ]
    train, holdout = split_by_class(rows)
    table = fit_predictor(train)
    assert len(train) == 5 and len(holdout) == 3
    assert table["read"]["learned"] is True
    assert table["read"]["preview_tokens"] == 160
    evaluation = evaluate_holdout(holdout, table)
    assert evaluation["observations"] == 3
    assert evaluation["per_class"]["read"]["candidate_ledger_token_turns"] > 0
    assert nearest_rank([1, 2, 3, 4], 0.75) == 3
    edge_rows = [
        {"content_class": content_class, "raw_tokens": 100 + index, "expanded": expanded}
        for content_class, expanded in (("never", False), ("always", True))
        for index in range(MIN_TRAIN_PER_CLASS)
    ]
    edge_table = fit_predictor(edge_rows)
    assert edge_table["never"]["preview_tokens"] == 0
    assert edge_table["always"]["preview_tokens"] == 100 + MIN_TRAIN_PER_CLASS - 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, default=Path(__file__).with_name("evidence.json"))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("elision policy self-test: ok")
        return 0
    root = args.root.resolve()
    output = args.output.resolve()
    report = build_report(root, output)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    os.replace(temporary, output)
    print(json.dumps(report["findings"], sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
