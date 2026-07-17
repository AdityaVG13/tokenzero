#!/usr/bin/env python3
"""Reduce a Pulse JSONL export into auditable deployment-telemetry totals (P17-002)."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


def reduce_ledger(path: Path) -> dict[str, Any]:
    events: list[dict[str, Any]] = []
    skipped = 0
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            skipped += 1

    raw = sum(int(e.get("raw_tokens") or 0) for e in events)
    visible = sum(int(e.get("visible_tokens") or 0) for e in events)
    recovery = sum(int(e.get("recovery_tokens") or 0) for e in events)
    hidden = raw - visible
    net_savings = ((raw - (visible + recovery)) / raw) if raw else 0.0
    stamps = [int(e.get("timestamp_unix") or 0) for e in events if e.get("timestamp_unix")]
    window = None
    if stamps:
        window = {
            "start_unix": min(stamps),
            "end_unix": max(stamps),
            "span_seconds": max(stamps) - min(stamps),
        }
    return {
        "schema": "tokenzero.deployment-telemetry-reducer.v1",
        "source": str(path),
        "call_count": len(events),
        "skipped_lines": skipped,
        "raw_tokens": raw,
        "visible_tokens": visible,
        "hidden_tokens": hidden,
        "recovery_tokens": recovery,
        "net_savings": round(net_savings, 6),
        "hidden_fraction_of_raw": round((hidden / raw) if raw else 0.0, 6),
        "sampling_window": window,
        "host_count": 1,
    }


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if not args or args[0] in ("-h", "--help"):
        print(
            "usage: deployment_telemetry_reducer.py <ledger.jsonl> [--output out.json]",
            file=sys.stderr,
        )
        return 2
    ledger = Path(args[0])
    out = reduce_ledger(ledger)
    text = json.dumps(out, indent=2, sort_keys=True) + "\n"
    if "--output" in args:
        Path(args[args.index("--output") + 1]).write_text(text)
    print(text, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
