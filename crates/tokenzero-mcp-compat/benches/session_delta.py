#!/usr/bin/env python3
"""Measure multi-turn headline compression with session-delta suppression.

The baseline and candidate traverse the same deterministic corpus in the same
order. The candidate differs only by replacing a compact capsule already seen
in the session with a recovery-ref marker plus delta telemetry.
"""
from __future__ import annotations

import hashlib
import json
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
import sys
sys.path.insert(0, str(REPO / "benchmarks"))
from bench_common import environment as bench_environment, pct_saved, write_json
OUT = Path(__file__).with_suffix("") / "evidence.json"
TURNS = 12
OP_SPECS = {
    "read": {"lines": 1260, "cadence": None},
    "search": {"lines": 660, "cadence": 6},
    "tree": {"lines": 960, "cadence": None},
    "git": {"lines": 540, "cadence": 8},
}


def payload(op: str, turn: int) -> str:
    spec = OP_SPECS[op]
    cadence = spec["cadence"]
    revision = 0 if cadence is None else (turn - 1) // cadence
    if op == "read":
        return "\n".join(
            f"src/session_{revision}.rs:{line:04}: pub fn handler_{line % 23}() -> Result<(), Error> {{ trace!(\"session delta fixture {line:04}\"); }}"
            for line in range(1, spec["lines"] + 1)
        )
    if op == "search":
        return "\n".join(
            f"src/module_{line % 17}.rs:{line * 7}: match session_delta_revision_{revision} with recovery_ref_{line:04}"
            for line in range(1, spec["lines"] + 1)
        )
    if op == "tree":
        return "\n".join(
            f"crates/component_{line % 29}/src/{'tests' if line % 5 == 0 else 'lib'}/node_{line:04}.rs"
            for line in range(1, spec["lines"] + 1)
        )
    return "\n".join(
        f"{line:04} {hashlib.sha256(f'{revision}:{line}'.encode()).hexdigest()[:12]} change module_{line % 19} session delta benchmark"
        for line in range(1, spec["lines"] + 1)
    )


def ref_for(text: str) -> str:
    return "tz://blob/" + hashlib.sha256(text.encode()).hexdigest()


def compact_capsule(op: str, text: str, ref: str) -> str:
    lines = text.splitlines()
    kept = lines[:4] + [f"... {len(lines) - 7} lines omitted; expand {ref} ..."] + lines[-3:]
    return f"[{op}: {len(text.encode())} raw bytes, {len(lines)} lines]\n" + "\n".join(kept)


def envelope(request_id: int, op: str, visible: str, ref: str, telemetry: dict | None = None) -> dict:
    result = {
        "refs": [{"kind": "blob", "ref": ref}],
        "status": "ok",
        "tool": op,
        "visible": {"text": visible},
    }
    if telemetry is not None:
        result["telemetry"] = telemetry
    return {"id": request_id, "jsonrpc": "2.0", "result": result}


def wire_bytes(value: dict) -> int:
    return len((json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode())


def main() -> None:
    seen: set[str] = set()
    rows = []
    request_id = 0
    for turn in range(1, TURNS + 1):
        for op in OP_SPECS:
            request_id += 1
            text = payload(op, turn)
            ref = ref_for(text)
            capsule = compact_capsule(op, text, ref)
            raw = wire_bytes(envelope(request_id, op, text, ref))
            baseline = wire_bytes(envelope(request_id, op, capsule, ref))
            repeated = ref in seen
            if repeated:
                visible = f"+{ref} (already seen; expand for exact bytes)"
                strategy = "seen_set_dedup"
            else:
                visible = capsule
                strategy = "full_compact_capsule"
                seen.add(ref)
            telemetry = {
                "output_strategy": strategy,
                "session_delta": {
                    "full_compact_bytes": len(capsule.encode()),
                    "repeat": repeated,
                    "saved_visible_bytes": len(capsule.encode()) - len(visible.encode()),
                    "turn": turn,
                },
            }
            delta = wire_bytes(envelope(request_id, op, visible, ref, telemetry))
            rows.append({
                "turn": turn,
                "operation": op,
                "ref": ref,
                "repeat": repeated,
                "raw_bytes": raw,
                "baseline_bytes": baseline,
                "delta_bytes": delta,
                "baseline_compression_pct": pct_saved(raw, baseline),
                "delta_compression_pct": pct_saved(raw, delta),
                "delta_vs_baseline_bytes": delta - baseline,
            })

    def totals(selected: list[dict]) -> dict:
        raw = sum(row["raw_bytes"] for row in selected)
        baseline = sum(row["baseline_bytes"] for row in selected)
        delta = sum(row["delta_bytes"] for row in selected)
        return {
            "raw_bytes": raw,
            "baseline_bytes": baseline,
            "delta_bytes": delta,
            "baseline_compression_pct": pct_saved(raw, baseline),
            "delta_compression_pct": pct_saved(raw, delta),
            "movement_percentage_points": round(pct_saved(raw, delta) - pct_saved(raw, baseline), 3),
            "delta_vs_baseline_drop_bytes": baseline - delta,
            "delta_vs_baseline_drop_pct": round(100 * (baseline - delta) / baseline, 3),
        }

    grouped: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        grouped[row["operation"]].append(row)
    first_or_changed = [row for row in rows if not row["repeat"]]
    repeats = [row for row in rows if row["repeat"]]
    regressions = [row for row in rows if row["delta_bytes"] > row["baseline_bytes"]]
    evidence = {
        "schema": "tokenzero.session-delta-headline.v1",
        "environment": bench_environment(REPO),
        "methodology": {
            "corpus": "12 turns x read/search/tree/git; read and tree stable, search changes every 6 turns, git every 8 turns",
            "baseline": "compact capsule emitted on every turn",
            "candidate": "identical compact capsule on first occurrence; turn-2+ identical refs use seen-set marker; delta telemetry remains on wire",
            "raw_denominator": "UTF-8 compact-JSON wire bytes for exact payload envelopes, including the same id/status/tool/ref framing",
            "serializer": "json.dumps(sort_keys=True,separators=(',',':')) plus newline",
            "integrity": "all operations and turns retained; no payload or loss filtering",
        },
        "fixture": {"turns": TURNS, "operations_per_turn": len(OP_SPECS), "samples": len(rows), "operation_specs": OP_SPECS},
        "headline": totals(rows),
        "turn_2_plus": totals([row for row in rows if row["turn"] > 1]),
        "repeated_outputs": totals(repeats),
        "first_or_changed_outputs": totals(first_or_changed),
        "per_operation": {op: totals(op_rows) for op, op_rows in grouped.items()},
        "losses": {
            "regressing_samples": len(regressions),
            "regression_bytes": sum(row["delta_bytes"] - row["baseline_bytes"] for row in regressions),
            "explanation": "First-seen and changed outputs carry delta telemetry without suppression and can be larger than baseline; they are included in headline totals.",
            "rows": regressions,
        },
        "samples": rows,
    }
    write_json(OUT, evidence, emit=True)


if __name__ == "__main__":
    main()
