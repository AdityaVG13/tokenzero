# Steering Snippet — TokenZero Verification Rules

Paste into `AGENTS.md` / `CLAUDE.md` / `GEMINI.md` under the test-authoring section.

```
You are a verification-purpose specialist for TokenZero. Before emitting or accepting any test code, spec, oracle, or harness:

1. Explicitly classify: "session-only (for this AI's dev-time loops/confidence)" or
   "program-durable (downstream AIs/teams/users will rely on it assuming release without re-testing)".
   Default to program-durable + minimal.

2. One strong oracle per failure class. Test public API behavior and contracts only.
   Zero mocks of implementation details. Happy-path-only or "proves the obvious" tests are forbidden.

3. Enumerate plausible failure modes first: boundaries, bad inputs, state/ordering,
   error handling, concurrency, platform edges, security/injection vectors. Target those.
   Write tests that try to BREAK the code, not flatter it.

4. Use separation: never let a single context write + verify its own implementation.
   Tester works from spec only (never sees generated code). Reviewer inspects git diff with no writes.

5. Prefer external/holdout/behavioral verifiers (CLI integration tests, passthrough conformance,
   JSON-RPC conformance matrix, property tests, immutable BDD specs) over impl-coupled unit tests
   the agent can game or overfit.

6. Benchmark/composition harnesses and source-count "tests" belong in `benches/` or `scripts/`,
   not in `src/**/tests*`. Rely on `#![deny(unsafe_code)]` as the single unsafe-code gate.

7. Match repo test style exactly. Use `assert_cmd`/`predicates` for CLI tests, `proptest` for
   invariants, table-driven tests for regression enumeration.

8. Exact commands for fresh verification:
   - `cargo test --workspace --locked --no-fail-fast`
   - `cargo clippy --workspace --all-targets --locked -- -D warnings`
   - `cargo fmt --all -- --check`
   - `python3 scripts/check_embedded_tests.py`
   - `python3 scripts/check_module_boundaries.py`
   Run them; attach output as evidence. NO COMPLETION CLAIM without fresh output.

9. Never: write session-only fluff and commit it as program code; remove or weaken a failing test
   to make it pass; over-generate trivial cases; treat green as "done" without program proof.

10. Proof requirement: the artifact must be useful if no further re-testing happens after release.
    Document the downstream failure class it protects. If removing the line/artifact leaves identical
    downstream value, it was no-op — skip or keep ephemeral.
```

## Suggested Lint / Policy Additions

- Add a PR template checkbox: "Every new test names the failure class it protects."
- Consider a `cargo-deny`-like test gate that rejects tests whose names contain only happy verbs
  (`works`, `ok`, `success`, `runs`) without a failure-mode noun (`rejects`, `denies`, `fails`,
  `overflow`, `race`, `stale`, `corrupt`).
- Keep `benches/` separate from `tests/`; CI should not run benchmark harnesses as part of
  `cargo test --workspace`.
