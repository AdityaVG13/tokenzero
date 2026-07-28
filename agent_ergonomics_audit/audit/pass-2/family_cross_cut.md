# Family Cross-Cut Audit (Phase 1/4) -- TokenZero agent ergonomics

**Target:** `/Users/aditya/AI/TokenZero`  
**Binary:** `tokenzero` 1.4.0 (`~/.local/bin/tokenzero` -> `target/release/tokenzero`)  
**Mode:** audit-only (no product code changes)  
**Date:** 2026-07-27  
**Inputs:** `partial/deep_probes.txt`, `partial/deep_probes2.txt`, `partial/tz_caps.json`, `surface_inventory.jsonl`, `intent_inference_corpus.jsonl`, live CLI probes (`partial/family_cross_cut_probes*.txt`)

Scope: CLI primary; codemode trampoline; MCP presence via `mcp-server` / install surfaces (not a live MCP session).

---

## Method

Ten consistency dimensions. Evidence = live process exit codes + stdout/stderr shapes + `capabilities --json` contract + help inventory + intent corpus sample (n=180, all category A global-flag typos).

Determinism check: two back-to-back `tokenzero capabilities --json` were **byte-identical** (`CAPS_IDENTICAL:yes`).

---

## Dimension scoreboard

| # | Dimension | Verdict | Severity peak |
|---|---|---|---|
| 1 | Flag spelling parity (`--json` / `--robot-json` / mixed) | **FAIL** -- only `--json`; `--robot-json` never accepted; global `--json` also fails | P1 |
| 2 | Exit-code dictionary (0/1/2) | **PARTIAL** -- dictionary declared; usage errors split 1 vs 2; shell process failure still process-exit 0 | P1 |
| 3 | Capabilities schema vs `--help` verbs | **FAIL** -- 17 caps entries vs 57 help verbs; 40 help-only verbs invisible to agents using contract | P0 |
| 4 | Output envelope shape | **FAIL** -- 7+ schema families; missing common keys; codemode uses `schema` not `schema_version` | P0 |
| 5 | Error message pedagogy | **FAIL** -- clap default / plain Error / JSON envelope / codemode:error mixed | P1 |
| 6 | Intent-inference unevenness | **FAIL** -- subcommand `--jsno` recovers; global `--jsno` clap-dies; corpus 168/180 useless | P0 |
| 7 | Dangerous-op gating | **PARTIAL** -- install/cache prune `--apply` safe-by-default; `edit` mutates without gate in `dangerous_operations` | P1 |
| 8 | Naming (find/grep/search; init/install; robot-*) | **PARTIAL** -- aliases work but help/docs/tool-field disagree | P2 |
| 9 | Empty help descriptions | **FAIL** -- 28 top-level verbs blank in `--help` | P2 |
| 10 | Determinism (capabilities re-run) | **PASS** | -- |

---

## Detailed findings (cross-cut)

### X1 -- Capabilities is a curated subset, not the CLI contract (D3)

- Help top-level verbs: **57**
- `capabilities --json` command entries: **17** (`read find recall fetch glob tree edit run expand mem pulse doctor install hook-claude-code capabilities codemode robot-docs-guide`)
- **40 help-only** verbs agents never see via the advertised discovery path, including agent-relevant: `discover`, `ingest`, `session-open`, `rewrite`, `stats`, `clients`, `init`, `cache`, `grep`, `mcp-server`, session-ledger, and the entire audit/*-eval cluster.
- `output_schemas` only documents `capabilities` + `run` -- not doctor/install/cache/pulse/cli.v1/codemode.
- Blast: every agent that starts with `tokenzero capabilities --json` (the canonical first command) under-discovers the real surface.

### X2 -- Envelope polymorphism breaks uniform parsers (D4)

Observed `schema_version` / envelope families on live probes:

| Surface | schema | `status` | `tool` |
|---|---|---|---|
| read/find/mem/discover/expand(err+json) | `tokenzero.cli.v1` | ok/error | verb name |
| run | `tokenzero.cli.v1` | **ok even when command fails** | **`shell`** (not `run`) |
| grep | `tokenzero.cli.v1` | ok | **`grep`** (not `find`) |
| search | `tokenzero.cli.v1` | ok | `find` |
| capabilities | `tokenzero.capabilities.v1` | (none) | `tokenzero` |
| doctor | `tokenzero.doctor.v1` | ok | `tokenzero` |
| doctor status | `tokenzero.doctor.health.v1` | ok | **missing** |
| install/init plan | `tokenzero.install_plan.v1` | `planned` | missing |
| cache prune | `tokenzero.cache.v1` | ok | missing |
| pulse/stats | `tokenzero.pulse.v1` | ok | missing |
| clients detect | `tokenzero.clients.v1` | `mixed` | missing |
| codemode | field **`schema`**=`tokenzero.codemode.v1` (not schema_version) | error | missing; uses `visible_ack` |

No single required key set. Agents cannot `jq -e '.status=="ok" and .tool'` across the family.

### X3 -- Intent inference is island-shaped (D6 + intent corpus)

Documented recoveries that **work**:

- `tokenzero read|find|run ... --jsno` -> treats as `--json` (EC 0)
- `tokenzero capabilities --jsno|--jason`, `capability`, `capabilites` -> capabilities (EC 0)
- `rn`/`shell` -> run; `doctor status|statuz`, `pulse stats`, `cache statuz`, `install plan|status`
- `robot-help`, `--robot-help`, `robot-doc guide`, `robotdocs guide`
- `run --timout N` recovers; `tokenzero --timout` does **not**

Documented recoveries that **fail** (clap EC 2, no teach):

- Global: `tokenzero --jsno`, `--jason`, `--json`, `--robot-json`, `--jsonn`, `--cachepath`, most edit-distance typos
- Intent sample: **168/180** `useless_error`, **12/180** `useful_hint`, **0** exit-0 recoveries (all sampled as bare global flags)

Caps list `--jsno`/`--jason` under **both** `run` and `capabilities` aliases -- overclaims global recovery.

### X4 -- Exit-code dictionary vs implementation (D2)

Declared: 0 success, 1 blocked, 2 usage.

Live splits for "missing required arg":

| Invocation | EC | Shape |
|---|---|---|
| `find` / `edit` (clap required) | **2** | clap usage |
| `read` / `run` / `expand` (custom) | **1** | `Error: X requires ...` plain text |
| unknown flag/subcommand | **2** | clap |
| path_not_allowed / invalid_ref | **1** | JSON if `--json`, else plain |
| `run --json -- false` | **0** | JSON `status=ok`, `telemetry.command_success=false` |
| `hook claude-code` empty stdin | **0** | fail-open (documented) |
| `codemode` (no sandbox in this artifact) | **1** | plain or JSON error |

Label `blocked` for EC1 is overloaded: true policy blocks **and** missing-arg usage **and** runtime feature-missing share code 1.

### X5 -- Error pedagogy not Error-Teaches-first (D5)

Four pedagogies coexist:

1. **Clap default** -- `error: unexpected argument... For more information, try '--help'.` (often no canonical recovery)
2. **Custom plain** -- `Error: run requires a command after --` / `error: ref must start with tz://...`
3. **JSON envelope** -- `tokenzero.cli.v1` + `error.code` (only when `--json` or default-json paths)
4. **Codemode** -- `codemode:error 9 ops=0 ...` or `{"schema":"tokenzero.codemode.v1",...}`

Same fault class differs by flag: `expand badref` (plain) vs `expand badref --json` (envelope). Agents that always pass `--json` get better structure than those that do not.

### X6 -- Dangerous-op gating asymmetric (D7)

Caps `dangerous_operations`:

- `install` gated by `--apply`; bare/`--plan`/`--json` => `dry_run:true`, status `planned` (safe) -- **good**
- `cache prune` gated by `--apply`; bare => `dry_run:true` -- **good**

Gaps:

- `edit` is `mutates:true` in caps but **absent** from `dangerous_operations`
- edit gate is opt-in `--dry-run`, not opt-in `--apply` (inverse of install/prune)
- No safe default plan mode for edit (must supply edits; apply is default)
- `init` also has `--apply`/`--plan` but is not listed under dangerous_operations (inherits install behavior)

### X7 -- Flag spelling: no `--robot-json`, global `--json` missing (D1)

- Canonical machine flag is **`--json` only** (83 inventory flag nodes).
- `--robot-json` rejected everywhere (capabilities, read, global) -- EC 2 clap.
- Global `tokenzero --json` rejected (no default command) -- EC 2.
- Robot discovery uses **`--robot-help` / `robot-help` / `robot-docs`** (not robot-json).
- Doctor inventory surfaces a dead `--robot-triage` flag family (bv bleed); top-level `tokenzero --robot-triage` / `robot-triage` fail.

### X8 -- Naming drift across help / caps / runtime tool field (D8)

| Concept | Help | Caps | Runtime `tool` | Robot-docs |
|---|---|---|---|---|
| search | **not** listed top-level | alias of find | `find` | advertised as first-class |
| grep | listed "Alias for find" | alias of find | **`grep`** | mostly silent |
| find | listed | primary | `find` | listed |
| run / shell | run listed | run; aliases shell/rn | **`shell`** | run |
| init / install | both listed; init "compat alias" | install only | install_plan schema | install |
| robot-docs / robot-help / --robot-help | robot-docs listed | robot-docs guide + aliases | n/a (markdown) | all three |

### X9 -- Empty help description cluster (D9)

28 top-level commands with **blank** help descriptions (live `tokenzero --help`):

`cache-pack, bench, mcp-server, mcp-smoke, mcp-soak, exact-recovery-shell, exact-recovery-audit, harm-eval, protected-anchor-audit, false-success-shell, repo-inventory, prompt-cache-pack, install-smoke, package-audit, shell-matrix, os-reach-audit, os-release-artifact, one-shot-eval, source-currency-audit, adapter-approval-audit, adapter-approval-template, claim-audit, completion-audit, security-privacy-audit, artifact-handoff, reach, ws-skeleton, quote`

These are mostly internal/audit surfaces but still pollute agent help scraping and inflate "mystery verb" rate.

### X10 -- Determinism (D10)

**PASS.** Capabilities output is stable across re-runs (same version 1.4.0, 17 commands, 2 dangerous ops). No non-determinism finding for the contract blob itself. (Pulse/stats counters change -- expected, out of scope for capabilities.)

### X11 -- Status-truth vs exit dictionary tension (D2/D4 cross)

`tokenzero run --json -- false` (and `exit 7`): **process exit 0**, envelope `status: "ok"`, `tool: "shell"`, truth only in `telemetry.command_success` / visible capsule. Caps `exit_codes[0].meaning` = "The requested command completed" is true for TokenZero transport but false for agent intuition about the inner command. `output_schemas.run` documents `telemetry.command_success` -- good but easy to miss when agents only check process EC / top-level status.

### X12 -- Codemode / MCP family seam (cross-surface)

- This binary: CodeMode JS sandbox **not compiled** (`missing feature surface-codemode / rquickjs`); CLI `codemode` errors EC 1.
- Caps still advertise `codemode_surface: true` and full codemode block with Tier B trampoline docs.
- MCP is a separate process (`mcp-server` blank help); install plans MCP writes; robot-docs mention `resource://tokenzero/codemode`.
- Envelope for codemode errors uses different key names (`schema`, `visible_ack`) from CLI (`schema_version`, `ack`).

---

## Top 12 by blast radius

Ranked for agent failure surface area (how many agent codepaths / how often / how wrong the success signal).

1. **P0** Caps incomplete vs help (X1) -- discovery lies by omission  
2. **P0** Envelope polymorphism (X2) -- universal JSON parsers break  
3. **P0** Intent islands / global typos dead (X3) -- first-token agent mistakes unrecoverable  
4. **P1** Exit-code dictionary mismatch (X4) -- retry/branch logic wrong  
5. **P1** Error pedagogy mix (X5) -- cannot teach from one template  
6. **P1** Status-truth vs `status=ok`/EC0 (X11) -- silent false success on shell  
7. **P1** Dangerous-op edit ungated in contract (X6) -- mutate asymmetry  
8. **P1** Flag spelling: no `--robot-json`, global `--json` dead (X7)  
9. **P1** Caps advertise codemode while binary lacks sandbox (X12)  
10. **P2** Naming tool-field drift find/grep/search/run/shell (X8)  
11. **P2** Empty help cluster 28 verbs (X9)  
12. **P2** doctor schema split (doctor.v1 vs doctor.health.v1; tool missing) -- subcase of X2  

Positive control: **capabilities determinism PASS** (X10).

---

## Recommended fix themes (sketches only)

1. **Contract completeness gate:** generate `capabilities.commands` from clap command tree; mark `stability: stable|experimental|internal`; never silently drop agent-facing verbs.
2. **Envelope v2:** require `schema_version`, `status`, `tool` (canonical verb), `exit_class` on all JSON; codemode rename `schema`->`schema_version`, `visible_ack`->`ack`.
3. **Global intent pre-parse:** apply typo map before clap at argv[1] for `--jsno/--jason/--json/--robot-help/--robot-json` -> redirect to capabilities or robot-docs with stderr teach line.
4. **Exit dictionary:** missing required args always EC 2; policy/IO always EC 1; document shell transport EC 0 + `command_success` as the only truth for run.
5. **Dangerous ops table:** add `edit` with `mutation_gate: default-apply` + recommend `--dry-run`; keep install/prune `--apply`.
6. **Alias tool field:** always emit canonical `tool` (`find` for grep/search; `run` for shell/rn) plus `invoked_as`.
7. **Help descriptions:** fill or hide internal audit verbs behind `tokenzero dev` / feature flag.
8. **feature_flags honesty:** `codemode_surface` should reflect compile-time presence or report `available:false` in capabilities.

---

## Probe artifacts

- `agent_ergonomics_audit/audit/partial/family_cross_cut_probes.txt`
- `agent_ergonomics_audit/audit/partial/family_cross_cut_probes2.txt`
- `agent_ergonomics_audit/audit/partial/family_cross_cut_probes3.txt`
- `agent_ergonomics_audit/audit/partial/family_cross_cut_findings.jsonl`
