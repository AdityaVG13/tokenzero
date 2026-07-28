#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/../.." && pwd)"
python3 "$ROOT/checkers/w6_cont2_generalize.py"
python3 "$ROOT/checkers/w6_mdc_separation.py"
python3 "$ROOT/checkers/w6_bp1_agency_phase.py"
python3 "$REPO/wave6-attach-FLAT/12_SOLPRO_CONT2_CHECKS.py"
echo "ALL PASS"
