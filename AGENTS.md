# AGENTS.md -- TokenZero

> Guidelines for AI coding agents working in this codebase.

> TokenZero is the model-facing measurement authority: token accounting,
> projection, compression, exact expansion, telemetry honesty. It consumes
> hub contracts (zero-abi, mcp) and never imports FSZero or GraphZero.

> Sibling repos: ZeroStack (hub), FSZero, GraphZero - engines never import each other; the hub composes them.
> Daemonless: one session-owned sidecar, parent-death-bound -- never a machine-wide service.

> NEVER run full Cargo or RUSTC tests. Targeted Tests are law and only when code changes warrant as such.
> ALL tests live in the tests/ folder at the repository root (tests/unit/<crate>/).
> Crate-level integration tests are declared as [[test]] targets whose paths point into tests/unit/.
> Inline #[cfg(test)] unit tests inside src/ remain acceptable.


---

## ZeroKernel (`z`) - Preferred Execution Surface

Any agent working in this repo MUST try ZeroKernel commands before raw shell
for filesystem inspection, code search, structured edits, and effects.
Get the full surface with `z.help()` inside any cell.

### Methods

| Purpose | Commands |
| --- | --- |
| Read/inspect | `z.read(path)` - `z.snap(path \| {path, selection})` - `z.lookup(dir)` |
| Search | `z.asgrep(query, {mode, path})` - modes: natural, pattern, symbols, definition, references, callers |
| Mutate | `z.write(path, content)` - `z.edit(snap \| path, patch)` - `z.remove(path)` |
| Atomic multi-file | `z.effect({targets, changes, verify?})` |
| Orchestrate | `z.parallel([...])` - `z.pipeline(items, ...stages)` - `z.shell(argv)` |
| Tokens | `z.measure` - `z.project` - `z.compress` - `z.expand` |
| Durable state | `z.state.get/set/has/delete/list` |

### Execution surfaces

1. **ZMP**: built-in `zero` tool (preferred while dogfooding; fresh bounded frame per call)
2. **CLI**: `cargo build -p zero-kernel && ./target/debug/zero-kernel exec -C "$PWD" < cell.js`
3. **MCP**: `./target/debug/zero-kernel mcp` (stdio transport)

### Rules

- Compound work goes in ONE cell: read -> compute -> edit -> effect(verify).
  Failed cells roll back all effects and state automatically.
- Parallel tool calls are safe for disjoint targets. Same-file mutations
  serialize through typed Conflict errors - re-snap and retry.
- Snap-bound edits require byte-exact `find` of the snapped selection;
  prefer `z.edit(snap, {replacement})` to swap a whole selection.
- Large reads return labeled outlines plus an exact handle -
  `z.expand(handle, {selector})` recovers exact bytes. Never write an
  outline back to disk.

---

## RULE 0 - THE FUNDAMENTAL OVERRIDE PREROGATIVE

If I tell you to do something, even if it goes against what follows below, YOU
MUST LISTEN TO ME. I AM IN CHARGE, NOT YOU.

---

## RULE 0.1 - VALUE DELIVERY OVER PROCESS: NO PROCESS PORN

ZeroStack is an evolving-project. Agent time, 
user time, tokens, review attention, and repository complexity are scarce. The
default job is to implement the requested capability or fix the concrete defect,
not to elaborate the machinery surrounding the work.

**Process is never the product unless the user explicitly asks for process
work.** Beads, plans, Agent Mail, audits, manifests, provenance, logging, CI,
test harnesses, dashboards, status reports, and agent coordination exist only to
support delivery. They must not become a self-perpetuating substitute for
delivery.

### The value test

Before doing non-product work, answer all three questions:

1. What concrete user-visible capability, correctness defect, or immediate
   implementation blocker does this work address?
2. Is this the smallest direct action that addresses it?
3. Will its likely value exceed its implementation, maintenance, review, and
   delay cost right now?

If any answer is unclear, **do not do the work**. Return to implementing the
requested functionality. Speculative future usefulness, elegance, completeness,
or “more confidence” is not enough.

### Hard scope limits

- A review request authorizes reading the relevant code, reproducing concrete
  defects, making the smallest sound fixes, and adding focused regressions. It
  does **not** authorize redesigning adjacent infrastructure, inventing a new
  analyzer, exhaustively hardening hypothetical cases, or chasing unrelated
  pre-existing failures.
- Never turn a small review or repair into hundreds or thousands of lines of CI,
  harness, analyzer, schema, logging, provenance, or planning code without the
  user's explicit approval for that expansion.
- If incidental support work is becoming comparable to or larger than the
  requested implementation, stop before expanding it. Report the concrete
  blocker and ask whether the user wants the support work. Do not rationalize
  continuing because time has already been spent.
- Do not build a validator for a validator, a harness for a harness, or an
  analyzer whose principal purpose is proving internal process artifacts unless
  that exact system is the requested deliverable.
- Do not chase a failure already present on `HEAD` unless it blocks the requested
  deliverable and the user authorizes broadening the scope. Record it briefly and
  continue or stop at the real boundary.
- Do not repeatedly re-audit, re-plan, re-hash, re-seal, poll, or add reviewers
  after the requested behavior has focused evidence. One coherent implementation
  plus proportionate verification is better than layers of ceremonial assurance.
- Do not spawn an agent swarm or subagents for a narrow task. Delegate only concrete,
  independent implementation or bounded verification that materially shortens
  the path to the requested result. Never create recursive review loops or wait
  repeatedly for agents with no deliverable in hand.

### Implementation must dominate

Unless the user explicitly requests planning, governance, CI, or tooling:

- Spend the dominant share of effort on working product code and direct tests of
  that product code.
- Prefer an existing test seam over creating a new framework. Tests should prove
  changed behavior, important boundaries, and named claims; exhaustive testing
  of internal ceremony is negative value.
- Keep logging actionable and proportional. “Great logging” means enough context
  to diagnose a real failure, not recording every intermediate state or building
  a second product around evidence collection.
- Treat Beads as concise execution bookkeeping. Once a Bead is sufficiently
  clear to implement safely, implement it. Do not spend hours optimizing issue
  prose, graph metrics, dependencies, or acceptance wording while its actual
  functionality remains absent.
- Run the narrowest relevant checks while iterating. Run broad DSR/repository
  gates only when there is a coherent implementation ready for that proof or the
  user explicitly asks for them.
- When choosing between missing P0 functionality and optional meta-infrastructure,
  implement the P0 functionality. Tooling wins only when it is a demonstrated
  blocker to that implementation.
  
### Mandatory checkpoint and stop rule

At the first sign of scope expansion, pause and state in plain language:

- the user-facing outcome being delivered;
- the files and approximate size of the proposed expansion;
- why the expansion is strictly necessary; and
- the smaller alternatives considered.

If that explanation cannot establish immediate net value in a few sentences,
**do not proceed**. Ask the user before expanding. A “fresh-eyes” instruction is
not permission for an unbounded expedition.

When work has drifted into process porn, stop immediately. Do not add more tests,
proof layers, or cleanup to justify the sunk cost. Freeze the tree, disclose the
exact state candidly, and wait for direction.

### Concrete anti-pattern

Adding slop tests, or slop code that don't have intent is a canonical failure. Adding LOC of code for tests is only absolutely necessary
if the test will meaningfully check whether the code should compile, tests for fluff is not wanted or needed. Never repeat it. 

---

## RULE NUMBER 1: NO FILE DELETION

**YOU ARE NEVER ALLOWED TO DELETE A FILE WITHOUT EXPRESS PERMISSION.** Even a
new file that you yourself created, such as a test file. You must always ask and
receive clear, written permission before deleting a file or folder of any kind.

---

## Irreversible Git & Filesystem Actions - DO NOT EVER BREAK GLASS

1. **Absolutely forbidden commands:** `git reset --hard`, `git clean -fd`,
   `rm -rf`, or any command that can delete or overwrite code/data must never be
   run unless the user explicitly provides the exact command and states, in the
   same message, that they understand and want the irreversible consequences.
2. **No guessing:** If there is any uncertainty about what a command might
   delete or overwrite, stop immediately and ask the user for specific approval.
3. **Safer alternatives first:** When cleanup or rollbacks are needed, use
   non-destructive inspection first: `git status`, `git diff`, backups, or
   explicit hand-written patches.
4. **Mandatory explicit plan:** Even after explicit user authorization, restate
   the command verbatim, list exactly what will be affected, and wait for
   confirmation that your understanding is correct.
5. **Document the confirmation:** When running any approved destructive command,
   record the user text that authorized it, the command actually run, and the
   execution time in your final response.

---

## Git Branch: ONLY Use `main`, NEVER `master`

When this directory is a git repository, the default branch is `main`.

- All work happens on `main`.
- Never create, switch to, or push feature branches unless the user explicitly
  overrides this file.
- Never reference `master` in code or docs. If you see it, treat it as a bug.
- If the remote also needs a legacy `master` ref, synchronize it from `main`
  only when the user or project automation asks for that exact operation.

---

## RULE 2: NO GIT BRANCHES. NO GIT WORKTREES. EVER.

`main` is the one and only branch. There is no "temporary" branch, no per-agent
branch, no per-task branch, and no scratch worktree.

### FORBIDDEN

- `git branch <anything-other-than-main>`
- `git checkout -b <foo>` or `git switch -c <foo>`
- `git worktree add ...`
- Pushing non-main refs to `origin`
- Creating pull requests or draft PRs from feature branches
- Working in scratch clones at paths like `/tmp/frankensim-*`,
  `/data/projects/frankensim-*`, or `~/projects/frankensim-*` to isolate work
- Using any tool or harness that creates branches or worktrees as a side effect

### WHAT YOU DO INSTEAD

- Commit directly to `main` when the user asks for commits and the work is ready.
- Keep unfinished work in the working tree.
- Coordinate through Agent Mail reservations when multiple agents are active.
- Use Beads issue IDs and file reservations as the isolation mechanism, not git
  branches.
- If another agent changed files, do not revert or stash their work. Work with
  the current tree.

---

## Project Truth Sources

This `AGENTS.md` is the operating contract for agents. The canonical technical constitution is `docs/zero-kernel.md`; the implementation program is `docs/internal/zero-kernel-cutover-plan.md`.

### ZeroKernel clean-cutover freeze

- V6 `zero.fs.*` / `zero.graph.*` / `zero.token.*`, `z.invoke`, raw workers, per-engine CodeMode/MCP catalogs, one-shot kernel children, and numeric generation-labelled APIs are noncanonical.
- Do not add features, aliases, tests, docs, or compatibility paths to those surfaces. Preserve only domain invariants being translated to direct `z.*` and typed engine contracts.
- The only model-facing execution surface is the daemonless in-process ZeroKernel direct `z` API.
- Spell the product `ZeroKernel`; Rust snake_case uses `zero_kernel`.

---

## Toolchain: Rust & Cargo

Use Cargo for Rust builds. Do not introduce another package manager unless the
user explicitly asks for a non-Rust subproject.

Expected baseline when the workspace exists:

- Rust 2024 edition.
- Cargo workspace with flat `zero-*` crates.
- `#![deny(unsafe_code)]` at crate or module level wherever practical.
- Explicit feature flags for frontier/moonshot capabilities.
- Release profile changes must justify the performance, determinism, and
  cancellation tradeoffs.

---

### Unsafe Code

Unsafe is a last resort and must be treated as an auditable boundary:

- Prefer safe Rust, const generics, ownership, and explicit layouts first.
- Keep unsafe leaves small, local, and behind safe facades.
- Candidate unsafe zones: SIMD microkernels, arena allocation internals, memory
  mapping, architecture dispatch, exact low-level layout handling.
- Each unsafe exception needs a documented invariant, tests, and preferably a
  ledger/contract entry once the repo has the relevant artifacts.
- Never use unsafe to paper over lifetime, aliasing, cancellation, or
  synchronization design problems.

---

## Performance Program

Performance claims must be roofline-aware and measurable. Do not write "fast"
unless there is a benchmark, target, machine fingerprint, and acceptance band.

---

## Code Editing Discipline

### No Script-Based Code Changes

Do not run broad regex or script-based code rewrites over source files. Make
code changes manually with focused patches. Use structured tools such as
`ast-sgrep` or fallback to `ast-grep` only when the pattern is genuinely syntactic and the diff can be
reviewed.

### No File Proliferation

Revise existing files in place unless a new file represents genuinely new
functionality or a required contract/test artifact.

Forbidden naming patterns:

- `main_v2.rs`
- `improved.rs`
- `new_version.rs`
- `final_final.rs`
- duplicate experimental copies of existing modules

### Backwards Compatibility

This project is early-stage. Prefer the correct design over compatibility shims.
Do not preserve bad APIs through wrappers unless the user explicitly asks for a
migration layer.

### Comments and Docs

- Document invariants, error models, determinism class, and no-claim boundaries.
- Avoid comments that merely restate code.
- In math-heavy code, include enough references or derivation notes for the next
  agent to verify signs, units, and assumptions.

---

## Output Style

Core library code should not print casually to stdout/stderr.

- Use structured tracing or ledger events for observability.
- CLI output, when added, must be deterministic and documented.
- Errors intended for agents should be structured and actionable.
- Diagnostics should include units, budgets, capability context, and suggested
  fixes when possible.

---

## Compiler Checks

After substantive Rust code changes, verify that the relevant checks pass.
DSR is the first choice for repo-level gates and release builds. Prefer RCH
only for narrow ad hoc Cargo probes or when DSR itself is unavailable.

---

## DSR - Required CI and Release Runner

GitHub Actions is not the CI source of truth for this repository. The account is
throttled/cut off, so agents must always use DSR in preference to GitHub
Actions for repo-level verification, release builds, and fallback release work.

- Use `dsr` if it is on `PATH`; 
- Use `dsr doctor` and `dsr health all` for DSR/host diagnostics.
- The default ZeroStack quality gate is `dsr quality --tool zerostack`.
  Operator config `~/.config/dsr/repos.yaml` runs one RCH-offloaded
  `cargo test -p zero-ref --test zeroref_api -- --test-threads=1`; never
  replace it with `cargo test --workspace`.

Do not wait on, poll, or cite GitHub Actions as required proof unless the user
explicitly asks for that. The workflow files are retained as manual specs and
historical gate documentation, not as automatic merge/release criteria.

When reporting verification, include the exact DSR command, pass/fail status,
and any run log or artifact path DSR prints. If DSR is unavailable, report the
exact blocker and then use RCH or local Cargo only as a clearly labeled fallback.

---

## Testing Policy

Tests scale with risk. Tests should be about intent, not fluff. 

### Unit Tests

Every module should include focused tests for:

- happy paths
- empty/boundary/max cases
- error conditions
- unit/dimension correctness where relevant
- deterministic tie-breaking

### Property and Metamorphic Tests

Use property tests for algebraic laws and invariants:

- chart conversion round trips within certified bounds
- adjoint identity checks
- exact-sequence identities
- rigid transform invariance
- unit-rescaling invariance
- refinement monotonicity
- interval containment under equivalent rewrites

### Concurrency and Cancellation Tests

Concurrency-sensitive code needs deterministic lab-runtime or model-checking
coverage:

- no task leaks
- no arena leaks
- loser branches drained
- cancellation latency bounded at tile boundaries
- pause/resume deterministic equivalence
- panic/fault containment propagates structured errors

### Golden Evidence

Golden artifacts should be deterministic, reproducible, and tied to contracts.
Do not regenerate golden files casually. If a golden changes, explain the
semantic reason and run the relevant verifier.

---

## Documentation and Contracts

Each crate should have a `CONTRACT.md` before it becomes a dependency target for
other crates. A contract should state:

- purpose and layer
- public types and semantics
- invariants
- error model
- determinism class
- cancellation behavior
- unsafe boundary, if any
- feature flags
- conformance tests
- no-claim boundaries

---

## Agent Workflow

### Start of Work

1. Read this file.
3. If present, read `README.md`, crate `CONTRACT.md`, and the relevant Beads
   issue.
4. Inspect the tree before editing.
5. Reserve files through Agent Mail if multiple agents are active and the tools
   are available.

### While Working

- Keep changes tightly scoped.
- Do not disturb unrelated files.
- Do not revert changes you did not make.
- Keep technical claims attached to tests, contracts, or no-claim language.
- Prefer small, reviewable patches.

### End of Work

Before ending a session:

1. Run applicable format/check/test lanes.
2. Report exactly what passed and what did not run.
3. If Beads is in use, update issue status and run `br sync --flush-only`.
4. Release Agent Mail file reservations if you made any.
5. Leave clear handoff notes for any blocker.

---

## MCP Agent Mail - Multi-Agent Coordination

When Agent Mail tools are available, use them for multi-agent coordination and
file reservations.

Typical same-repository flow:

1. Register identity for this project path.
2. Reserve files before editing.
3. Use the Beads issue ID or task name as the thread ID.
4. Send start/progress/completion messages for shared work.
5. Release reservations when finished.

Reservations are advisory, but in this project they are the isolation mechanism.
They replace branches and worktrees.

---

## Beads (`br`) - Issue Tracking

If `.beads/` exists, use Beads for task state and dependency tracking.

Useful commands:

```bash
br ready
br list --status=open
br show <id>
br update <id> --status=in_progress
br close <id> --reason "Completed"
br sync --flush-only
```

Conventions:

- Use Beads IDs in Agent Mail thread IDs and commit messages.
- Do not run bare interactive tools in automated sessions if robot/non-TUI
  modes exist.
- `br sync --flush-only` does not commit; stage and commit intentionally only
  when the user asks for commits or the workflow requires it.

---

## `bv` - Graph-Aware Triage

If `bv` is available and `.beads/` exists, use robot modes only. Bare `bv`
launches an interactive TUI and can block the session.

```bash
bv --robot-triage
bv --robot-next
bv --robot-plan
bv --robot-insights
```

Use `bv` for work selection and dependency insight. Use Agent Mail for
coordination and file reservations.

---

## `ubs` - Bug Scanner

Before committing code, run `ubs` on changed files when available:

```bash
ubs <changed-files>
ubs $(git diff --name-only --cached)
```

Fix true positives at the root cause and rerun on the affected files.

---

## RCH - Remote Compilation Helper

Use DSR first for repo-level gates. Use RCH for CPU-heavy Cargo probes when DSR
does not cover the needed check or when you are intentionally running a narrow
diagnostic:

```bash
rch exec -- env CARGO_TARGET_DIR="${RCH_TARGET_BASE:-${TMPDIR:-/tmp}}/rch_target_zerostack_check" cargo check --all-targets
rch exec -- env CARGO_TARGET_DIR="${RCH_TARGET_BASE:-${TMPDIR:-/tmp}}/rch_target_zerostack_test" cargo test --all-targets
```

**ALWAYS use `${RCH_TARGET_BASE:-${TMPDIR:-/tmp}}` as the base — never `${TMPDIR:-/tmp}`
or a bare `/tmp` — when you invent a new per-task target dir name.** 

Prefer reusing one of the two names above. If you truly need a distinct target dir
for an isolated task, still base it on `$RCH_TARGET_BASE` and delete it when done.

Do not rely on local fallback for heavy builds in a shared-agent environment
unless the user explicitly authorizes it.

Quick diagnostics:

```bash
rch doctor
rch status
rch queue
```

---

## Search Tools

 - Use `zero` or `zerostack_execute` if available. 
 - Use `ast-grep` or `asgrep` as a fallback
 
 Do NOT use broad scripted rewrites where a hand patch is safer. 
 
 ---
 
 <!-- bv-agent-instructions -->
 
 ## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`) for issue tracking and [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) (`bv`) for graph-aware triage. Issues are stored in `.beads/` and tracked in git.

### Using bv as an AI sidecar

bv is a graph-aware triage engine for Beads projects (.beads/beads.jsonl). Instead of parsing JSONL or hallucinating graph traversal, use robot flags for deterministic, dependency-aware outputs with precomputed metrics (PageRank, betweenness, critical path, cycles, HITS, eigenvector, k-core).

**Scope boundary:** bv handles *what to work on* (triage, priority, planning). `br` handles creating, modifying, and closing beads.

**CRITICAL: Use ONLY --robot-* flags. Bare bv launches an interactive TUI that blocks your session.**

#### The Workflow: Start With Triage

**`bv --robot-triage` is your single entry point.** It returns everything you need in one call:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

```bash
bv --robot-triage        # THE MEGA-COMMAND: start here
bv --robot-next          # Minimal: just the single top pick + claim command

# Token-optimized output (TOON) for lower LLM context usage:
bv --robot-triage --format toon
```

#### Other bv Commands

| Command | Returns |
|---------|---------|
| `--robot-plan` | Parallel execution tracks with unblocks lists |
| `--robot-priority` | Priority misalignment detection with confidence |
| `--robot-insights` | Full metrics: PageRank, betweenness, HITS, eigenvector, critical path, cycles, k-core |
| `--robot-alerts` | Stale issues, blocking cascades, priority mismatches |
| `--robot-suggest` | Hygiene: duplicates, missing deps, label suggestions, cycle breaks |
| `--robot-diff --diff-since <ref>` | Changes since ref: new/closed/modified issues |
| `--robot-graph [--graph-format=json\|dot\|mermaid]` | Dependency graph export |

#### Scoping & Filtering

```bash
bv --robot-plan --label backend              # Scope to label's subgraph
bv --robot-insights --as-of HEAD~30          # Historical point-in-time
bv --recipe actionable --robot-plan          # Pre-filter: ready to work (no blockers)
bv --recipe high-impact --robot-triage       # Pre-filter: top PageRank scores
```

### br Commands for Issue Management

```bash
br ready              # Show issues ready to work (no blockers)
br list --status=open # All open issues
br show <id>          # Full issue details with dependencies
br create --title="..." --type=task --priority=2
br update <id> --status=in_progress
br close <id> --reason="Completed"
br close <id1> <id2>  # Close multiple issues at once
br sync --flush-only  # Export DB to JSONL
```


### Workflow Pattern

1. **Triage**: Run `bv --robot-triage` to find the highest-impact actionable work
2. **Claim**: Use `br update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id>`
5. **Sync**: Always run `br sync --flush-only` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `br ready` shows only unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers 0-4, not words)
- **Types**: task, bug, feature, epic, chore, docs, question
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Session Protocol

```bash
git status              # Check what changed
git add <files>         # Stage code changes
br sync --flush-only    # Export beads changes to JSONL
git commit -m "..."     # Commit everything
git push                # Push to remote
```

<!-- end-bv-agent-instructions -->

