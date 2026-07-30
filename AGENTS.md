# AGENTS.md — TokenZero

> Operating contract for AI coding agents in this repository. Read completely before your first edit.

## RULE 0 — OPERATOR OVERRIDE
If the operator (Aditya) tells you to do something, even against this file, you listen. He is in charge, not you.

## RULE 1 — NO FILE DELETION
Never delete a file or directory without express written permission — even files you created. Ask first, every time.

## RULE 2 — GIT DISCIPLINE
- Branch: ONLY `main`. No feature branches, no worktrees, no `master`.
- NEVER: force-push, rebase published history, `reset --hard` shared state, `clean -fd`, amend pushed commits.
- Commit subjects: generic, conventional (`fix:`, `feat:`, `test:`, `chore:`). NEVER put bead ids, agent names, or session ids in commit messages.
- Pull before you start. Push when a logical unit is done and verified.

## RULE 3 — ONE WRITER PER REPO
Multiple agents work this codebase concurrently. Before editing:
1. `git status --porcelain` — if the tree is dirty with paths you did not touch, STOP. Another agent is mid-flight. Do not commit over them, do not stash their work. Back out and report.
2. Check `br show <bead>` — if assignee is not you and status is in_progress, it is NOT yours.
Watched mtimes advancing on files you did not edit = live rival writer. Halt.

## RULE 4 — TEST POLICY (HARD)
- NEVER run full `cargo test` / `cargo build` for the whole workspace on this machine.
- Targeted tests only, and only when the change genuinely needs them (most one-line changes do not).
- All compilation/tests go through RCH (remote compilation helper, DGX Spark):
  `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_<reposlug> cargo test -p <crate> <filter> -- --test-threads=1`
- A pre-existing failure is not yours to absorb: verify via `git stash` baseline, then file a bead for it.

## RULE 5 — BEADS ARE THE MEMORY
- `br show <id>` before starting; read description, notes, AND acceptance criteria — acceptance is the contract.
- Close only with evidence (commit sha + verification output). Never close unverified.
- Any defect you find en route — even trivial — gets its own bead with reproduction evidence. Never fix-and-forget silently, never leave it untracked.
- Blocked? Set status blocked with a note naming the exact blocker. Someone sweeps blocked beads to reopen them.

## RULE 6 — TOKEN EFFICIENCY (RACC)
This project's reason to exist is minimizing agent round-trips. Practice it:
- Refs first: results return durable, expandable refs; read them, expand only when needed, FIRST try.
- One-call discovery: search/query hits carry snap-to-file targets — `HIT <path>#L<start>-L<end> kind=<k> sym=<enclosing>` with content inlined (sub-4KB never preview-only). Grammar: FSZero docs/design/target-ref-grammar.md. Do not invent a second grammar.
- Batch shell work into single calls; write reports to files, keep chat output tight.
- If the substrate itself wastes your round-trips (missing ref, preview-only small result, silent undefined, exit-0 error JSON), that is a BUG: file a bead in the owning repo.

## THE RACC CONTRACT (what every change must preserve)
1. Honesty: billed/visible token accounting is a receipt, never an estimate presented as fact.
2. Determinism: same op => same bytes across every surface/adapter (CLI, MCP, CodeMode, raw-worker).
3. Durability: a ref handed to an agent survives process restart and expands from any session.
4. Loud failure: errors are typed and expandable; never silent undefined, never exit 0 with error JSON on stdout.
5. Certificates over vibes: lossy/compressed output must carry certification; uncertified lossy presents as expandable, never as a committed result.


## THIS REPO — TokenZero (token/compression engine)
COORDINATION WARNING: this repo is frequently owned by a dedicated session. Check `git status` and in_progress bead assignees before ANY edit; if another agent is active, back out.

Workspace crates: tokenzero-engine (raw_worker_v2_protocol, workspace resolution), tokenzero-recovery (shared_cas.rs incl. zerostack.cas-gc.v1 GC, embedded/segment stores), tokenzero-mcp (codemode store), tokenzero (CLI, zerostack_store.rs). RCH target dir: /tmp/rch_target_tokenzero.

### RACC role
TokenZero is the accounting engine: billed/raw/recovery/visible token honesty is its core receipt guarantee.
- No double tokenization; no cumulative counters that double-charge recovery.
- Lossy compression is a GATE decision (hub zero-gate): uncertified lossy must present as expandable, never as a committed result. A lossy declaration is not a certification.
- Re-expansion charges recovery tokens; T8 replay identity must hold across expand/re-expand cycles.

### Known defect areas (beads exist — check before touching)
- Shared store has NO project namespacing: <store>/tokenzero/recovery-cache.json collides across projects (bead ljx; blocked on hub sce contract). Do not entrench the current layout.
- Literal '~' directory at repo root held misplaced store data (bead 2r6) — never resolve store roots with unexpanded '~'.
- Inline threshold (pn93) and raw-worker revision-abort (3nig) are the top round-trip taxes on live agents; treat both as contract bugs.
- Shell/compact runtime result envelopes are richer than their typed declarations (bead u28 family) — converge on hub zero-result/v1, do not add a fourth shape.
