# Implementation bead distillation — paste prompt (powerful coding model)

**You create the beads** (via `br` / project tracker). The operator does **not** want a prior agent to have created them.  
**You do not** invent new RADC math. You distill **frozen** theory + RACC product docs into an **implementation graph**.

```text
# RADC → TOKENZERO IMPLEMENTATION BEADS (distill only)

You are a senior engineer + beads author. Your ONLY job is to read the attach
pack and produce a COMPLETE implementation bead graph for TokenZero so humans
or coding agents can implement recovery-aware compression WITHOUT redoing
Wave-1–5 research.

DO NOT prove new theorems.
DO NOT open MDC dual-track math debates.
DO NOT claim 99.9% compression.
DO create beads with acceptance tests, deps, and explicit file/crate targets.

════════════════════════════════════════════════════════
0. READ ORDER
════════════════════════════════════════════════════════
1) 00_RADC_FORMAL_CORE_V1_FREEZE.md          — what is frozen vs not
2) 01_README_OPEN_FIRST.txt
3) 02_IMPL_DISTILL_BRIEF.md                 — product KPI + epic shape
4) 03_racc-public.md                        — product contract
5) 04_RACC_RESEARCH_DISTILL.md
6) Sol Pro Cont-2 package (05–)             — strongest finite theorem + checkers
7) Wave-4 excerpt / Sol Pro theory index as needed for corridor parameters
8) Grok conflict matrix (MDC dual-track reminder only)

If something is OPEN in the freeze, create a bead as "blocked on formal" or
"measurement pilot" — do not invent a proof.

════════════════════════════════════════════════════════
1. PRODUCT GOAL (return, not moonshot)
════════════════════════════════════════════════════════
Implement recovery-aware context compression in TokenZero such that:

  KPI = recovery-adjusted tokens per SUCCESSFUL task
      = visible_tokens + expand_tokens + weighted(fail/retry)

wins against baseline "paste full context" on a fixed pilot suite, at equal or
better task success / anchor recall.

Secondary: never-wrong-bytes expands; typed dangling refs; opaque handles
(not raw content hashes as "opaque").

════════════════════════════════════════════════════════
2. EPIC SHAPE (create these epic families; split into P0–P2 tasks)
════════════════════════════════════════════════════════
E0 — Spec freeze import
  - Import Formal Core v1 into docs/spec (RADC_FORMAL_CORE_V1.md in repo docs
    or Pareto→docs path operator chooses)
  - Statement lock: ledger M/L/D definitions matching code counters
  - Forbidden claims list

E1 — Telemetry / ledger (must ship before "wins")
  - Instrument visible token counts, expand token counts, fail/retry, success
  - Emit per-task recovery-adjusted cost
  - Golden tests: ledger identity on fixtures

E2 — Exact-ref / CAS path (EDC-style)
  - Opaque handle emission (random alias or non-content-leaking id)
  - Private map alias → payload (CAS/content hash internal only)
  - Expand API: selector → bytes; dangling-ref typed error; never wrong bytes
  - Unit tests for opacity regression (visible transcript must not contain
    raw payload when mode=exact-ref)

E3 — Capsule policy v1
  - Policy: prefer exact-ref for local payloads above size threshold
  - Demand-aware expand hooks (tool/agent requests slice/bit/range)
  - Config: rho_fail, lambda_fail, handle cost estimates (measured later)

E4 — Pilot harness
  - Fixed task suite (file-heavy agent tasks)
  - A/B: baseline paste-full vs exact-ref policy
  - Report: success rate, RATC, expand rate, Pareto plot data (CSV/JSON)
  - Promotion gate: only promote if RATC improves without success regression

E5 — Corridor measurement
  - Measure real h_tau, q_tau, c_tau on pilot tokenizer
  - Compare to formal rho*(s) inequalities as ADVISORY (not hard ship block
    until validated)
  - Document gaps between formal gauge and product

E6 — Caching layer (recovery-aware cache)
  - Reuse CAS entries across turns/tasks
  - Eviction + dangling semantics aligned with RACC public contract
  - Optional later: predictive pre-warm (P2/P3, marked speculative)

E7 — Certificate regression (optional P2)
  - Vendor Cont-2 Python/C++ checkers into CI or xtask for formal regression
  - Do not block product on general-n open problems

E8 — Docs / operator
  - How to read RATC reports
  - What we do NOT claim (99.9% always, global optimality)

════════════════════════════════════════════════════════
3. BEAD QUALITY BAR
════════════════════════════════════════════════════════
Every bead MUST have:
- clear title (imperative)
- type: feature|task|bug|chore|docs
- priority 0–4
- description with: context, acceptance tests, out-of-scope
- dependencies (blocks / depends-on)
- suggested paths (crates/modules) when guessable from TokenZero layout
- proof_status tag if it cites a theorem: PI|DR|EC|BE|SB|PRODUCT

Use the project's bead tool (`br` / beads_rust) if available:
  br create --title="..." --type=... --priority=...
  br dep add ...
  br update ... 
Do NOT hand-edit .beads/issues.jsonl.

If br is unavailable, write a complete markdown bead queue:
  impl-beads/P0-....md ... with the same fields, ready for import.

════════════════════════════════════════════════════════
4. OUTPUT
════════════════════════════════════════════════════════
A) Create beads (preferred) + print `br list` / ready summary
B) Also write:
   - IMPL_BEAD_GRAPH.md (human map of epics → beads → deps)
   - IMPL_PILOT_SUITE.md (proposed 10–20 tasks for E4)
   - IMPL_RISKS.md (theory/product mismatch risks)

Spend the time to make the graph complete enough that a coding agent can
execute P0 without asking "what is RADC?" again.

Begin. Distill freeze → beads. No new math.
```
