#!/usr/bin/env python3
"""Fail-closed gate for README deployment telemetry provenance (P17-002).

Requires a checked-in evidence package that records sampling frame + uncertainty
and a deterministic reducer over an exported Pulse ledger fixture. README
historical ~20k / 38.1M figures remain non-release telemetry until a matching
full ledger is attached; the fixture proves the audit path.
"""
from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]


def _reduce_ledger(path: Path) -> dict[str, Any]:
    # Load sibling module without requiring PYTHONPATH=repo root.
    mod_path = Path(__file__).resolve().parent / "deployment_telemetry_reducer.py"
    spec = importlib.util.spec_from_file_location("deployment_telemetry_reducer", mod_path)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.reduce_ledger(path)
EVIDENCE_REL = Path("benchmarks/claims/deployment-telemetry/evidence.json")
FIXTURE_REL = Path("benchmarks/claims/deployment-telemetry/fixture-ledger.jsonl")

REQUIRED_README_MARKERS = (
    "Treat this as deployment",
    "not a release claim",
    "benchmarks/claims/deployment-telemetry/evidence.json",
)

README_CLAIMED = {
    "call_count_approx": 20_000,
    "raw_tokens_approx": 38_100_000,
    "hidden_tokens_approx": 17_900_000,
    "net_savings_approx": 0.30,
}


def evaluate(root: Path | None = None) -> dict[str, Any]:
    base = root or REPO
    readme = (base / "README.md").read_text()
    evidence_path = base / EVIDENCE_REL
    fixture_path = base / FIXTURE_REL

    missing_markers = [m for m in REQUIRED_README_MARKERS if m not in readme]
    evidence: dict[str, Any] | None = None
    if evidence_path.is_file():
        evidence = json.loads(evidence_path.read_text())

    reducer: dict[str, Any] | None = None
    if fixture_path.is_file():
        reducer = _reduce_ledger(fixture_path)

    required_evidence_keys = (
        "candidate_id",
        "sampling_frame",
        "uncertainty",
        "readme_claimed",
        "audited_in_checkout",
        "export_command",
    )
    evidence_ok = evidence is not None and all(k in evidence for k in required_evidence_keys)
    fixture_ok = reducer is not None and reducer["call_count"] >= 1 and reducer["raw_tokens"] > 0
    markers_ok = not missing_markers

    audited = bool(evidence and evidence.get("audited_in_checkout"))
    matched = bool(evidence and evidence.get("readme_totals_match_attached_ledger"))

    if not markers_ok or not evidence_ok or not fixture_ok:
        result = "REJECT_MISSING_EVIDENCE"
    elif audited and matched:
        result = "ACCEPT_AUDITED"
    else:
        # Fail closed for release use; accept as gated telemetry with attached path.
        result = "ACCEPT_GATED_TELEMETRY_EVIDENCE"

    return {
        "gate": "deployment-telemetry-evidence",
        "candidate_id": "P17-002",
        "evidence_path": str(EVIDENCE_REL),
        "fixture_ledger_path": str(FIXTURE_REL),
        "readme_markers_ok": markers_ok,
        "missing_readme_markers": missing_markers,
        "evidence_ok": evidence_ok,
        "fixture_reducer": reducer,
        "audited_in_checkout": audited,
        "readme_totals_match_attached_ledger": matched,
        "readme_claimed": README_CLAIMED,
        "export_command": (evidence or {}).get("export_command")
        or "tokenzero pulse export-jsonl <output.jsonl> --json",
        "note": (
            "README deployment totals need a checked-in Pulse export, sampling "
            "window, and uncertainty. Until a ledger matching the historical "
            "~20k/38.1M/17.9M/30% figures is attached, treat them as gated "
            "telemetry (not release-audited). The fixture + reducer prove the "
            "audit path."
        ),
        "result": result,
    }


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    out = evaluate()
    text = json.dumps(out, indent=2, sort_keys=True) + "\n"
    if "--output" in args:
        Path(args[args.index("--output") + 1]).write_text(text)
    print(text, end="")
    return 0 if out["result"].startswith("ACCEPT_") else 1


if __name__ == "__main__":
    raise SystemExit(main())
