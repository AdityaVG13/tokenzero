# Phase 4 Triangulation — Top 10 Recommendations (Pass 2)

**Target:** `/Users/aditya/AI/TokenZero`  
**Binary:** `tokenzero` 1.4.0 (`~/.local/bin/tokenzero`)  
**Mode:** audit-only (no product changes; no `cargo test --workspace`)  
**Triangulator:** independent re-eval of playbook Top 10 against live CLI + corpus + family cross-cut  
**Date:** 2026-07-27  

**Inputs:** `recommendations.jsonl` (R-001..R-015), `playbook.md`, `scorecard.md`, `intent_inference_corpus.jsonl` / `partial/intent_sample_results.jsonl`, `partial/deep_probes.txt`, `partial/deep_probes2.txt`, `agent_surfaces.jsonl`, `pass-2/family_cross_cut.md`, `pass-1/parity_mcp_cli.md`, live probes below.

---

## Method

For each Top-10 rec:

1. Re-run or re-confirm **live command + exit + excerpt**
2. Score **Agree / Disagree / Modify** (priority and/or scope)
3. Note **contradictions with existing strengths** (capabilities JSON, robot-docs, doctor multi-slice, `read --jsno` recovery)
4. Propose **merge/split** when overlap is structural

Lowest-dimension residual gaps collected at end (intent_inference, error_pedagogy, regression_resistance, self_documentation on empty verbs).

---

## Live evidence board (triangulator-run)

| Probe | Exit | Excerpt / note |
|---|---:|---|
| `tokenzero --robot-triage` | 2 | `unexpected argument '--robot-triage'` |
| `tokenzero robot-triage` | 2 | `unrecognized subcommand 'robot-triage'`; tip: `robot-docs`, `robot-doc` |
| `tokenzero --jsno` | 2 | clap unexpected; no did-you-mean for `--json` |
| `tokenzero --jsonn` | 2 | same |
| `tokenzero read --jsno Cargo.toml` | 0 | full `tokenzero.cli.v1` ok JSON (typo recovered) |
| `tokenzero capabilities --jsno` | 0 | full capabilities contract |
| `tokenzero --exlpain` | 2 | tip suggests **`--help`** (wrong family for "explain") |
| `tokenzero read` | 1 | `Error: read requires a path` — no copy-pasteable `tokenzero read <path> --json` |
| `tokenzero edit --force` | 2 | tip: pass as value via `-- --force` (unhelpful) |
| `tokenzero find --foo query` | 2 | clap default + `--help` only |
| `tokenzero search foo` | 0 | works; tool path is find; **not** listed as top-level help verb |
| `tokenzero rn --json -- true` | 0 | recovers to run/shell envelope |
| `tokenzero reed Cargo.toml` | 2 | tip includes `read` among similar subcommands (not auto-act) |
| `tokenzero robot-help` / `--robot-help` | 0 | robot guide (R-014 mostly done) |
| `tokenzero capabilities --json` | 0 | **17** `commands[]`; no `mcp_tools`; `dangerous_operations` = install + cache prune only |
| `tokenzero --help` empty blurbs | 0 | **28** verbs with blank description |
| `tokenzero doctor` bare | 0 | multi-slice JSON (`cache`, `capabilities`, `mcp`, `next_steps`, …); **no** `recommendations[]` |
| Intent sample n=180 | — | `useless_error` **168**, `useful_hint` **12** (12 are almost all `--halp/--hlep/--exlpain` → tip `--help`) |
| `audit/regression_tests/` | — | **only** `.gitkeep` (no golden pins) |

Scorecard medians (pass-1): intent_inference **390**, error_pedagogy **427**, regression_resistance **357**, self_documentation min **107** on empty-verb cluster.

---

## Per-rec triangulation

### 1. R-001 — `--robot-triage` mega-command (score 980 → **MODIFY ↓ P1**)

| Field | Judgment |
|---|---|
| Verdict | **Modify** — keep problem, demote priority, merge with R-010 |
| Prior playbook | P0 / 980 |
| Triangulated | **P1** / ~820 |
| Why | Gap is real (live EC2), but **not** the largest agent blast. Canonical path already exists: `capabilities --json` + `doctor` + `robot-docs guide`. Adding a fourth mega name is composition, not discovery repair. |

**Evidence:**

```text
$ tokenzero --robot-triage
EXIT:2
error: unexpected argument '--robot-triage' found
Usage: tokenzero [COMMAND]
For more information, try '--help'.
```

```text
$ tokenzero robot-triage
EXIT:2
error: unrecognized subcommand 'robot-triage'
  tip: some similar subcommands exist: 'robot-docs', 'robot-doc'
```

**Contradictions with strengths:**

- `tokenzero capabilities --json` already returns contract + `agent_next_steps` + `canonical_invocations` (strength: self_documentation high on this surface, weighted_mean **762.9**).
- Bare `tokenzero doctor` already multi-slice health JSON (`cache`, `capabilities`, `mcp.ready`, `next_steps`, …) — ergonomics **810**.
- `robot-docs guide` is paste-ready and documents first commands.

**Merge:** **R-001 ∪ R-010** → single theme *Mega-path composition*: either (a) `doctor --robot-triage` alias that *adds* `recommendations[]`/`commands[]`/quick_ref shape, or (b) root alias that **shells the same composition** without inventing a parallel health engine.

**Do not:** ship a mega-command that reimplements doctor while leaving caps incomplete (that would paper over R-004).

---

### 2. R-002 — Global Levenshtein-1 flag recovery (score 950 → **AGREE P0**, scope tighten)

| Field | Judgment |
|---|---|
| Verdict | **Agree** P0 (intent_inference is worst agent dimension after regression_resistance) |
| Prior | P0 / 950 |
| Triangulated | **P0** / 960 |
| Scope change | Prioritize **subcommand + high-frequency flags** first; bare global `--jsno` with no verb is lower value than `tokenzero <verb> --jsno` (already partially works) |

**Evidence:**

```text
$ tokenzero --jsno
EXIT:2
error: unexpected argument '--jsno' found
Usage: tokenzero [COMMAND]
# no tip for --json

$ tokenzero read --jsno Cargo.toml
EXIT:0
{"schema_version":"tokenzero.cli.v1","status":"ok","tool":"read",...}

$ tokenzero capabilities --jsno
EXIT:0
# full capabilities contract
```

Intent sample: **168/180** `useless_error`, **12/180** `useful_hint`. Corpus is **category A only** (bare global flag typos) — methodologically overweights global-root failures, but island-shaped recovery is still confirmed by family cross-cut X3.

**Contradictions with strengths:**

- Subcommand recoveries are real: `read --jsno`, `capabilities --jsno`, `rn`→run, `search`→find.
- `feature_flags.intent_inference_aliases: true` **overclaims** global coverage (caps advertise aliases under run/capabilities; global root still clap-dies).
- robot-docs claims recoveries (`--jsno`, `--jason`, `rn`, …) that agents may expect everywhere.

**Merge:** **R-002 ∪ R-013** (quality gate: never suggest unrelated `--help` unless edit-distance family matches help).  
**Split residual:** verb-typo table (R-011) stays separate (reed→read suggests but does not act).

---

### 3. R-004 — Empty help + capabilities completeness (score 920 → **MODIFY ↑ P0**, split)

| Field | Judgment |
|---|---|
| Verdict | **Modify** — **uplift to P0**; split into two workstreams |
| Prior | P1 / 920 |
| Triangulated | **P0** / 990 for caps completeness; P1 for empty blurbs on experimental cluster |
| Why | Family cross-cut ranks **caps incomplete vs help** as #1 blast: agents that follow the advertised first command under-discover. |

**Evidence:**

```text
$ tokenzero capabilities --json | jq '.commands | length'
17
# names: read find recall fetch glob tree edit run expand mem pulse doctor install
#        hook claude-code capabilities codemode robot-docs guide

$ tokenzero --help | # blank descriptions
# 28 verbs: cache-pack bench mcp-server ... quote
```

Missing from caps but agent-relevant: `discover`, `ingest`, `rewrite`, `grep`, `search` (alias), `cache`, `init`, `clients`, `stats`, `session-open`, `mcp-server`.

**Contradictions with strengths:**

- Caps quality for the **included** 17 is excellent (schemas, exit_codes, dangerous_ops, env_vars, stdout_contract) — do not dilute; **generate full tree** with `stability: stable|experimental|internal`.
- robot-docs already documents `search` and recoveries; help/caps lag.

**Split:**

| ID | Scope | Priority |
|---|---|---|
| **R-004a** | Generate `capabilities.commands` from clap; mark stability; document all agent-primary verbs | **P0** |
| **R-004b** | One-line About for every remaining top-level command | P1 (or moot if R-015 hides them) |

**Merge with R-015:** empty-desc pollution and experimental gating are the same UX disease.

---

### 4. R-003 — Error-Teaches rewrite (score 900 → **AGREE P1**)

| Field | Judgment |
|---|---|
| Verdict | **Agree** P1 |
| Prior | P1 / 900 |
| Triangulated | **P1** / 910 |

**Evidence:**

```text
$ tokenzero read
EXIT:1
Error: read requires a path
# missing: tokenzero read <path> [--json]

$ tokenzero edit --force
EXIT:2
error: unexpected argument '--force' found
  tip: to pass '--force' as a value, use '-- --force'
# never teaches --dry-run or correct edit shape

$ tokenzero find --foo query
EXIT:2
... For more information, try '--help'.
```

**Contradictions with strengths:**

- When `--json` is present, many paths already emit structured `tokenzero.cli.v1` errors (good parseability).
- `expand badref` teaches prefix rules clearly (`ref must start with tz://...`) — positive control for pedagogy.

**Merge layer:** implement **central error formatter** that R-002/R-013/R-003 all call — one cookbook: (a) what failed (b) where (c) exact command. Do not ship three independent clap patches.

---

### 5. R-005 — Pin ergonomics contracts / regression_tests (score 880 → **AGREE P1**, schedule earlier)

| Field | Judgment |
|---|---|
| Verdict | **Agree** P1; **do not defer** until after feature work |
| Prior | P1 / 880 |
| Triangulated | **P1** / 900 |

**Evidence:**

```text
$ ls agent_ergonomics_audit/audit/regression_tests/
.gitkeep   # only
```

Scorecard: regression_resistance median **357** (lowest dimension). Caps re-run is deterministic (family X10 PASS) but **unguarded**.

**Contradictions with strengths:**

- Strengths exist **without pins**: caps schema, doctor JSON, install dry-run — any PR can silently break them.
- Audit already has probe dumps (`partial/*`) that are not executable golden tests.

**Merge:** R-005 tests are the **acceptance harness** for R-002/R-004a/R-001; land stubs first (schema keys, stdout purity, no TUI on bare root, `--jsno` after fix).

---

### 6. R-009 — MCP–CLI parity in capabilities (score 860 → **AGREE P1**)

| Field | Judgment |
|---|---|
| Verdict | **Agree** P1 |
| Prior | P1 / 860 |
| Triangulated | **P1** / 850 (slight demote vs CLI-first gaps) |

**Evidence:**

- Live `capabilities --json` has **no** `mcp_tools` / `mcp` parity map (`has mcp: False`).
- Pass-1 `parity_mcp_cli.md`: 20 MCP `tz_*` tools; CLI names diverge; `tz_grep` ≠ CLI `grep` semantics; `tz_batch` MCP-only; doctor/install/robot-docs CLI-only.
- `mcp-server` has **empty** top-level help blurb; `tokenzero mcp-server --help` works but is not agent-primary discovery.

**Contradictions with strengths:**

- Doctor already reports `mcp: {ready, server}` — partial presence signal.
- robot-docs CodeMode section correctly states same engine as MCP `tz_*`.

**Do not merge** with R-004a blindly: parity table is a **cross-surface** artifact; caps completeness is **CLI tree** completeness. Ship both fields: `commands[]` full tree + `mcp_tools[]` map.

---

### 7. R-015 — Gate experimental/eval verbs (score 840 → **MODIFY ↑ P1**)

| Field | Judgment |
|---|---|
| Verdict | **Modify** — **uplift P2→P1**; merge with R-004b |
| Prior | P2 / 840 |
| Triangulated | **P1** / 880 |

**Evidence:** 28 empty-desc top-level verbs (deep_probes2 + live help). Surfaces like `verb__harm-eval` weighted_mean **319.3**, self_documentation **107**. Primary agent path (read/find/run/doctor) is drowning in help scraping noise.

**Contradictions with strengths:**

- Core path help blurbs are good (read, find, doctor, capabilities).
- Hiding eval verbs **improves** discoverability of strengths without removing `TOKENZERO_EXPERIMENTAL=1` escape hatch.

**Merge:** **R-015 ∪ R-004b** — either fill *or* hide; never leave blank top-level noise.

---

### 8. R-008 — Dangerous-op surface expansion (score 820 → **AGREE P1**)

| Field | Judgment |
|---|---|
| Verdict | **Agree** P1 |
| Prior | P1 / 820 |
| Triangulated | **P1** / 830 |

**Evidence:**

```text
capabilities.dangerous_operations:
  - install  gate=--apply  safe=install --plan --json
  - cache prune gate=--apply safe=cache prune --json

edit --help: has --dry-run (opt-in), mutates by default; NOT in dangerous_operations
cache --help: migrate-refs / migrate-rollback / migrate-cleanup present
clients --help: rollback subcommand present
```

**Contradictions with strengths:**

- install + cache prune **safe-by-default** model is a flagship strength (safety_with_recovery high on those surfaces) — extend the *registry*, do not invent a second safety language.
- edit already has undo refs (recovery strength) but contract under-documents mutation gate asymmetry (`--dry-run` vs `--apply`).

---

### 9. R-013 — Wrong did-you-mean quality gate (score 800 → **MERGE into R-002**)

| Field | Judgment |
|---|---|
| Verdict | **Modify** — not standalone Top-8; quality gate inside R-002 |
| Prior | P1 / 800 |
| Triangulated | **P0-component** of R-002 |

**Evidence:**

```text
$ tokenzero --exlpain
EXIT:2
  tip: a similar argument exists: '--help'
# "explain" ≠ help

# Intent useful_hint=12 are almost exclusively --halp/--hlep/--hep/--exlpain → --help
# so "useful_hint" rate overstates good pedagogy
```

**Contradiction:** clap tip **is** correct for true help typos (`--halp`); gate must be family-aware, not "disable all tips".

---

### 10. R-010 — Doctor as default mega-path (score 780 → **MERGE into R-001**, keep as implementation vehicle)

| Field | Judgment |
|---|---|
| Verdict | **Modify** — preferred *implementation* of mega-path, not separate priority |
| Prior | P2 / 780 |
| Triangulated | **P1-implementation** of merged R-001∪R-010 |

**Evidence:** bare doctor already multi-slice; missing discoverability as "robot-triage shape" and `recommendations[]`/`commands[]` fields. Doctor weighted_mean **712.9** (top tier).

**Contradiction:** promoting doctor without fixing caps (R-004a) still leaves discovery incomplete.

---

## Overlap map (merge/split)

```
                    ┌─────────────────────────────┐
   R-004a (P0) ────┤ capabilities full clap tree  │
                    └───────────┬─────────────────┘
                                │ feeds
                    ┌───────────▼─────────────────┐
   R-001∪R-010 ────┤ mega-path / doctor triage    │  (P1)
                    └─────────────────────────────┘

   R-002∪R-013 (P0) ── intent recovery + DYM quality
          │
          ▼ shares formatter
   R-003 (P1) ── Error-Teaches exact invocation

   R-015∪R-004b (P1) ── hide OR describe empty verbs

   R-005 (P1) ── pins all of the above

   R-008 (P1) ── dangerous_operations registry
   R-009 (P1) ── mcp_tools[] parity map
```

**Out of Top-8 (still open, lower blast or covered):** R-006 search alias docs, R-007 exit-code audit, R-011 verb typos, R-012 JSON-everywhere matrix, R-014 robot-help aliases (**mostly done** live).

---

## Strengths to preserve (do not regress)

| Strength | Evidence | Implication |
|---|---|---|
| `capabilities --json` contract | keys: exit_codes, dangerous_operations, output_schemas, agent_next_steps, feature_flags | Extend; never replace with markdown-only |
| `robot-docs` / `robot-help` / `--robot-help` | all EXIT 0 guide | Keep aliases; R-014 nearly closed |
| Doctor multi-slice JSON | bare doctor EXIT 0 | Prefer enhance doctor for mega-path |
| Subcommand `--jsno` recovery | read/capabilities EXIT 0 | Expand, do not remove islands |
| install/cache prune `--apply` gate | caps + live dry_run | Template for R-008 |
| `search` alias works | EXIT 0 | Document in help (R-006) |
| Caps determinism | family X10 PASS | Pin with R-005 |

---

## Residual gap list (lowest dimensions)

### intent_inference (median 390)

| Gap | Evidence | Linked recs |
|---|---|---|
| Global flag typos clap-die | `--jsno/--jsonn` EC2; 168/180 useless | R-002 |
| Bad DYM tips counted as "useful" | `--exlpain`→`--help`; 12/180 hints | R-013→R-002 |
| Verb typos suggest but do not act | `reed` tips `read` but EC2 | R-011 residual |
| Caps overclaim aliases | `intent_inference_aliases: true` + partial coverage | R-004a honesty field |
| Corpus bias | sample is global-only category A | widen savvy corpus residual |

### error_pedagogy (median 427)

| Gap | Evidence | Linked recs |
|---|---|---|
| Missing exact invocation | `read` → "requires a path" only | R-003 |
| Clap "pass as value" tips | `edit --force` | R-003 |
| Four pedagogies coexist | clap / plain Error / JSON / codemode:error (family X5) | R-003 + envelope residual |
| Missing-arg exit split | read EC1 vs find EC2 | R-007 residual |
| Shell false success | `run --json -- false` process EC0, `status=ok` | family X11 residual (not in top10) |

### regression_resistance (median 357)

| Gap | Evidence | Linked recs |
|---|---|---|
| Empty `regression_tests/` | only `.gitkeep` | R-005 |
| Caps schema unpinned | 17 commands can drift | R-005a |
| Intent recovery unpinned | islands can regress silently | R-005c after R-002 |
| No stdout/stderr purity harness | audit probes only | R-005b |

### self_documentation on empty verbs (min 107)

| Gap | Evidence | Linked recs |
|---|---|---|
| 28 blank help blurbs | live `--help` | R-004b / R-015 |
| Caps 17/57 under-export | capabilities commands | R-004a |
| `mcp-server` blank + no mcp_tools in caps | help + caps | R-009 + R-015 |
| Audit/*-eval cluster pollution | scorecard lowest 25 all empty verbs | R-015 |
| `search` in robot-docs not in help list | deep_probes2 + guide | R-006 residual |

### Additional residual (not in original top 10, high blast)

| Gap | Source | Suggested future rec |
|---|---|---|
| Envelope polymorphism (7+ schema families) | family X2 | R-016 Envelope v2 |
| `feature_flags.codemode_surface: true` while binary lacks rquickjs | family X12 + live codemode EC1 | R-017 honest feature_flags |
| `tool` field drift (grep vs find, shell vs run) | family X8 | R-018 canonical tool + invoked_as |

---

## Final ranked Top 8 (after triangulation)

| Rank | ID | Title | Label | Score | vs pass-1 |
|---:|---|---|---|---:|---|
| 1 | **R-004a** | Complete `capabilities.commands` from clap (+ stability tags) | **P0** | 990 | ↑ split/uplift from R-004 |
| 2 | **R-002∪R-013** | Intent recovery + did-you-mean quality gate | **P0** | 960 | =/merge |
| 3 | **R-003** | Error-Teaches: exact corrected invocation | **P1** | 910 | = |
| 4 | **R-005** | Pin contracts in `regression_tests/` (ship with #1–2) | **P1** | 900 | schedule earlier |
| 5 | **R-015∪R-004b** | Gate/hide empty experimental verbs OR fill blurbs | **P1** | 880 | ↑ from P2 |
| 6 | **R-009** | Export MCP tool map + parity notes in capabilities | **P1** | 850 | slight demote vs CLI gaps |
| 7 | **R-008** | Expand `dangerous_operations` (edit, cache migrate*, clients rollback) | **P1** | 830 | = |
| 8 | **R-001∪R-010** | Mega-path via doctor/capabilities composition | **P1** | 820 | ↓ from P0 |

### Explicit demotions / drops from playbook Top 10

| Rec | Action |
|---|---|
| R-001 as standalone P0 mega-command | Demote; implement via doctor composition |
| R-013 standalone | Fold into R-002 |
| R-010 standalone | Fold into R-001 |
| R-004 undifferentiated | Split; 004a is new #1 P0 |

---

## Uplift notes (priority deltas)

See also: `pass-2/uplift_notes.md` and structured `triangulation/pass2_top10.json`.

| Rec | Delta | Reason |
|---|---|---|
| R-004a | **→ P0** | Discovery lie-by-omission beats missing mega-command |
| R-015 | **P2 → P1** | Empty-verb pollution is active agent noise |
| R-001 | **P0 → P1** | Strengths already cover multi-hop discovery |
| R-002 | stay **P0** | Confirmed 168/180 + live islands |
| R-013 | merge | Quality gate, not separate rank |

---

## Verification note

- All command excerpts re-run 2026-07-27 against `tokenzero 1.4.0`.
- Hard rule honored: **no** `cargo test --workspace`.
- ZeroStack was intermittently busy/timeout on large expands; native reads used for audit corpus after status confirm.
