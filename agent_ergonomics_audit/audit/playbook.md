# TokenZero Agent Ergonomics Playbook — audit-only (honest)

Mode is **audit-only**: recommendations are filed as beads, not applied.

## Critical correction (Pass 2–3)

`tokenzero doctor --robot-triage` **already works** (exit 0, `tokenzero.doctor.robot_triage.v1`).  
Root `tokenzero --robot-triage` / `tokenzero robot-triage` fail (exit 2).  
**R-001 is discoverability + shape promotion**, not "add a missing mega-command from scratch."

## Intent headline (do not misuse naive 168/180)

- Naive sample: 180 rows / **48 unique** invocations (over-concentrated); raw outcomes skewed by repeated global typos.
- Combined unique (naive ∪ savvy ∪ pass-3): see `intent_metrics.json` — recovery is **asymmetric** (primary-path often recovers; global/MCP-name paths often fail).
- Clap tips that only suggest `--help` for unrelated flags are **wrong_hint** pedagogy (R-013), not wins.

## Top recommendations (post-triangulation)

### 1. R-004: Complete `capabilities.commands` from clap + fill empty help (P0)
**Problem:** capabilities lists ~17 commands; help has ~57 verbs; 28 empty blurbs.  
**Fix:** export full public verb set; fill About text; mark experimental.

### 2. R-002 + R-013: Global Levenshtein-1 recovery + DYM quality gate (P0)
**Problem:** global `--jsno`/`--jsonn` clap-fail; `read --jsno` recovers (island). Wrong tips (e.g. unrelated → `--help`).  
**Fix:** known-flags ed1 handler; refuse wrong-family suggestions.

### 3. R-018: run status-truth (P0)
**Problem:** `run --json -- false` → process exit 0 / `status=ok` while `command_success=false`.  
**Fix:** non-zero exit or top-level status=error when child fails.

### 4. R-016: MCP tz_* → CLI did-you-mean map (P0)
**Problem:** `tz_read` as CLI fails; tip may suggest `tree`.  
**Fix:** MCP→CLI table before generic similar.

### 5. R-017: CLI grep vs MCP tz_grep semantics (P0)
**Problem:** CLI grep = literal find alias; MCP tz_grep = regex.  
**Fix:** align or document loudly; no silent mismatch.

### 6. R-001 (refined): Promote `doctor --robot-triage` to root mega-path (P0/P1)
**Problem:** Mega-command **exists under doctor** but root aliases and Agent surfaces footer omit it; agents following Polish Bar look for root `--robot-triage` and miss the working path. Envelope should include quick_ref+recommendations+commands+health if not already complete.  
**Fix:**  
1) Alias root `--robot-triage` and `robot-triage` → `doctor --robot-triage`  
2) Add to top-level Agent surfaces footer and robot-docs First Commands  
3) Ensure envelope shape matches mega-command contract  
4) Pin schema in capabilities.output_schemas  

### 7. R-003: Error-Teaches exact corrected invocation (P1)
### 8. R-005: regression_tests/ pins (P1)
### 9. R-019: codemode feature_flags truth (P1)
### 10. R-015: Hide/gate empty-help experimental verbs (P1)

## Full ranked list
See `recommendations.jsonl` (R-001–R-023) and `bead_ids.txt`.

## Pass 2–3 triangulation delta
See `pass-2/triangulation_top10.md` and `pass-3/RESIDUAL_GAPS.md`.

Beads: 7 epics + 23 tasks in `bead_ids.txt`.
