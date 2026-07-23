#!/usr/bin/env python3
"""Analyze Pulse JSONL token spend and rank-frequency distributions.

The analyzer uses only aggregate event metadata. It never emits commands, paths,
queries, session identifiers, or ref values.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Iterator, Mapping, Sequence

CLASSES = ("history", "content", "protocol", "plan", "prose")
REF_RE = re.compile(r"(?:tz|fz|gz)://[^\s\][(){}<>,;\"']+")


@dataclass(frozen=True)
class Fit:
    exponent: float
    confidence_interval_95: tuple[float, float]
    sample_size: int
    method: str

    def as_dict(self) -> dict[str, object]:
        return {
            "exponent": self.exponent,
            "confidence_interval_95": list(self.confidence_interval_95),
            "sample_size": self.sample_size,
            "method": self.method,
        }


def classify_operation(tool_kind: str) -> str:
    """Map a Pulse operation to one mutually exclusive spend class."""
    name = tool_kind.lower()
    if any(term in name for term in ("expand", "recall", "history", "memory")):
        return "history"
    if any(term in name for term in ("read", "find", "grep", "glob", "tree", "map")):
        return "content"
    if any(term in name for term in ("shell", "run", "exec", "plan", "batch")):
        return "plan"
    if any(term in name for term in ("compact", "rewrite", "summar", "ingest", "prose")):
        return "prose"
    return "protocol"


def iter_events(paths: Sequence[Path]) -> Iterator[tuple[dict[str, object] | None, str | None]]:
    for path in paths:
        with path.open(encoding="utf-8", errors="replace") as source:
            for line in source:
                try:
                    value = json.loads(line)
                except json.JSONDecodeError as error:
                    yield None, f"{path.name}:{error.lineno}:{error.colno}"
                    continue
                if not isinstance(value, dict):
                    yield None, f"{path.name}:non-object"
                    continue
                yield value, None


def _strings(value: object) -> Iterable[str]:
    if isinstance(value, str):
        yield value
    elif isinstance(value, Mapping):
        for nested in value.values():
            yield from _strings(nested)
    elif isinstance(value, list):
        for nested in value:
            yield from _strings(nested)


def refs_in_event(event: Mapping[str, object]) -> Iterable[str]:
    for value in _strings(event):
        yield from REF_RE.findall(value)


def log_log_fit(counts: Mapping[str, int]) -> Fit | None:
    """OLS fit of log frequency on log rank with a normal 95% slope CI."""
    frequencies = sorted((count for count in counts.values() if count > 0), reverse=True)
    if len(frequencies) < 3:
        return None
    xs = [math.log(rank) for rank in range(1, len(frequencies) + 1)]
    ys = [math.log(value) for value in frequencies]
    x_bar = sum(xs) / len(xs)
    y_bar = sum(ys) / len(ys)
    sxx = sum((x - x_bar) ** 2 for x in xs)
    slope = sum((x - x_bar) * (y - y_bar) for x, y in zip(xs, ys)) / sxx
    residual = sum(
        (y - (y_bar + slope * (x - x_bar))) ** 2 for x, y in zip(xs, ys)
    )
    standard_error = math.sqrt((residual / (len(xs) - 2)) / sxx)
    exponent = -slope
    margin = 1.96 * standard_error
    return Fit(exponent, (exponent - margin, exponent + margin), len(xs), "log-log OLS")


def hill_fit(counts: Mapping[str, int], tail_size: int | None = None) -> Fit | None:
    """Hill tail-index fit, converted to Zipf rank exponent s = 1 / alpha."""
    frequencies = sorted((count for count in counts.values() if count > 0), reverse=True)
    if len(frequencies) < 4:
        return None
    k = tail_size if tail_size is not None else max(2, math.isqrt(len(frequencies)))
    k = min(max(2, k), len(frequencies) - 1)
    threshold = frequencies[k]
    denominator = sum(math.log(value / threshold) for value in frequencies[:k])
    if denominator <= 0:
        return None
    alpha = k / denominator
    exponent = 1.0 / alpha
    # A log-scale interval stays positive for small tails, unlike a symmetric
    # normal interval. The delta-method standard error of log(alpha) is 1/sqrt(k).
    factor = math.exp(1.96 / math.sqrt(k))
    return Fit(
        exponent,
        (exponent / factor, exponent * factor),
        k,
        "Hill tail index converted with s=1/alpha",
    )


def _fit_report(counts: Mapping[str, int]) -> dict[str, object]:
    hill = hill_fit(counts)
    log_log = log_log_fit(counts)
    return {
        "observations": sum(counts.values()),
        "distinct": len(counts),
        "hill": hill.as_dict() if hill else None,
        "log_log": log_log.as_dict() if log_log else None,
    }


def analyze(paths: Sequence[Path]) -> dict[str, object]:
    spend = Counter({name: 0 for name in CLASSES})
    operations: Counter[str] = Counter()
    refs: Counter[str] = Counter()
    seen_ids: set[str] = set()
    parsed = malformed = duplicates = 0
    for event, error in iter_events(paths):
        if error is not None or event is None:
            malformed += 1
            continue
        event_id = event.get("event_id")
        if isinstance(event_id, str) and event_id in seen_ids:
            duplicates += 1
            continue
        if isinstance(event_id, str):
            seen_ids.add(event_id)
        parsed += 1
        tool_kind = str(event.get("tool_kind") or "unknown")
        visible = event.get("visible_tokens", 0)
        visible_tokens = visible if isinstance(visible, int) and visible >= 0 else 0
        spend[classify_operation(tool_kind)] += visible_tokens
        operations[tool_kind] += 1
        refs.update(refs_in_event(event))

    total_spend = sum(spend.values())
    fractions = {
        name: spend[name] / total_spend if total_spend else 0.0 for name in CLASSES
    }
    return {
        "schema_version": "tokenzero.pulse-spend-zipf.v1",
        "corpus": {
            "files": [path.name for path in paths],
            "file_sha256": {
                path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in paths
            },
            "events_analyzed": parsed,
            "malformed_records": malformed,
            "duplicate_event_ids_skipped": duplicates,
        },
        "spend": {
            "metric": "visible_tokens",
            "total": total_spend,
            "tokens_by_class": dict(spend),
            "fractions": fractions,
            "amdahl_p_by_class": fractions,
        },
        "rank_frequency": {
            "refs": _fit_report(refs),
            "operations": _fit_report(operations),
        },
        "methodology": {
            "classification": {
                "history": "expand, recall, history, and memory operations",
                "content": "read, find, grep, glob, tree, and code-map operations",
                "plan": "shell, run, exec, plan, and batch operations",
                "prose": "compact, rewrite, summarize, ingest, and prose operations",
                "protocol": "all other operations",
            },
            "amdahl": "Each class fraction is p for an optimization confined to that class.",
            "refs": "All tz://, fz://, and gz:// strings in event metadata; values are not emitted.",
            "confidence_intervals": (
                "Asymptotic 95% intervals: normal OLS slope and log-scale Hill; "
                "not bootstrap intervals."
            ),
            "hill_tail_size": "floor(sqrt(distinct items)), with a minimum of two",
        },
        "caveats": [
            "The operation taxonomy is a proxy: Pulse records tool outputs, not full model context composition.",
            "A zero class fraction means no matching operation occurred; it does not prove zero global spend.",
            "Fits assume independent observations and are descriptive, not causal or workload forecasts.",
            "Small distinct-operation and Hill-tail samples can produce wide or unstable intervals.",
        ],
    }


def render_markdown(report: Mapping[str, object]) -> str:
    """Render a compact, aggregate-only Markdown report."""
    corpus = report["corpus"]
    spend = report["spend"]
    rank = report["rank_frequency"]
    assert isinstance(corpus, Mapping) and isinstance(spend, Mapping)
    assert isinstance(rank, Mapping)
    fractions = spend["fractions"]
    assert isinstance(fractions, Mapping)
    lines = [
        "# Pulse spend and rank-frequency report",
        "",
        f"Corpus: {corpus['events_analyzed']:,} valid events from {', '.join(corpus['files'])}; "
        f"{corpus['malformed_records']} malformed and {corpus['duplicate_event_ids_skipped']} duplicates skipped.",
        "",
        "## Spend fractions and Amdahl p",
        "",
        "The metric is visible tokens. For an optimization confined to one class, its fraction is Amdahl p.",
        "",
        "| Class | Tokens | Fraction / p |",
        "|---|---:|---:|",
    ]
    tokens = spend["tokens_by_class"]
    assert isinstance(tokens, Mapping)
    for name in CLASSES:
        lines.append(f"| {name} | {tokens[name]:,} | {fractions[name]:.6f} |")
    lines.extend(
        [
            "",
            "## Zipf exponent fits",
            "",
            "| Kind | Method | s | 95% CI | n |",
            "|---|---|---:|---:|---:|",
        ]
    )
    for kind in ("refs", "operations"):
        item = rank[kind]
        assert isinstance(item, Mapping)
        for method in ("hill", "log_log"):
            fit = item[method]
            if not isinstance(fit, Mapping):
                lines.append(f"| {kind} | {method} | unavailable | unavailable | 0 |")
                continue
            low, high = fit["confidence_interval_95"]
            lines.append(
                f"| {kind} | {method} | {fit['exponent']:.6f} | "
                f"[{low:.6f}, {high:.6f}] | {fit['sample_size']} |"
            )
    lines.extend(["", "## Caveats", ""])
    for caveat in report["caveats"]:
        lines.append(f"- {caveat}")
    event_count = corpus["events_analyzed"]
    if isinstance(event_count, int) and event_count < 20_000:
        lines.append(
            f"- This corpus has {event_count:,} events, below the approximate "
            "20,000-event target; no events were synthesized."
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", type=Path, help="Pulse JSONL files")
    parser.add_argument("--output", type=Path, help="Write JSON report to this path")
    parser.add_argument("--markdown-output", type=Path, help="Write Markdown report")
    args = parser.parse_args()
    report = json.dumps(analyze(args.inputs), indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(report, encoding="utf-8")
    else:
        print(report, end="")
    if args.markdown_output:
        args.markdown_output.write_text(
            render_markdown(json.loads(report)), encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
