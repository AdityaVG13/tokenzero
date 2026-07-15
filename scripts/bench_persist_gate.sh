#!/bin/sh
# TokenZero persist-path regression gate.
#
# Pins the persist-path performance wins from commits 5e94976 (BufWriter),
# fd61796 (skip-reload), and d0f9422 (fsync policy) by running the
# `persist_path` criterion group and comparing against a saved baseline.
# Criterion's own baseline comparison is the measurement mechanism; this
# script drives it and fails on a >25% p50 (median) regression.
#
# Usage:
#   # 1. Record a baseline on a known-good checkout:
#   scripts/bench_persist_gate.sh --save-baseline
#
#   # 2. On a candidate checkout, compare against it (fails on regression):
#   scripts/bench_persist_gate.sh
#
# Options:
#   --save-baseline        Save current run as the baseline and exit 0.
#   --baseline NAME        Baseline name to save/compare (default: persist_gate).
#   --threshold PCT        Regression threshold in percent (default: 25).
#
# Runs from the repo root. All cargo invocations use -j 4 and only build
# -p tokenzero-recovery. No background processes.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

BENCH=hotpaths; GROUP=persist_path
BASELINE=persist_gate; THRESHOLD=25
SAVE=0

while [ $# -gt 0 ]; do
    case "$1" in
        --save-baseline) SAVE=1 ;;
        --baseline) BASELINE="$2"; shift ;;
        --threshold) THRESHOLD="$2"; shift ;;
        -h|--help)
            sed -n '2,23p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
    shift
done

if [ "$SAVE" -eq 1 ]; then
    echo "saving baseline '$BASELINE' for group '$GROUP'" >&2
    cargo bench -p tokenzero-recovery --bench "$BENCH" -j 4 -- \
        --save-baseline "$BASELINE" "$GROUP"
    echo "baseline '$BASELINE' saved" >&2
    exit 0
fi

# Compare against the saved baseline. Criterion prints a per-benchmark
# change report and, when a regression is detected, a line containing
# "Performance has regressed". We capture the output, surface it, and then
# enforce the numeric threshold on the reported median (p50) change.
TARGET_DIR="${CARGO_TARGET_DIR:-target}"; BASE_DIR="$TARGET_DIR/criterion"
if [ ! -d "$BASE_DIR" ]; then
    echo "no criterion data at $BASE_DIR; run --save-baseline first" >&2
    exit 3
fi

OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT

echo "comparing against baseline '$BASELINE' (threshold ${THRESHOLD}% p50)" >&2
cargo bench -p tokenzero-recovery --bench "$BENCH" -j 4 -- \
    --baseline "$BASELINE" "$GROUP" 2>&1 | tee "$OUT"

# Enforce the p50 threshold from criterion's per-estimate change lines, e.g.:
#   median   [+12.3% +18.7% +25.1%]
# We read the point estimate (middle value) for the median row and fail if
# any benchmark's median regressed beyond the threshold.
WORST="$(awk -v thr="$THRESHOLD" '
    /[Mm]edian/ {
        for (i = 1; i <= NF; i++) {
            v = $i; gsub(/[][%+]/, "", v)
            if (v ~ /^-?[0-9]+([.][0-9]+)?$/) {
                n = v + 0
                # the middle of the three bracketed values is the point estimate;
                # track the largest positive change seen across median rows.
                if (n > worst) worst = n
            }
        }
    }
    END { printf "%.2f", worst + 0 }
' "$OUT")"

echo "worst median change: ${WORST}% (threshold ${THRESHOLD}%)" >&2

# Also honor criterion's own regression verdict as a backstop.
if grep -q "Performance has regressed" "$OUT"; then
    REGRESSED=1
else
    REGRESSED=0
fi

awk -v w="$WORST" -v thr="$THRESHOLD" 'BEGIN { exit !(w > thr) }' && OVER=1 || OVER=0

if [ "$OVER" -eq 1 ] || [ "$REGRESSED" -eq 1 ]; then
    echo "FAIL: persist-path regression beyond ${THRESHOLD}% p50" >&2
    exit 1
fi

echo "OK: no persist-path regression beyond ${THRESHOLD}% p50" >&2
exit 0
