#!/usr/bin/env python3
"""Measure ref framing and boundary-aware packing on sub-1KiB payloads.

TokenZero's dependency-free lexical counter is always measured. When tiktoken
is installed, cl100k_base and o200k_base are also measured; unavailable
encodings are disclosed in evidence instead of replaced by estimates.
"""
from __future__ import annotations

import hashlib
import importlib.metadata
import json
import platform
import subprocess
from collections import defaultdict
from pathlib import Path
from typing import Callable

REPO = Path(__file__).resolve().parents[3]
OUT = Path(__file__).with_suffix("") / "evidence.json"
SIZES = (32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 960)
KINDS = ("ack", "preview", "ref_list")
VARIANTS = 8


def tokenzero_lexical(text: str) -> int:
    tokens = 0
    in_token = False
    for char in text:
        if char.isascii() and (char.isalnum() or char == "_"):
            if not in_token:
                tokens += 1
                in_token = True
        elif char.isspace():
            in_token = False
        else:
            in_token = False
            tokens += 1
    return tokens


def tokenizer_registry() -> tuple[dict[str, Callable[[str], int]], dict]:
    tokenizers: dict[str, Callable[[str], int]] = {"tokenzero_lexical_v1": tokenzero_lexical}
    metadata = {"tokenzero_lexical_v1": {"implementation": "exact Python port of tokenzero_core::count_tokens ASCII semantics", "version": "1"}}
    unavailable = []
    try:
        import tiktoken
    except ImportError:
        unavailable.extend(["cl100k_base", "o200k_base"])
    else:
        version = importlib.metadata.version("tiktoken")
        for encoding_name in ("cl100k_base", "o200k_base"):
            encoding = tiktoken.get_encoding(encoding_name)
            tokenizers[encoding_name] = lambda text, encoding=encoding: len(encoding.encode(text))
            metadata[encoding_name] = {"implementation": "tiktoken", "version": version}
    return tokenizers, {"measured": metadata, "unavailable": unavailable}


def make_payload(kind: str, size: int, variant: int) -> str:
    patterns = {
        "ack": f"ok operation={variant} status=stored ref_ready=true ",
        "preview": f"src/module_{variant}.rs:{variant + 10}: fn compact_payload_{variant}() {{ value += 1; }} ",
        "ref_list": f"tz://blob/{hashlib.sha256(f'{variant}'.encode()).hexdigest()} ",
    }
    pattern = patterns[kind]
    value = (pattern * (size // len(pattern) + 2))[:size]
    assert len(value.encode()) == size
    return value


def compact_json(value: dict) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n"


def inline_wire(kind: str, payload: str) -> str:
    return compact_json({"result": {"kind": kind, "status": "ok", "visible": payload}})


def ref_wire(kind: str, payload: str) -> str:
    digest = hashlib.sha256(payload.encode()).hexdigest()
    return compact_json({"result": {"kind": kind, "raw_bytes": len(payload.encode()), "ref": f"tz://blob/{digest}", "status": "ok"}})


def environment() -> dict:
    diff = subprocess.run(["git", "diff", "--binary"], cwd=REPO, capture_output=True, check=True).stdout
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True, check=True).stdout.strip()
    return {
        "commit": commit,
        "machine": platform.machine(),
        "os": platform.platform(),
        "python": platform.python_version(),
        "source_diff_sha256": hashlib.sha256(diff).hexdigest(),
    }


def pct_saved(baseline: int, candidate: int) -> float:
    return round(100 * (baseline - candidate) / baseline, 3)


def summarize(rows: list[dict]) -> dict:
    inline = sum(row["inline_tokens"] for row in rows)
    forced = sum(row["ref_tokens"] for row in rows)
    boundary = sum(min(row["inline_tokens"], row["ref_tokens"]) for row in rows)
    inline_bytes = sum(row["inline_bytes"] for row in rows)
    ref_bytes = sum(row["ref_bytes"] for row in rows)
    return {
        "samples": len(rows),
        "inline_tokens": inline,
        "forced_ref_tokens": forced,
        "boundary_aware_tokens": boundary,
        "forced_ref_overhead_tokens": forced - inline,
        "boundary_aware_savings_tokens": inline - boundary,
        "forced_ref_savings_pct": pct_saved(inline, forced),
        "boundary_aware_savings_pct": pct_saved(inline, boundary),
        "boundary_aware_compaction_ratio": round(boundary / inline, 6),
        "inline_wire_bytes": inline_bytes,
        "ref_wire_bytes": ref_bytes,
        "ref_wire_savings_pct": pct_saved(inline_bytes, ref_bytes),
    }


def crossover(rows: list[dict]) -> dict:
    by_size: dict[int, list[dict]] = defaultdict(list)
    for row in rows:
        by_size[row["payload_bytes"]].append(row)
    points = []
    for size in sorted(by_size):
        selected = by_size[size]
        inline = sum(row["inline_tokens"] for row in selected)
        refs = sum(row["ref_tokens"] for row in selected)
        points.append({
            "payload_bytes": size,
            "samples": len(selected),
            "inline_tokens": inline,
            "ref_tokens": refs,
            "ref_minus_inline_tokens": refs - inline,
            "ref_cheaper": refs < inline,
        })
    first = next((point for point in points if point["ref_cheaper"]), None)
    previous = None if first is None else next((point for point in reversed(points) if point["payload_bytes"] < first["payload_bytes"]), None)
    return {"first_measured_ref_win": first, "previous_measured_point": previous, "points": points}


def main() -> None:
    tokenizers, tokenizer_metadata = tokenizer_registry()
    corpus = [
        {"kind": kind, "payload_bytes": size, "variant": variant, "payload": make_payload(kind, size, variant)}
        for kind in KINDS
        for size in SIZES
        for variant in range(VARIANTS)
    ]
    results = {}
    for tokenizer_name, count in tokenizers.items():
        rows = []
        for sample in corpus:
            inline = inline_wire(sample["kind"], sample["payload"])
            ref = ref_wire(sample["kind"], sample["payload"])
            rows.append({
                "kind": sample["kind"],
                "payload_bytes": sample["payload_bytes"],
                "variant": sample["variant"],
                "inline_bytes": len(inline.encode()),
                "ref_bytes": len(ref.encode()),
                "inline_tokens": count(inline),
                "ref_tokens": count(ref),
            })
        per_class = {kind: summarize([row for row in rows if row["kind"] == kind]) for kind in KINDS}
        per_class_crossover = {kind: crossover([row for row in rows if row["kind"] == kind]) for kind in KINDS}
        forced_losses = [row for row in rows if row["ref_tokens"] > row["inline_tokens"]]
        ties = [row for row in rows if row["ref_tokens"] == row["inline_tokens"]]
        results[tokenizer_name] = {
            "overall": summarize(rows),
            "per_class": per_class,
            "crossover": per_class_crossover,
            "losses": {
                "forced_ref_losing_samples": len(forced_losses),
                "forced_ref_tied_samples": len(ties),
                "forced_ref_excess_tokens": sum(row["ref_tokens"] - row["inline_tokens"] for row in forced_losses),
                "boundary_aware_losing_samples": 0,
                "explanation": "Forced refs lose below the tokenizer-specific crossover. Boundary-aware packing selects the lower measured token count, so those payloads remain inline; this is selection, not a claim that ref framing is free.",
            },
            "samples": rows,
        }
    evidence = {
        "schema": "tokenzero.small-payload-packing.v1",
        "environment": environment(),
        "tokenizers": tokenizer_metadata,
        "methodology": {
            "corpus": "264 deterministic ASCII payloads: 3 classes x 11 byte sizes x 8 variants; every payload is below 1 KiB",
            "baseline": "compact JSON envelope with payload inline",
            "forced_ref": "compact JSON envelope with SHA-256 tz://blob ref and raw byte count",
            "boundary_aware": "per sample, emit whichever complete envelope has fewer tokens under that tokenizer",
            "ratio_direction": "candidate tokens / inline tokens; below 1 is better",
            "crossover": "first measured payload size whose aggregate ref envelope costs fewer tokens than aggregate inline envelope; previous sampled size is quoted",
            "integrity": "all generated classes, sizes, and variants retained; unavailable tokenizer libraries disclosed, never estimated",
        },
        "fixture": {"classes": list(KINDS), "payload_sizes_bytes": list(SIZES), "variants_per_class_size": VARIANTS, "samples": len(corpus), "max_payload_bytes": max(SIZES)},
        "results": results,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
