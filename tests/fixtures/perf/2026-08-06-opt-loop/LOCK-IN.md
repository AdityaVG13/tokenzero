# LOCK-IN -- S4_whole clean expand (Pass 11)

> **Canonical lock-in docs (this file):** `tests/fixtures/perf/2026-08-06-opt-loop/` (trackable).  
> **Full opt-loop working set** (hyperfine JSON, goldens, pass reports; gitignored):  
> `tests/artifacts/perf/2026-08-06-opt-loop/`.

**Phase:** Regression gate / lock-in after KEEP stack (not a product optim lever)  
**Date:** 2026-08-06  
**Verdict:** PRODUCTIVE (docs + optional hyperfine gate)  
**Product code changes:** **none**

This file freezes the post-KEEP expand latency bar so later work can detect
regressions without re-running the full 12-pass opt loop.

---

## KEEP stack (shipped)

| Order | Lever | Commit (short) | Full SHA (repo tip context) | Effect on primary gold |
|------:|-------|----------------|-----------------------------|------------------------|
| 1 | **H1a** candidate-driven `mask_expansion_secrets` | `5bab208` | `git show 5bab208 --stat` | S4_whole p50 **235.6 → ~69** ms |
| 2 | **H3** session resume prove-on-disk | `5608dcf` | | clean p50 **~26.3** ms |
| 3 | **R1** single-pass tokens + skip verified rehash | `30061eb` | | clean p50 **~23.0** ms |
| 4 | **H2** Auto find in-process (no `rg` spawn) | `d39041e` | `d39041e1ee3fe48681fb8d5e16fe718def59d2f3` | S3_find **~16 → ~10.6** ms; S4 hold |

Tip at final-sweep / lock-in: **`d39041e`** (ahead of origin/main at measurement time).

---

## Final numbers (host class: this MacBook, n=20 hyperfine, flag-off)

Source: [`final-sweep/summary.json`](../../../artifacts/perf/2026-08-06-opt-loop/final-sweep/summary.json) (Pass 9), confirmed Pass 10 ZERO-CHANGE.

| Gold | p50 ms | p95 ms | mean ± std ms | Artifact |
|------|-------:|-------:|---------------|----------|
| **S4_whole clean** (primary) | **21.73** | **22.20** | 21.78 ± 0.30 | [`final-sweep/hyperfine-S4_whole.json`](../../../artifacts/perf/2026-08-06-opt-loop/final-sweep/hyperfine-S4_whole.json) |
| S3_find Auto | 10.59 | 11.11 | 10.50 ± 0.51 | [`final-sweep/hyperfine-S3_find.json`](../../../artifacts/perf/2026-08-06-opt-loop/final-sweep/hyperfine-S3_find.json) |
| S4_window | 14.15 | 14.81 | 14.20 ± 0.38 | [`final-sweep/hyperfine-S4_window.json`](../../../artifacts/perf/2026-08-06-opt-loop/final-sweep/hyperfine-S4_window.json) |
| S4_symbol | 17.18 | 17.42 | 17.17 ± 0.18 | [`final-sweep/hyperfine-S4_symbol.json`](../../../artifacts/perf/2026-08-06-opt-loop/final-sweep/hyperfine-S4_symbol.json) |

**Trajectory (S4_whole clean p50):**  
235.6 → 69 (H1a) → 26.3 (H3) → 23.0 (R1) → **21.7** (final-sweep hold; H2 expand-neutral).

**Binary at final-sweep:**  
`aa616e4ab05d79a9f1ef1beb1ab2318f77568dd821710d3d8241a51af6822d45`  
→ `/tmp/rch_target_tokenzero/release-perf/tokenzero` (`tokenzero 1.4.0`).

---

## Golden identity (must not change)

| Field | Value |
|-------|-------|
| Ref | `tz://blob/e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274` |
| Expected stdout sha256 | `e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274` |
| Capture | [`goldens/`](../../../artifacts/perf/2026-08-06-opt-loop/goldens/) + [`goldens/GOLDEN-META.md`](../../../artifacts/perf/2026-08-06-opt-loop/goldens/GOLDEN-META.md) |
| Final-sweep status | **PASS** (`final-sweep/golden-status.txt`) |

Corpus is secret-free; non-raw expand is identity (stdout sha == CAS blob id).

### Golden re-verify

```bash
BIN="${TOKENZERO_BIN:-./target/release-perf/tokenzero}"
CACHE="tests/artifacts/perf/2026-08-06-opt-loop/final-sweep/cache/store.json"
# or: seed a fresh clean cache (see below)

"$BIN" expand \
  tz://blob/e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274 \
  --cache-path "$CACHE" | shasum -a 256
# expect: e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274
```

If sha differs: **FAIL golden -- treat as product regression**, not a perf flake.

---

## Rebuild (release-perf via RCH only)

Do **not** full-workspace cargo on this Mac. Targeted binary:

```bash
rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero \
  cargo build -p tokenzero --profile release-perf -j 4

# suite symlink (optional, matches prior passes)
mkdir -p target/release-perf
ln -sfn /tmp/rch_target_tokenzero/release-perf/tokenzero target/release-perf/tokenzero
```

Profile definition: workspace `Cargo.toml` `[profile.release-perf]` (inherits release, opt-level 3, thin LTO, codegen-units 1).

---

## Clean-cache S4_whole recipe (primary gold)

Fat `tests/artifacts/perf/_corpus/caches/s2.json` is **polluted** for wall-clock (~48 ms class) -- do not use as primary latency gold.

```bash
ROOT=tests/artifacts/perf/2026-08-06-opt-loop
BIN="${TOKENZERO_BIN:-./target/release-perf/tokenzero}"
CACHE_DIR="$ROOT/gate-cache"   # or reuse final-sweep/cache
mkdir -p "$CACHE_DIR"
CACHE="$CACHE_DIR/store.json"
REF=tz://blob/e179e885d1e6c39cd3d35ed21d7820461ef75af429b0e331892aef8240003274

# seed (once)
"$BIN" read tests/artifacts/perf/_corpus/large.rs \
  --allowed-root "$PWD" --cache-path "$CACHE" --json >/dev/null

# golden
"$BIN" expand "$REF" --cache-path "$CACHE" | shasum -a 256

# latency (page-touch + process warm; default shell for >/dev/null)
hyperfine --warmup 5 --runs 20 --export-json /tmp/s4_whole.json \
  -n S4_whole_clean \
  "'$BIN' expand '$REF' --cache-path '$CACHE' >/dev/null"
```

---

## Regression thresholds

Apply on **this host class** (Apple Silicon laptop similar to measurement machine).  
Cross-host numbers are not comparable without a re-baseline.

| Check | Rule | Rationale |
|-------|------|-----------|
| **Absolute ceiling** | Fail if S4_whole clean **p50 > 30 ms** | ~1.4× final 21.7 ms; catches H1a-scale reversions without flaking ±0.5 ms noise |
| **Relative** | Fail if p50 **> baseline × 1.25** (+25%) | Same policy as `scripts/bench_persist_gate.sh` |
| **Golden** | Fail if stdout sha ≠ `e179e885…` | Isomorphism / correctness |
| **Optional soft** | Warn if p50 > 25 ms | Early signal before absolute hard fail |

Saved numeric baseline (from final-sweep):

| Field | Value |
|-------|-------|
| `baseline_p50_ms` | **21.73** |
| `baseline_p95_ms` | **22.20** |
| Source | `final-sweep/hyperfine-S4_whole.json` |
| File for gate | [`lock-in-baseline.json`](./lock-in-baseline.json) |

### Absolute vs relative

- Use **absolute 30 ms** as a host-class hard stop when no baseline file is present.
- When `lock-in-baseline.json` (or `--baseline PATH`) exists, enforce **both** absolute and relative.
- Re-save baseline only after intentional KEEP re-measure on the same host class (`scripts/bench_s4_whole_gate.sh --save-baseline`).

---

## Gate script

Optional small driver (hyperfine + JSON parse; no criterion, no product hooks):

```bash
# compare against lock-in baseline + 30 ms absolute
scripts/bench_s4_whole_gate.sh

# record a new baseline after intentional re-measure
scripts/bench_s4_whole_gate.sh --save-baseline

# tune
scripts/bench_s4_whole_gate.sh --abs-ms 30 --threshold 25 --runs 20
```

Manual-only path is the recipe above; the script is sugar, not required CI infra.

---

## What this is not

- Not a product hot-path optim lever (Pass 11 = lock-in only).
- Not a claim that 21.7 ms is a labeled Q99 quality metric -- it is wall-clock gold only.
- Not portable across machines without re-baseline.
- Pass 10 already established **no remaining product lever ≥ 2.0** on S4_whole; residual E1 (CLI envelope) is out of band.

---

## Related artifacts

| Path | Role |
|------|------|
| [`final-sweep/`](../../../artifacts/perf/2026-08-06-opt-loop/final-sweep/) | Pass 9 deep re-profile numbers |
| [`pass-10-no-lever/`](../../../artifacts/perf/2026-08-06-opt-loop/pass-10-no-lever/) | Absolute no-lever confirmation |
| [`OPTIMIZATION-MATRIX.md`](../../../artifacts/perf/2026-08-06-opt-loop/OPTIMIZATION-MATRIX.md) | Scored levers |
| [`ISOMORPHISM-PLAN.md`](../../../artifacts/perf/2026-08-06-opt-loop/ISOMORPHISM-PLAN.md) | Behavior contracts under opts |
| [`scripts/bench_persist_gate.sh`](../../../../scripts/bench_persist_gate.sh) | Sibling gate (criterion persist_path) |
| [`scripts/bench_s4_whole_gate.sh`](../../../../scripts/bench_s4_whole_gate.sh) | This lock-in gate |

---

*Pass 11 complete. No product code changes. No commit from this pass.*
