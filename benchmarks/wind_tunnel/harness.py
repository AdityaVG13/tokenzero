#!/usr/bin/env python3
"""Wind-tunnel MVP: counterfactual replay of plan-journal action sequences.

Loads recorded TokenZero plan journals (or fixtures), runs them through a
baseline vs candidate context-policy stub, diffs the resulting action
sequences, and exits non-zero on divergence.

This is intentionally NOT the full Mars wind-tunnel (no model re-execution,
no multi-hour corpus runs). It lands the gate shape: load -> policy stub ->
diff -> reject. Point ``--journals`` at real on-disk journals when ready.

Usage (fixtures, seconds):
  python3 benchmarks/wind_tunnel/harness.py \\
    --journals benchmarks/wind_tunnel/fixtures \\
    --baseline identity --candidate identity

Expect divergence (exit 1):
  python3 benchmarks/wind_tunnel/harness.py \\
    --journals benchmarks/wind_tunnel/fixtures \\
    --baseline identity --candidate drop_shell

Real journals (local store; keep --limit small):
  python3 benchmarks/wind_tunnel/harness.py \\
    --journals .zerostack/tokenzero/plan-journals \\
    --baseline identity --candidate identity --limit 32

Bead: tokenzero-wind-tunnel-replay-tyq (MVP scope).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable, Sequence

# Allow `python3 benchmarks/wind_tunnel/harness.py` from the repo root.
_REPO_ROOT = Path(__file__).resolve().parents[2]
if str(_REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(_REPO_ROOT))

from benchmarks.wind_tunnel.policies import get_policy
from benchmarks.wind_tunnel.types import Action, SequenceDiff

SCHEMA = "tokenzero.wind-tunnel-replay.v1"
PLAN_JOURNAL_VERSION = "tokenzero.plan-journal.v1"


def extract_actions(journal: dict[str, Any]) -> list[Action]:
    ops = journal.get("operations")
    if not isinstance(ops, list):
        raise ValueError("journal missing operations[]")
    actions: list[Action] = []
    for i, op in enumerate(ops):
        if not isinstance(op, dict):
            raise ValueError(f"operations[{i}] is not an object")
        method = op.get("method")
        if not isinstance(method, str) or not method:
            raise ValueError(f"operations[{i}] missing method")
        index = int(op.get("index", i))
        op_id = str(op.get("id", f"op-{index}"))
        state = str(op.get("state", ""))
        actions.append(Action(index=index, id=op_id, method=method, state=state))
    return actions


def load_journal(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"{path}: expected JSON object")
    version = data.get("version")
    if version is not None and version != PLAN_JOURNAL_VERSION:
        raise ValueError(
            f"{path}: unsupported version {version!r} "
            f"(expected {PLAN_JOURNAL_VERSION})"
        )
    return data


def iter_journal_paths(root: Path) -> list[Path]:
    if root.is_file():
        return [root]
    if not root.is_dir():
        raise FileNotFoundError(f"journals path not found: {root}")
    return sorted(p for p in root.glob("*.json") if p.is_file())


def replay_under_policy(
    actions: Sequence[Action],
    policy_name: str,
) -> list[Action]:
    """Apply a policy stub to the recorded sequence (deterministic MVP replay)."""
    return get_policy(policy_name)(actions)


def first_divergence(
    baseline: Sequence[Action],
    candidate: Sequence[Action],
) -> int | None:
    n = max(len(baseline), len(candidate))
    for i in range(n):
        if i >= len(baseline) or i >= len(candidate):
            return i
        if baseline[i].fingerprint() != candidate[i].fingerprint():
            return i
    return None


def diff_journal(
    path: Path,
    baseline_policy: str,
    candidate_policy: str,
) -> SequenceDiff:
    journal = load_journal(path)
    recorded = extract_actions(journal)
    baseline = replay_under_policy(recorded, baseline_policy)
    candidate = replay_under_policy(recorded, candidate_policy)
    return SequenceDiff(
        journal=str(path),
        baseline=baseline,
        candidate=candidate,
        first_divergence=first_divergence(baseline, candidate),
    )


def run_corpus(
    paths: Iterable[Path],
    baseline_policy: str,
    candidate_policy: str,
) -> dict[str, Any]:
    diffs: list[SequenceDiff] = []
    for path in paths:
        diffs.append(diff_journal(path, baseline_policy, candidate_policy))
    mismatches = [d for d in diffs if not d.match]
    return {
        "schema": SCHEMA,
        "mvp": True,
        "mvp_scope": (
            "Policy stubs only; no model re-execution. Gates action-sequence "
            "identity between baseline and candidate transforms of recorded "
            "plan-journal operations (tokenzero-wind-tunnel-replay-tyq MVP)."
        ),
        "baseline_policy": baseline_policy,
        "candidate_policy": candidate_policy,
        "journals_n": len(diffs),
        "matches_n": len(diffs) - len(mismatches),
        "divergences_n": len(mismatches),
        "match": len(mismatches) == 0,
        "divergences": [d.to_dict() for d in mismatches],
        "journals": [
            {
                "path": d.journal,
                "match": d.match,
                "first_divergence": d.first_divergence,
                "baseline_len": len(d.baseline),
                "candidate_len": len(d.candidate),
            }
            for d in diffs
        ],
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Wind-tunnel MVP: replay plan journals under baseline vs candidate "
            "policy stubs and diff action sequences."
        )
    )
    parser.add_argument(
        "--journals",
        type=Path,
        required=True,
        help=(
            "Directory of plan-journal JSON files, or a single journal path. "
            "Real stores usually live at "
            ".zerostack/tokenzero/plan-journals (or "
            "$ZEROSTACK_STORE_ROOT/tokenzero/plan-journals)."
        ),
    )
    parser.add_argument(
        "--baseline",
        default="identity",
        help="Baseline policy stub (default: identity).",
    )
    parser.add_argument(
        "--candidate",
        default="identity",
        help="Candidate policy stub (default: identity).",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=0,
        help="Optional max journals to load (0 = all). Keep small for smoke.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="Optional path to write the JSON report.",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Print only a one-line summary object.",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        paths = iter_journal_paths(args.journals.resolve())
    except FileNotFoundError as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr)
        return 2
    if not paths:
        print(
            json.dumps({"error": f"no *.json journals under {args.journals}"}),
            file=sys.stderr,
        )
        return 2
    if args.limit and args.limit > 0:
        paths = paths[: args.limit]

    try:
        report = run_corpus(paths, args.baseline, args.candidate)
    except ValueError as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr)
        return 2

    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    if args.quiet:
        print(
            json.dumps(
                {
                    "match": report["match"],
                    "journals_n": report["journals_n"],
                    "divergences_n": report["divergences_n"],
                    "baseline_policy": report["baseline_policy"],
                    "candidate_policy": report["candidate_policy"],
                    "wrote": str(args.output) if args.output else None,
                },
                sort_keys=True,
            )
        )
    else:
        print(json.dumps(report, indent=2, sort_keys=True))

    return 0 if report["match"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
