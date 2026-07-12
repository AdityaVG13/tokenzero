# Store-Root Precedence (Frozen for ZeroRef v1)

> Bead: `tokenzero-zeroref-v1-shared-cas-cqr.6`
> Source: `crates/tokenzero-mcp/src/workspace.rs`

## Cache-Path Resolution

Precedence (highest → lowest):

| # | Source | Description |
|---|--------|-------------|
| 1 | `--cache-path` (CLI arg) | Explicit per-call override. Always wins. |
| 2 | `TOKENZERO_CACHE_PATH` (env) | Process-wide env override. Wins when no CLI arg. |
| 3 | `<root>/.zerostack/tokenzero/recovery-cache.json` | Project-local unified store. Used when `.zerostack/` exists and either the unified file exists or the legacy file does not. |
| 4 | `<root>/.tokenzero/recovery-cache.json` | Legacy fallback. Used when no `.zerostack/` and no explicit/env path. |

## Store-Root Resolution

Precedence (highest → lowest):

| # | Source | Condition | Description |
|---|--------|-----------|-------------|
| 1 | `<root>/.zerostack` | Directory exists | Project-local store. Always wins when present, regardless of shared opt-in. |
| 2 | `ZEROSTACK_STORE_ROOT` / `ZERO_STACK_STORE_ROOT` (env) | `TOKENZERO_SHARED_STORE` or `ZEROSTACK_SHARED_STORE` is truthy (`1`, `on`, `true`, `yes`) | Shared/meta store. Active only with explicit opt-in. Relative paths join to `repo_root`. |
| 3 | _(none — legacy fallback)_ | No `.zerostack`, no pin, or pin without opt-in | Falls back to `<root>/.tokenzero/` for cache path. `effective_store_root` is `null`. |

## Workspace-Root Resolution

Precedence (highest → lowest):

| # | Source | Description |
|---|--------|-------------|
| 1 | `--root` (CLI arg) | Explicit per-call workspace root. |
| 2 | `TOKENZERO_ROOT` (env) | Process-wide env override. |
| 3 | `std::env::current_dir()` | Current working directory (cwd). |

## Key Invariants

1. **Explicit `--cache-path` always wins** over env and project-local defaults.
2. **`TOKENZERO_CACHE_PATH` wins over project-local** `.zerostack` default.
3. **Project-local `.zerostack` wins over shared opt-in.** Even when `TOKENZERO_SHARED_STORE=1` is set, a local `.zerostack/` directory takes precedence.
4. **`ZEROSTACK_STORE_ROOT` is non-operative without opt-in.** A bare global pin (without `TOKENZERO_SHARED_STORE` or `ZEROSTACK_SHARED_STORE`) is ignored. Doctor reports this as an info-level finding with `isolation_mode: "per_root"`.
5. **Default fallback is project-local.** Without any explicit or env configuration, cache paths resolve under `<root>/.tokenzero/` (legacy) or `<root>/.zerostack/tokenzero/` (unified, if `.zerostack/` exists).
6. **Bare global pin warns.** When `ZEROSTACK_STORE_ROOT` is set but no shared opt-in env is active, `mismatch_summary` contains `"ignored for isolation"` and instructs the user to set `TOKENZERO_SHARED_STORE=1`.
7. **Two roots with the same basename do not collide.** Store roots are resolved per-project; `proj_a/.zerostack` and `proj_b/.zerostack` are distinct even if both projects are named identically.
8. **Nonexistent pin paths are still resolved.** `resolve_store_root_with_env` does not check disk existence of the pin path; it returns it as-is. This allows pre-creation of store directories.
9. **Relative pin paths join to `repo_root`.** A relative `ZEROSTACK_STORE_ROOT` value is joined to the workspace root, not cwd.

## Env Var Reference

| Env Var | Role | Truthy Values |
|---------|------|---------------|
| `TOKENZERO_CACHE_PATH` | Cache-path override (level 2) | Any non-empty path |
| `ZEROSTACK_STORE_ROOT` | Global store-root pin | Any non-empty path |
| `ZERO_STACK_STORE_ROOT` | Legacy spelling of store-root pin | Any non-empty path |
| `TOKENZERO_SHARED_STORE` | Opt-in to shared store | `1`, `on`, `true`, `yes` (case-insensitive) |
| `ZEROSTACK_SHARED_STORE` | Alternate opt-in to shared store | `1`, `on`, `true`, `yes` (case-insensitive) |
| `TOKENZERO_ROOT` | Workspace-root override | Any non-empty path |

## Test Coverage

Integration tests: `crates/tokenzero/tests/store_root_precedence.rs`

| Test | Invariant |
|------|-----------|
| `cli_cache_path_beats_env_and_project_local` | 1, 2 |
| `tokenzero_cache_path_overrides_project_local` | 2 |
| `dot_zerostack_detected_and_used` | 3, 5 |
| `global_pin_without_opt_in_is_ignored` | 4, 6 |
| `global_pin_with_tokenzero_shared_store_is_active` | 4 (opt-in case) |
| `global_pin_with_zerostack_shared_store_is_active` | 4 (alternate env) |
| `dot_zerostack_wins_over_shared_opt_in` | 3 |
| `missing_store_root_with_opt_in_still_resolves` | 8 |
| `no_zerostack_no_pin_falls_back_to_legacy_tokenzero` | 5 |
| `relative_store_root_resolves_against_repo_root` | 9 |
| `two_roots_same_basename_no_collision` | 7 |
| `cwd_fallback_when_no_root_arg` | Workspace-root #3 |
| `legacy_store_root_env_spelling_with_opt_in` | Legacy env spelling |
