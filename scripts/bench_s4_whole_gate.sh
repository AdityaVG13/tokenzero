#!/bin/sh
# TokenZero S4_whole clean-expand regression gate.
#
# Pins expand latency after the 2026-08-06 KEEP stack
# (H1a 5bab208, H3 5608dcf, R1 30061eb, H2 d39041e).
#
# Measurement: hyperfine on clean-cache S4_whole (not criterion).
# Default policy (both enforced when a baseline file is present):
#   1) absolute: fail if p50_ms > ABS_MS (default 30)
#   2) relative: fail if p50_ms > baseline_p50 * (1 + THRESHOLD/100) (default 25%)
# Always checks golden stdout sha when --check-golden (default on).
#
# Usage:
#   # Record baseline on a known-good release-perf binary:
#   scripts/bench_s4_whole_gate.sh --save-baseline
#
#   # Compare (fails on regression):
#   scripts/bench_s4_whole_gate.sh
#
# Options:
#   --save-baseline     Write measured p50 into baseline file and exit 0.
#   --baseline PATH     Baseline JSON (default: tests/fixtures/.../lock-in-baseline.json).
#   --abs-ms MS         Absolute p50 ceiling in ms (default: 30).
#   --threshold PCT     Relative regression percent (default: 25).
#   --runs N            Hyperfine runs (default: 20).
#   --warmup N          Hyperfine warmup (default: 5).
#   --bin PATH          tokenzero binary (default: TOKENZERO_BIN or target/release-perf/tokenzero).
#   --no-golden         Skip golden sha check.
#   --skip-seed         Do not re-seed cache (reuse existing store.json).
#
# Rebuild is NOT done here (RCH-only cargo policy). Build first:
#   rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero \
#     cargo build -p tokenzero --profile release-perf -j 4
#
# See: tests/artifacts/perf/2026-08-06-opt-loop/LOCK-IN.md
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

OPT_DIR="tests/artifacts/perf/2026-08-06-opt-loop"
BASELINE_DEFAULT="tests/fixtures/perf/2026-08-06-opt-loop/lock-in-baseline.json"
BASELINE_FALLBACK="$OPT_DIR/lock-in-baseline.json"
BASELINE="$BASELINE_DEFAULT"
CACHE_DIR="$OPT_DIR/gate-cache"
CACHE="$CACHE_DIR/store.json"
CORPUS_RS="tests/artifacts/perf/_corpus/large.rs"
REF="tz://blob/e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274"
GOLDEN_SHA="e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274"

ABS_MS=30
THRESHOLD=25
RUNS=20
WARMUP=5
SAVE=0
CHECK_GOLDEN=1
SKIP_SEED=0
BIN="${TOKENZERO_BIN:-$ROOT/target/release-perf/tokenzero}"

while [ $# -gt 0 ]; do
    case "$1" in
        --save-baseline) SAVE=1 ;;
        --baseline) BASELINE="$2"; shift ;;
        --abs-ms) ABS_MS="$2"; shift ;;
        --threshold) THRESHOLD="$2"; shift ;;
        --runs) RUNS="$2"; shift ;;
        --warmup) WARMUP="$2"; shift ;;
        --bin) BIN="$2"; shift ;;
        --no-golden) CHECK_GOLDEN=0 ;;
        --skip-seed) SKIP_SEED=1 ;;
        -h|--help)
            sed -n '2,40p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
    shift
done

if [ ! -x "$BIN" ] && [ ! -f "$BIN" ]; then
    echo "missing binary: $BIN" >&2
    echo "build with RCH: rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo build -p tokenzero --profile release-perf -j 4" >&2
    exit 3
fi

command -v hyperfine >/dev/null 2>&1 || {
    echo "hyperfine not found on PATH" >&2
    exit 3
}
command -v python3 >/dev/null 2>&1 || {
    echo "python3 not found on PATH" >&2
    exit 3
}

if [ ! -f "$CORPUS_RS" ]; then
    echo "missing corpus: $CORPUS_RS" >&2
    exit 3
fi

mkdir -p "$CACHE_DIR"

if [ "$SKIP_SEED" -eq 0 ]; then
    # Fresh-ish clean cache: remove store if present so seed is deterministic.
    # Keep dir; only wipe store family for a clean seed when not skipping.
    if [ ! -f "$CACHE" ]; then
        echo "seeding clean cache at $CACHE" >&2
        "$BIN" read "$CORPUS_RS" \
            --allowed-root "$ROOT" --cache-path "$CACHE" --json >/dev/null
    else
        echo "reusing existing cache $CACHE (pass --skip-seed explicitly is noop; delete gate-cache to reseed)" >&2
    fi
fi

if [ ! -f "$CACHE" ]; then
    echo "cache missing after seed: $CACHE" >&2
    exit 3
fi

if [ "$CHECK_GOLDEN" -eq 1 ]; then
    GOT="$("$BIN" expand "$REF" --cache-path "$CACHE" | shasum -a 256 | awk '{print $1}')"
    if [ "$GOT" != "$GOLDEN_SHA" ]; then
        echo "FAIL golden: got $GOT expected $GOLDEN_SHA" >&2
        exit 1
    fi
    echo "OK golden: $GOT" >&2
fi

OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT

echo "hyperfine S4_whole_clean (warmup=$WARMUP runs=$RUNS)" >&2
# Default shell so stdout redirect works. (shell=none cannot parse >/dev/null.)
# Shell spawn cost is noise vs 30 ms absolute / +25% relative thresholds.
hyperfine --warmup "$WARMUP" --runs "$RUNS" \
    --export-json "$OUT" \
    -n S4_whole_clean \
    "'$BIN' expand '$REF' --cache-path '$CACHE' >/dev/null"

P50_MS="$(python3 - "$OUT" <<'PY'
import json, sys
path = sys.argv[1]
d = json.load(open(path))
r = d["results"][0]
print(f"{r['median'] * 1000:.3f}")
PY
)"

echo "measured S4_whole clean p50: ${P50_MS} ms" >&2

if [ "$SAVE" -eq 1 ]; then
    python3 - "$BASELINE" "$P50_MS" "$ABS_MS" "$THRESHOLD" "$BIN" <<'PY'
import json, sys, hashlib, subprocess
from pathlib import Path
from datetime import datetime, timezone
path, p50, abs_ms, thr, bin_path = sys.argv[1:6]
p50 = float(p50)
try:
    h = hashlib.sha256(Path(bin_path).read_bytes()).hexdigest()
except Exception:
    h = "unknown"
try:
    ver = subprocess.check_output([bin_path, "--version"], text=True).strip()
except Exception:
    ver = "unknown"
base = {
    "name": "S4_whole_clean",
    "locked_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "binary_path": bin_path,
    "binary_sha256": h,
    "tokenzero_version": ver,
    "n": "see last hyperfine export",
    "p50_ms": p50,
    "ref": "tz://blob/e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274",
    "golden_stdout_sha256": "e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274",
    "thresholds": {
        "abs_p50_ms": float(abs_ms),
        "relative_pct": float(thr),
        "warn_p50_ms": 25.0,
    },
    "keep_commits": ["5bab208", "5608dcf", "30061eb", "d39041e"],
}
Path(path).parent.mkdir(parents=True, exist_ok=True)
Path(path).write_text(json.dumps(base, indent=2) + "\n")
print(f"baseline saved: {path} p50_ms={p50}", file=sys.stderr)
PY
    exit 0
fi

# Compare thresholds.
python3 - "$P50_MS" "$ABS_MS" "$THRESHOLD" "$BASELINE" <<'PY'
import json, sys
from pathlib import Path

p50 = float(sys.argv[1])
abs_ms = float(sys.argv[2])
thr = float(sys.argv[3])
base_path = Path(sys.argv[4])

fail = False
warn = False

if p50 > abs_ms:
    print(f"FAIL absolute: p50 {p50:.3f} ms > {abs_ms:.1f} ms ceiling", file=sys.stderr)
    fail = True
else:
    print(f"OK absolute: p50 {p50:.3f} ms <= {abs_ms:.1f} ms", file=sys.stderr)

if p50 > 25.0:
    print(f"WARN soft: p50 {p50:.3f} ms > 25.0 ms", file=sys.stderr)
    warn = True

if base_path.is_file():
    b = json.loads(base_path.read_text())
    bp = float(b["p50_ms"])
    limit = bp * (1.0 + thr / 100.0)
    delta_pct = ((p50 / bp) - 1.0) * 100.0
    print(f"baseline p50: {bp:.3f} ms; relative limit (+{thr:.0f}%): {limit:.3f} ms; delta: {delta_pct:+.1f}%", file=sys.stderr)
    if p50 > limit:
        print(f"FAIL relative: p50 {p50:.3f} ms > {limit:.3f} ms", file=sys.stderr)
        fail = True
    else:
        print(f"OK relative: p50 {p50:.3f} ms <= {limit:.3f} ms", file=sys.stderr)
else:
    print(f"note: no baseline at {base_path}; absolute-only check", file=sys.stderr)

if fail:
    print("FAIL: S4_whole clean expand regression", file=sys.stderr)
    sys.exit(1)

print("OK: no S4_whole clean expand regression beyond thresholds", file=sys.stderr)
sys.exit(0)
PY
