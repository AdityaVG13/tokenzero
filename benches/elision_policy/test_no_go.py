#!/usr/bin/env python3
"""Machine-verifiable no-go gate for adaptive crossover (tokenzero-3bf).

Fails if evidence claims a robust win without promotion criteria, or if product
code appears to wire adaptive crossover while robust_win is false.
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = Path(__file__).with_name("evidence.json")
EVALUATOR = Path(__file__).with_name("evaluate.py")
STATIC_PREVIEW = 256
MIN_HOLDOUT = 30


def main() -> int:
    # Refresh evidence so the gate is not stale relative to corpus inputs.
    proc = subprocess.run(
        [sys.executable, str(EVALUATOR), "--root", str(ROOT), "--output", str(EVIDENCE)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        print(proc.stdout)
        print(proc.stderr, file=sys.stderr)
        return proc.returncode

    raw = EVIDENCE.read_bytes()
    report = json.loads(raw)
    findings = report["findings"]
    corpus = report["corpus"]
    holdout = report["holdout_evaluation"]
    replay = report["recorded_session_replay"]
    decision = findings["adaptive_crossover_decision"]
    robust = findings["robust_win"]

    assert report["schema"] == "tokenzero.elision-policy-evaluation.v1"
    assert decision in {"do-not-implement", "promote-behind-default-off-flag"}
    assert robust is True or decision == "do-not-implement"
    assert robust is False or decision == "promote-behind-default-off-flag"

    # Current corpus: no explicit expansion labels ⇒ must not promote.
    if corpus["explicitly_labeled_rows"] == 0:
        assert robust is False
        assert decision == "do-not-implement"
        assert holdout["observations"] == 0
        assert findings["learned_classes"] == []
        # Recorded session-delta replay must be pure static fallback ties.
        assert replay["observations"] >= 1
        assert replay["candidate_visible_byte_turns"] == replay["static_visible_byte_turns"]
        for name, row in replay["per_class"].items():
            assert row["outcome"] == "tie", name
            assert row["candidate_status"] == "static-fallback", name
            assert row["candidate_minus_static_byte_turns"] == 0, name

    # Promotion math must hold whenever robust_win is claimed.
    if robust:
        overall = holdout["overall"]
        assert holdout["observations"] >= MIN_HOLDOUT
        assert findings["learned_classes"]
        assert overall["candidate_ledger_token_turns"] < overall["static_ledger_token_turns"]
        assert overall["candidate_expansion_miss_rate"] <= overall["static_expansion_miss_rate"]

    # Product code must not wire adaptive crossover while no-go stands.
    if not robust:
        banned = re.compile(
            r"adaptive[_-]?crossover|elision_policy_table|learned_preview_tokens|promote-behind-default-off-flag",
            re.I,
        )
        offenders: list[str] = []
        for path in ROOT.joinpath("crates").rglob("*.rs"):
            text = path.read_text(encoding="utf-8", errors="ignore")
            if banned.search(text):
                offenders.append(str(path.relative_to(ROOT)))
        assert not offenders, f"adaptive crossover appears wired despite no-go: {offenders}"

    digest = hashlib.sha256(raw).hexdigest()
    print(
        json.dumps(
            {
                "ok": True,
                "evidence_sha256": digest,
                "robust_win": robust,
                "adaptive_crossover_decision": decision,
                "explicitly_labeled_rows": corpus["explicitly_labeled_rows"],
                "recorded_session_replay_rows": corpus["recorded_session_replay_rows"],
                "candidate_minus_static_byte_turns": (
                    replay["candidate_visible_byte_turns"] - replay["static_visible_byte_turns"]
                    if replay.get("candidate_visible_byte_turns") is not None
                    else None
                ),
                "static_preview_tokens": report["predictor"]["static_preview_tokens"],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
