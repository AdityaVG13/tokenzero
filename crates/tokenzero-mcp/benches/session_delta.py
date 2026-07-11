#!/usr/bin/env python3
"""Reproduce byte-stable session-delta envelope measurements.

The baseline and candidate use the same deterministic read payload, refs, and
JSON serializer. Only session-delta encoding differs. Output is the exact UTF-8
wire byte count for each compact JSON envelope.
"""
from __future__ import annotations

import json
from pathlib import Path

OUT = Path(__file__).with_suffix("") / "evidence.json"
REF = "tz://file/" + "a" * 64
SHA = "a" * 64
FULL = "\n".join(f"line {i:03}: session delta fixture payload" for i in range(1, 81))
DELTA = f"+{REF} {SHA} (already seen)"


def envelope(turn: int, enabled: bool) -> dict:
    visible = DELTA if enabled and turn > 1 else FULL
    result = {
        "accounting": {"raw_tokens": 640, "visible_tokens": 12 if visible == DELTA else 640},
        "refs": [{"kind": "file", "ref": REF}],
        "status": "ok",
        "tool": "read",
        "visible": {"text": visible},
    }
    if enabled:
        result["telemetry"] = {
            "output_strategy": "seen_set_dedup" if turn > 1 else "full",
            "session_delta": {
                "delta_bytes": len(visible.encode()),
                "from_hwm": turn - 1,
                "full_bytes": len(FULL.encode()),
                "saved_bytes": len(FULL.encode()) - len(visible.encode()),
                "to_hwm": turn,
            },
        }
    return {"id": turn, "jsonrpc": "2.0", "result": result}


def encoded_bytes(value: dict) -> int:
    return len((json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode())


def main() -> None:
    baseline = [encoded_bytes(envelope(turn, False)) for turn in range(1, 4)]
    candidate = [encoded_bytes(envelope(turn, True)) for turn in range(1, 4)]
    turns = []
    for turn, (before, after) in enumerate(zip(baseline, candidate), 1):
        turns.append({
            "turn": turn,
            "baseline_bytes": before,
            "candidate_bytes": after,
            "change_bytes": after - before,
            "change_pct": round(100 * (after - before) / before, 3),
        })
    evidence = {
        "schema": "tokenzero.session-delta-measurement.v1",
        "method": "byte-stable compact JSON serialization of identical deterministic read envelopes; only TOKENZERO_MCP_DEDUP semantics differ",
        "serializer": "json.dumps(sort_keys=True,separators=(',',':')) + newline; UTF-8 length",
        "fixture": {"full_payload_bytes": len(FULL.encode()), "delta_line_bytes": len(DELTA.encode()), "turns": 3},
        "baseline": {"session_delta": False, "turn_bytes": baseline, "total_bytes": sum(baseline)},
        "candidate": {"session_delta": True, "turn_bytes": candidate, "total_bytes": sum(candidate)},
        "turns": turns,
        "turn_2_plus": {
            "baseline_bytes": sum(baseline[1:]),
            "candidate_bytes": sum(candidate[1:]),
            "drop_bytes": sum(baseline[1:]) - sum(candidate[1:]),
            "drop_pct": round(100 * (sum(baseline[1:]) - sum(candidate[1:])) / sum(baseline[1:]), 3),
        },
        "losses": [f"turn 1 adds {candidate[0] - baseline[0]} bytes ({100 * (candidate[0] - baseline[0]) / baseline[0]:.3f}%) for watermark and byte telemetry"],
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
