# Fresh-eyes review -- Pass 2 Round 1 (Phase 7)

**Reviewer role:** independent audit-quality + product spot-check  
**Target:** TokenZero `1.4.0` (`~/.local/bin/tokenzero`)  
**Scope:** audit artifacts under `agent_ergonomics_audit/` + live CLI probes (no product code changes; no `cargo test --workspace`)  
**Date:** 2026-07-27  

---

## Executive counts

| Bucket | Count |
|---|---:|
| Audit-quality issues noted | **18** |
| New product findings (not already a first-class `recommendations.jsonl` item) | **8** (R-016..R-023 stubs) |
| Fellow-agent findings under-promoted into recs | **6+** (FCC/parity) |

---

## Lens 1 -- Bugs / errors / confusion in the audit itself

### A1. Dual-scorer independence is fabricated (HIGH)

- Scorer A and B evidence maps are **byte-identical for all 68 surfaces** (`identical evidence A/B = 68/68`).
- Score deltas are only from the discrete set `{-25,-20,-15,+15}` (4 unique deltas); mean B−A ≈ −3.7.
- 56/68 A rows use template evidence `"baseline for verb <name>; see deep_probes"`.
- 28/68 surfaces share the exact same weighted mean **319.3** and the same three worst dims (107/207/207).
- Yet scorer rows claim `score_confidence: "evidence-backed-probe"` and merged surfaces claim `"dual-scorer-median"`.

**Impact:** Dimension medians and "highest/lowest" tables look precise but are mostly synthetic banded templates. Cannot support ranking arguments that depend on 10–30 point differences.

### A2. Intent headline metrics are misclassified and over-concentrated (HIGH)

Scorecard: `{'useless_error': 168, 'useful_hint': 12}` from n=180.

Problems:

1. **All 180 sampled runs are category A / naive generator.** Savvy corpus results exist (`partial/intent_savvy_results.jsonl`, n=83 with richer outcomes: 36 inferred_and_acted, 26 useful_hint, 19 useless_error) but **do not appear in the scorecard headline**.
2. Only **48 unique invocations** in the 180 sample. Top dups: `--jsonn`×46, `--cachepath`×30, `--dr-yrun`×10, `--dryrun`×10, `--robot-riage`×9. Not a stratified stress of the CLI.
3. All 12 `useful_hint` hits are clap suggestions of **`--help` for typos like `--halp/--hlep/--exlpain/--hep`**. R-013 correctly labels those suggestions as *wrong*. The metric therefore **counts harmful suggestions as useful_hint** -- direct contradiction with R-013.
4. All 180 actual exits are `2`. Paths that exit 0 (subcommand `--jsno` recovery) or 1 (missing path / blocked) were never in the naive sample that drives the headline ratio.

### A3. Probe scripts contaminate exit codes with pipelines (HIGH)

`partial/family_cross_cut_probes.txt` records `EXIT:0` for failing invocations such as `tokenzero --json` (stderr shows clap usage error; true exit is **2**). Pattern matches `cmd 2>&1 | head; echo EXIT:$?` capturing `head`'s status.

**Impact:** Any downstream claim that usage errors exit 0 is untrustworthy unless re-probed without pipes (or with `PIPESTATUS[0]` / `set -o pipefail`). R-007's "probes OK for robot-triage/jsno/nosuch" needs re-verification protocol, not the contaminated logs.

### A4. Manifest contradicts scorecard / recommendations (MEDIUM)

`audit/manifest.json` pass 1 summary still has:

- `surfaces_scored: 0` (scorecard: 68)
- `recommendations_total: 0` (file has 15 recs)
- `completed_at: null`
- `fresh_eyes_rounds: 0`

Pass metadata is not a reliable entry point for later phases.

### A5. Inventory vs scored set inconsistency (MEDIUM)

- Inventory: 728 surfaces, kinds **only** `verb` (112) + `flag` (616).
- Scored set: 68 rows including `exit_code__*`, `env__*`, and global flags that are not inventory rows.
- Env/exit surfaces score very high (691–725) largely from "documented in capabilities" evidence -- circular self-congratulation vs real agent recovery.

### A6. R-001 / R-010 stale or internally inconsistent (MEDIUM)

Live probes (accurate exits):

| Invocation | Exit | Notes |
|---|---:|---|
| `tokenzero --robot-triage` | 2 | missing (R-001 problem true at **root**) |
| `tokenzero doctor --robot-triage` | 0 | **flag already exists** |
| bare `tokenzero doctor` | 0 | **richer** multi-slice (~16KB, 31 keys) than triage (~546B, 10 keys) |

R-001 claims no robot-triage path at all (overstates).  
R-010 fix sketch says "Alias doctor --robot-triage" (already present).  
Triage shape lacks `recommendations[]` / `commands[]` / `quick_ref` that R-001 wants -- real gap is **shape + discoverability**, not existence of the flag.

### A7. Expected uplift overconfidence (MEDIUM)

Examples: R-004 `self_documentation +500`, R-001 `agent_ergonomics +400`, R-002 `intent_inference +400` on a 0–1000 scale where many surfaces already sit at 300–700. No baseline-surface-id binding; uplift not computable or falsifiable. Risk language copy-pasted (`"low-medium — surface-only"`) on all 15 recs.

### A8. Rec ID order vs priority order confusion (LOW)

File order is priority-sorted, so IDs appear as R-001, R-002, **R-004**, R-003, … All of R-001..R-015 exist (15 total); no missing ID -- but humans may think R-003 was dropped.

### A9. Empty phase deliverables despite claims (MEDIUM)

| Path | State |
|---|---|
| `audit/triangulation/` | `.gitkeep` only |
| `audit/regression_tests/` | `.gitkeep` only (R-005 not applied, expected in audit-only -- but scorecard still ranks regression_resistance as if measured) |
| `audit/agent_simulations/{pre,post}_pass_1/` | `.gitkeep` only |
| `scorecard.md` | ends after highest-15 table; no methodology, no dual-scorer agreement, no savvy rollup |

Pass-1 NOTES claim dual scorers + subagent clusters; evidence quality does not match the narrative.

### A10. Fellow-agent FCC/parity findings not promoted to recommendations (MEDIUM)

`partial/family_cross_cut_findings.jsonl` (15) and `partial/parity_findings.jsonl` (12) contain P0/P1 items only partially reflected in R-001..R-015:

| Fellow finding | In recommendations.jsonl? |
|---|---|
| FCC-002 envelope polymorphism | **No** |
| FCC-006 run status=ok while command_success=false | **No** (parity notes "run transport exit 0" lightly) |
| FCC-009 codemode_surface true / runtime missing | **No** |
| Parity: MCP `tz_*` → misleading CLI tips | Partially R-009 (weak; no tip-quality fix) |
| Parity: CLI grep literal vs MCP tz_grep regex | **No** |
| FCC-008 global `--json` / no `--robot-json` | Partial via R-002 only for typos |

### A11. stresses_surface_id tagging errors (LOW)

Naive samples like `tokenzero --jsonn` tagged `flag__read__json` / root flags mis-attributed to subcommand flag surfaces. Weakens surface-level intent scores.

### A12. Scorecard "intent sample outcomes" uses Python dict repr (LOW)

`{'useless_error': 168, 'useful_hint': 12}` is not JSON; fine for humans, awkward for machines parsing the scorecard.

---

## Lens 2 -- Product ergonomics the scorecard under-weighted or missed

Probes re-run with **non-piped** exit capture. Confirms much of FCC/parity work; items below are either absent from recs or understated.

### P1. `feature_flags.codemode_surface: true` while PATH binary cannot execute CodeMode (NEW → R-016)

```
tokenzero codemode --json --plan 'return 1'
→ status=error, exit 1
→ "CodeMode JavaScript sandbox was not compiled into this artifact (missing feature surface-codemode / rquickjs)"
```

Yet `capabilities --json` advertises `codemode_surface: true`, a full `codemode` block, and agent_next_steps that push the trampoline. Agents burn turns on a non-functional path. **Self-documentation lie.**

### P2. JSON envelope polymorphism (NEW → R-017; FCC-002)

No common required keys across `cli.v1`, `doctor.v1`, `doctor.health.v1`, `install_plan.v1`, `cache.v1`, `pulse.v1`, `clients.v1`, `codemode.v1` (`schema` vs `schema_version`, `visible_ack` vs `ack`, missing `tool`). Scorecard still gives strong mean `output_parseability` (627) driven by primary read-path only.

### P3. `run --json -- false` → process exit 0, top-level `status: "ok"`, `tool: "shell"`, `telemetry.command_success: false` (NEW → R-018; FCC-006)

Documented as intentional status-truth, but top-level `status=ok` + exit_codes meaning for 0 ("completed") trains agents to trust the wrong field. Not a first-class rec.

### P4. MCP name → CLI tip disasters (NEW → R-019; strengthens R-009)

| Agent types | Tip offered | Should suggest |
|---|---|---|
| `tz_read` | `tree` | `read` |
| `ls` | `false-success-shell` | `tree` / `glob` |
| `tz_shell` | `shell` (ok alias) | `run` primary |

Parity file has this; recommendations only say "document divergences."

### P5. Primary-verb flag help text mostly empty (NEW → R-020)

`read`/`find`/`run`/`edit`/`tree`/`expand` `--help` lists many flags with **no About text** (`--budget`, `--allowed-root`, `--cache-path`, `--timeout-seconds`, `--mode`, `--raw`, …). R-004 covers empty **subcommand** blurbs only.

### P6. `doctor --robot-triage` is thinner than bare doctor (NEW → R-023; corrects R-001/R-010)

Bare doctor already returns multi-slice health; triage drops to a 10-key summary without `next_steps` richness / recommendations / command catalog. Mega-command work should **merge up**, not re-alias an existing flag.

### P7. `hook claude-code` empty stdin → exit 0, empty stdout/stderr (NEW → R-022)

Silent success on missing payload. Violates Never-Silent-Fail; agents cannot tell mis-wiring from no-op.

### P8. Did-you-mean ranking noise for install typos (extends R-011/R-013)

`instal` suggests `stats`, `client-status`, `init`, `ingest`, `install-smoke`, `install` -- install is last among noise. Not just missing recovery; **bad ranking**.

### Confirmed existing recs (not new)

- Empty top-level about: **28** verbs -- R-004/R-015 valid.
- Global `--jsno` / `--jsonn` clap death -- R-002 valid.
- `--exlpain` → `--help` -- R-013 valid; conflicts with useful_hint labeling (A2).
- `search` works as alias, missing from top-level help list as named command -- R-006 valid (alias is on `find` in clap).
- capabilities 17 vs ~57 help verbs -- R-004/FCC-001 valid.
- dangerous_ops only install + cache prune -- R-008 valid; edit has `--dry-run` but not in registry.

---

## Lens 3 -- Overconfidence / contradictions among fellow scores & recs

1. **Intent 12 useful_hint vs R-013:** same `--exlpain→--help` events counted as success and filed as defect.
2. **regression_resistance median 357** treated as product gap (R-005) while audit itself has zero golden tests; circular.
3. **High scores for exit_code__0/1 (~725)** based on "documented in capabilities" while FCC-004 documents EC1 vs EC2 split and custom vs clap inconsistency -- scores ignore family-cross-cut evidence.
4. **verb__capabilities 762.9** with template evidence on several dims; FCC-001/009 show capabilities is incomplete and partially false -- score overconfident.
5. **R-001 vs R-010** double-count mega-command work with contradictory existence claims about `--robot-triage`.
6. **R-002 claim** "Subcommand read --jsno already infers" is **true** when path present (`tokenzero read README.md --jsno` exit 0). Good. But global dark-spot narrative over-indexes on a non-stratified naive sample.
7. **Playbook** lists R-005 sub-items as separate `###` noise in regex of IDs; Top-10 stops at R-010 and drops R-006/007/011/012/014 from narrative though they are in jsonl.
8. **Parity P0** (tz_* fail as CLI; grep regex semantic split) is higher severity than several P2 recs that made the top-10 by ID order -- priority_score process undervalued parity file.

---

## Recommended audit hygiene (before trusting pass-2 scores)

1. Re-score with **real dual evidence** or drop dual-scorer pretense; label confidence `synthetic-template` where applicable.
2. Rebuild intent headline from **union of naive+savvy**, unique invocations, and a classifier that marks wrong did-you-mean as `misleading_hint` (not `useful_hint`).
3. Fix probe harness exit capture; re-run family probes.
4. Sync `manifest.json` summary with artifact reality; set `completed_at` / counts.
5. Promote FCC-002/006/009 and parity P0 into recommendations (see `extra_recommendations.jsonl`).
6. Rewrite R-001/R-010 around existing `doctor --robot-triage` shape gaps.
7. Do not treat 319.3 cluster as independent measurements of 28 verbs.

---

## Probe log (representative, accurate exits)

```
tokenzero --robot-triage          → 2  (unexpected argument)
tokenzero --jsno                  → 2  (no did-you-mean)
tokenzero --exlpain               → 2  (tip: --help)
tokenzero read README.md --jsno   → 0  (recovers JSON)
tokenzero doctor --robot-triage   → 0  (thin triage schema)
tokenzero doctor                  → 0  (rich multi-slice)
tokenzero run --json -- false     → 0  (status=ok, command_success=false)
tokenzero codemode --plan 'return 1' --json → 1 (sandbox missing)
tokenzero tz_read README.md       → 2  (tip: tree)
tokenzero ls .                    → 2  (tip: false-success-shell)
tokenzero hook claude-code < /dev/null → 0 (empty)
```

---

## Artifacts written

- `audit/pass-2/fresh_eyes_round1.md` (this file)
- `audit/pass-2/fresh_eyes_findings.jsonl`
- `audit/pass-2/extra_recommendations.jsonl` (R-016..R-023)
