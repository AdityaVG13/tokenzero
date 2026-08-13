# Repo shape (TokenZero is the template)

FSZero and GraphZero follow this file. Do not invent a second layout.

## Crate map

```text
crates/<product>-core          domain types only
crates/<product>-<authority>   TZ recovery / FS store+bytes / GZ store+claims
crates/<product>-engine        dispatch (TZ) / fs_ops (FS) / query (GZ)
crates/<product>               CLI
crates/<product>-codemode      raw-worker bin, thin
crates/<product>-mcp           MCP surface
crates/<product>-test-support  re-export zero-testkit + engine fixtures
crates/<product>-install       only if the product ships an installer
tests/                         workspace member, subdirs named by crate
```

TokenZero today: `tokenzero-core`, `tokenzero-recovery` (authority), `tokenzero-engine`, `tokenzero` (CLI), `tokenzero-codemode`, `tokenzero-mcp-compat` (MCP name until slim-public `4uql.11`), `tokenzero-test-support`, `tokenzero-install`, plus `tokenzero-runtime` / `tokenzero-filters` / `tokenzero-pulse`.

## Workspace law

- Cargo resolver 3
- `[workspace.package]` and `[workspace.dependencies]`
- One hub `rev =` for every `zero-*` crate (currently `bd721f7fc4866b24dec0c552da3d96bd8d816fbc`)
- No path-patch of hub crates
- `tests/` is a workspace member
- `deny.toml`, `CONTRIBUTING.md`, `SECURITY.md`

## What not to copy

- `Pareto/`, `formal/`, `beads_compliance_audit/`, `ubs_audit/` (research attic; `formal/` is `export-ignore`)
- Audit dump trees, RADC wave zips, agent-session caches

## Deliberate exceptions

- `tokenzero-mcp-compat` name until `tokenzero-slim-public-repo-4uql.11` (owner-gated). Do not rename in a shape pass.
- FS `src/core/path.rs` atomic write (xattr/mode/mtime) stays product-local.
- GZ `put_nosync*` stays a GraphZero batch extension.
- Packaging zip/tar path rewrites stay local if hub `atomic_write_file` cannot express the format.
- Hub journals are not FS `mutation_log` and not GZ snapshot WAL.

## Shared I/O already on the hub

Generic bytes/json replace uses `zero_store::atomic_write_file` / `replace_file`. CAS put/get uses `SharedCas`. Recovery journal framing uses `SessionWal`.
