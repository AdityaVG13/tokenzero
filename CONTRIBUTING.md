# Contributing

TokenZero is a public runtime for recovery-aware context compression.

## Public Scope

Contributions should stay inside the public product surface:
- Runtime code in `crates/`.
- Rust tests next to the owning crate.
- Public docs in `README.md` and `docs/`.
- Packaging metadata under `package/` and `packaging/`.

Do not submit secrets, machine-local state, generated caches, unpublished planning notes, model checkpoints, or personal filesystem paths.

Research attic (`formal/`, `Pareto/`, `beads_compliance_audit/`, `ubs_audit/`) is not the public product surface. `formal/` stays in the git tree for optional Cont-2 checkers; `git archive` and GitHub source tarballs omit it (`export-ignore`). The other attic trees are gitignored and local-only.

## Development

Use Cargo for public runtime work.

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Before a release branch is pushed, also run:

```bash
make release-check
```

For staged-only pre-commit checks:

```bash
git diff --check --cached
```

Use conventional commits, for example `fix: preserve exact refs across restart`.
