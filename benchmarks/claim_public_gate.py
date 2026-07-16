"""Gate the public northstar 99% claim until claim-audit publication allows it."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]

# Phrases that must appear near the end-to-end / northstar headline.
REQUIRED_README_GATE_MARKERS = (
    "fixed-suite point estimate",
    "not a workload-population or release claim",
    "public_claims_approved",
    "release_publication_allowed",
    "tokenzero claim-audit",
)


def pct(raw: int, visible: int) -> float:
    return 100.0 * (raw - visible) / raw


def evaluate(root: Path | None = None) -> dict[str, Any]:
    base = root or REPO
    north = json.loads((base / "benchmarks/northstar/current.json").read_text())
    demo = json.loads((base / "demo/demo_results.json").read_text())
    audit = json.loads((base / "results/current/tokenzero_claim_audit.json").read_text())
    readme = (base / "README.md").read_text()

    n = north["compression"]["totals"]
    d = demo["totals"]
    measured = pct(n["raw_tokens"], n["visible_tokens"])
    claimed = n["savings_pct"]
    conservative = measured >= claimed

    population_ci_available = False
    random_sample = False
    approved = bool(audit.get("public_claims_approved"))
    publication_allowed = bool(audit.get("release_publication_allowed"))
    claim_status = audit.get("claim_status")

    readme_gated = all(marker in readme for marker in REQUIRED_README_GATE_MARKERS)
    missing_markers = [m for m in REQUIRED_README_GATE_MARKERS if m not in readme]

    # Fixed-suite arithmetic alone never unlocks public publication.
    if not population_ci_available:
        correctly_gated = (
            conservative
            and readme_gated
            and not approved
            and not publication_allowed
            and claim_status == "blocked"
        )
        result = (
            "ACCEPT_GATED_PUBLIC_CLAIMS" if correctly_gated else "REJECT_PUBLIC_CLAIMS"
        )
    else:
        correctly_gated = (
            conservative and approved and publication_allowed and readme_gated
        )
        result = "ACCEPT_PUBLIC_CLAIMS" if correctly_gated else "REJECT_PUBLIC_CLAIMS"

    return {
        "northstar": {
            "raw": n["raw_tokens"],
            "visible": n["visible_tokens"],
            "measured_pct": measured,
            "claimed_pct": claimed,
            "workloads": len(north["compression"]["workloads"]),
            "snapshots": 1,
            "claim_is_conservative_lower_bound": conservative,
        },
        "demo": {
            "raw": d["raw_tokens"],
            "visible": d["visible_tokens"],
            "measured_pct": pct(d["raw_tokens"], d["visible_tokens"]),
            "claimed_pct": d["savings_pct"],
            "workloads": len(demo["workloads"]),
            "snapshots": 1,
            "rounds_to_claim": round(pct(d["raw_tokens"], d["visible_tokens"]), 1)
            == d["savings_pct"],
        },
        "publication_gate": {
            "claim_status": claim_status,
            "public_claims_approved": approved,
            "release_publication_allowed": publication_allowed,
        },
        "readme_gate": {
            "gated": readme_gated,
            "missing_markers": missing_markers,
        },
        "statistical_scope": {
            "random_sample": random_sample,
            "population_ci_available": population_ci_available,
            "reason": (
                "one checked-in snapshot per fixed convenience suite; "
                "exact arithmetic supports only these suites, not "
                "workload-population generalization"
            ),
        },
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
