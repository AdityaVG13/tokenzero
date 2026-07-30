# TokenZero Install

TokenZero is package-first for users after launch and source-checkout friendly
for developers before launch. The public Core runtime target is the Rust
`tokenzero` binary (compatibility shim / selected symlink after surface install).

Launch offer: Compress aggressively. Recover exactly. One install.

## Mutually exclusive package surfaces (tokenzero-irx9.3)

Users install **one** surface from the same revision and shared core — never both:

| Artifact | Surface | Who should install it |
|---|---|---|
| `tokenzero-mcp` | FastMCP per-operation tools | **Native CodeMode clients** (ZeroStack hub, CodeMode-capable harnesses) |
| `tokenzero-codemode` | CodeMode plan catalog | **Legacy MCP-only clients** |
| `tokenzero` | Compatibility shim | Symlink to the selected surface binary only — never a dual-catalog binary |

**Selection matrix:** native CodeMode client → install `tokenzero-mcp`; otherwise → install `tokenzero-codemode`.

**Process mutual exclusion (not release-only naming):** one installed/running process must never contain or expose both catalogs. Dual surface features are a **compile error**. Dual argv/env selection and dual client registration **fail closed** at startup. Default package features enable **exactly one** surface (`surface-mcp`); CodeMode builds use `--no-default-features --features surface-codemode`.

Installing one surface after the other cleanly replaces the prior registration and peer binary. The installer writes `install-state.json` + `client-config.json` atomically itself and **never** starts a stdio server to write state.

### Surface installer (macOS / Linux)

```bash
# Build + install one surface (writes install-state + client-config atomically;
# never starts a stdio server to write state).
./packaging/install.sh --surface mcp          # or: --surface codemode
./packaging/install.sh --surface mcp --prefix ~/.tokenzero-install --bin-dir ~/.local/bin

# Identity / SBOM / uninstall
./packaging/install.sh --sbom --surface mcp
./packaging/install.sh --uninstall --prefix ~/.tokenzero-install

# Single-surface cargo builds (peer surface dependency excluded).
# Always pass --no-default-features: default = ["surface-mcp"], so naming a
# surface WITHOUT it leaves surface-mcp on too, and enabling both surfaces is a
# hard compile error (tokenzero-irx9.3). This bites hardest for codemode, where
# the obvious command fails:
#   cargo build --release --bin tokenzero-codemode --features surface-codemode
#     error: tokenzero surfaces are mutually exclusive ... never both.
# Plain `cargo build --release` also silently builds only tokenzero-mcp.
cargo build --release -p tokenzero --bin tokenzero-mcp --no-default-features --features surface-mcp
cargo build --release -p tokenzero --bin tokenzero-codemode --no-default-features --features surface-codemode

# Or let the installer pick the flags for you (it uses exactly the above):
./packaging/install.sh --surface codemode
```

Surface binaries also accept non-hanging packaging subcommands:

```bash
tokenzero-mcp install --surface mcp --prefix DIR
tokenzero-mcp sbom
tokenzero-mcp doctor
tokenzero-mcp uninstall --prefix DIR
```

Help, doctor, SBOM, and uninstall output identify the selected surface and the
shared semantic contract digest.

## Package Install

The public launch path is a GitHub Release download. Download the archive for
your OS from `https://github.com/AdityaVG13/tokenzero/releases`, verify the
matching `.sha256` file, extract it, and put `tokenzero` or `tokenzero.exe`
somewhere on `PATH` (after a surface install, `tokenzero` is a symlink to the
selected artifact).

```bash
tokenzero install --global --plan --mcp --shell --cli --json
tokenzero install --global --apply --mcp --shell --cli --json
tokenzero doctor --json
```

`tokenzero install` and `tokenzero install --plan` are read-only. They detect integration capabilities, classify risk, and print the plan without writing files.

Crates.io, npm, and Homebrew publication are separate gated channels and are
not required for the GitHub Release download path.

## Multi-machine / portable binaries (wqw.3)

TokenZero never requires host-absolute personal checkout paths
(e.g. `/Users/you/AI/tokenzero/target/release/tokenzero`) in client configs.

**Binary discovery order** for `tokenzero`, `rg`, and `curl`:

1. **Env override** — `TOKENZERO_BIN`, `TOKENZERO_RG_PATH`, `TOKENZERO_CURL_PATH`
2. **PATH**
3. **Well-known layouts** — `~/.tokenzero/bin`, `~/.cargo/bin`, `~/.local/bin`,
   `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin` (plus `current_exe` for
   the running TokenZero process)
4. **Clear error** with install/PATH guidance

**Recommended multi-machine setup:**

1. Install a release binary on each machine (`tokenzero install --global` or
   put the release binary on `PATH`).
2. Wire MCP with a portable config — use `command: "tokenzero"` (PATH), not a
   personal absolute path. See `mcp.md`.
3. Inspect resolved paths: `tokenzero doctor --json` → `engine_binaries`.

Optional overrides when PATH cannot be controlled (CI, restricted shells):

```bash
export TOKENZERO_BIN=/usr/local/bin/tokenzero
export TOKENZERO_RG_PATH=/usr/local/bin/rg
```

## Developer Install

```bash
git clone https://github.com/AdityaVG13/tokenzero.git
cd tokenzero
cargo build --release -p tokenzero --locked
target/release/tokenzero install --plan --json
target/release/tokenzero doctor --json
```

Use the source checkout path for development, tests, local package verification,
and pre-launch dogfooding.

## What Install Detects

`tokenzero install --plan --json` detects capabilities:

| Capability | Examples |
| --- | --- |
| `mcp` | JSON and TOML MCP registries |
| `hooks` | Claude Code `PreToolUse` entry in `.claude/settings.json` |
| `shim` | universal PATH shims under `.tokenzero/shims/` |
| `instructions` | `AGENTS.md`, `CLAUDE.md`, `DROID.md`, `GEMINI.md`, `GROK.md`, `.cursor/rules/*.mdc` |
| `shell` | launchers and existing shell profiles |
| `runtime` | PATH-visible Rust `tokenzero` binary |

Plans are capability-first. Adapter ids may be client-specific, but public output leads with capability, scope, path, risk, and rollback metadata.

## Apply

```bash
tokenzero install --apply
```

Project apply writes only project-local safe surfaces:

- parseable MCP config entries;
- compact instruction pointers;
- enforceable hook routing;
- rollback metadata under `.tokenzero/install/`;
- direct disk verification rows.

It preserves existing non-TokenZero config entries and never overwrites corrupt or unknown schemas.

## Global Scope

```bash
tokenzero install --global --plan --mcp --shell --cli --json
tokenzero install --global --apply --mcp --shell --cli --json
```

Global writes require explicit global scope. `--mcp` configures AI client
surfaces, `--shell` installs the shell launcher, and `--cli` installs a stable
launcher plus a versioned copy of the Rust runtime under `~/.tokenzero/bin`.
When `--root` is omitted, global scope defaults to the platform home directory:
`%USERPROFILE%` on Windows and `$HOME` on Unix-like hosts. Unix-like hosts write
`~/.tokenzero/bin/tokenzero`; Windows writes `~/.tokenzero/bin/tokenzero.cmd`.
Generated MCP configs launch the installed `tokenzero-runtime-*` copy directly,
while the shell launcher keeps a stable command on PATH. Each global write is
listed, backed up, restricted by a declared write allow-list, and
rollback-capable. Do not run global apply during pre-launch implementation
unless explicitly approved.

Global MCP setup merge-patches only the `tokenzero` server entry and preserves
other configured servers. Current client surfaces include Claude Code, Claude
Desktop, Codex, Cursor, Gemini, Grok, OpenCode, and generic TokenZero MCP
registry files under `~/.config/tokenzero/agents/`. Windows client discovery
uses `%APPDATA%`-style paths such as `AppData/Roaming/Claude` and
`AppData/Roaming/Cursor` in addition to dotfile registries.

### Manual MCP config (no installer)

If you wire a client by hand instead of running `tokenzero install`, the
server entry is:

```json
{"mcpServers":{"tokenzero":{"type":"local","command":"__TOKENZERO_BIN__","args":["mcp-server","--allowed-root","__REPO__","--cache-path","__CACHE__"],"tools":["*"]}}}
```

Substitute `__TOKENZERO_BIN__` (installed binary), `__REPO__` (allowed root),
and `__CACHE__` (cache file path). The installer writes exactly this shape and
merge-patches only the `tokenzero` entry. (Formerly shipped as
demo/tokenzero-mcp.template.json; MCP vs CodeMode surface selection is covered
by the mutually exclusive package surfaces section above.)

## Hooks and Shims

```bash
tokenzero install --global --plan --hooks --shims --json
tokenzero install --global --apply --hooks --shims --json
```

Both surfaces ride the same planner as `--mcp`: plan is read-only, apply is
two-phase (snapshot-first rollback manifest, then atomic writes plus
verification rows), `tokenzero install --rollback latest` restores, and
`tokenzero clients detect/doctor --json` reports their installed/mixed/missing
state.

`tokenzero clients scan --json` probes the machine first: it detects which AI
harnesses are present (config directories under the home root plus launcher
binaries on PATH — Claude Code, Codex, Cursor, Gemini, Factory droid,
OpenCode, Grok, and manual-adapter harnesses like Windsurf, Aider, Zed, and
Crush) and emits the exact `tokenzero install` invocation that wires the
supported ones. Detection never writes; harnesses marked `supported: false`
are adapted manually with the snippets in `docs/codemode.md`.

`--hooks` merge-patches only the TokenZero-owned `PreToolUse` entry into
`.claude/settings.json` (the entry whose command runs `tokenzero hook
claude-code`); every other hook and setting is preserved and re-apply is
idempotent. The hook rewrites Claude Code `Bash` calls to run under `tokenzero
run` and is strictly fail-open: commands that must not be wrapped (compound
commands containing `cd`/`export`/`unset`/`alias` in any top-level segment,
background `&` jobs, heredocs, interactive programs) pass through unchanged.
Wrapping can be disabled per command by prefixing it with
`TOKENZERO_NO_WRAP=1 ...`, or session-wide by setting the `TOKENZERO_NO_WRAP`
environment variable (values `0`/`false`/`off` keep wrapping ON; any other
value disables it). Hooks writes are claude-scoped: plans targeting other
agents (for example `--agent grok`) never list a `.claude` path. The `clients`
standard profile includes `hooks` alongside `mcp`.

The same capability wires a `SessionStart` entry running `tokenzero hook
claude-code-session-start`: after a compaction or resume wipes the model's
context, it injects a compact, token-budgeted session pack listing the
payloads TokenZero already served this workspace — most recent first, each
with its exact `tz://` ref — so the agent re-orients with `expand`/`recall`
instead of re-reading files or re-running commands. Fresh sessions
(`startup`/`clear`) stay silent, and a workspace with no recovery cache
injects nothing.

`--shims` writes small POSIX shims for `cat head tail grep rg find ls tree wc`
under `~/.tokenzero/shims/`. Tools missing from PATH at install time are
skipped, so the plan is per-machine. Each shim bakes in the real binary path
(resolved with the shim directory excluded) and targets the stable launcher
behind an executability guard, so a missing or stale launcher falls through to
the real binary instead of failing. Activation needs BOTH halves in the
harness's environment — the env flag and the PATH entry:

```bash
export TOKENZERO_SHIM=1
export PATH="$HOME/.tokenzero/shims:$PATH"
```

`TOKENZERO_INNER=1` (set automatically for children of `tokenzero run` and
`tz_shell`) breaks recursion and double-wrapping. Shims stay out of the
`clients` standard profile — they are opt-in via `--shims` because they shadow
PATH-visible binaries. The shim scripts are POSIX `sh`; on Windows they only
take effect inside POSIX shells (Git Bash/MSYS), which is also where coreutils
names like `cat` and `grep` resolve.

## Rollback

```bash
tokenzero install --rollback latest
tokenzero clients rollback latest
```

Rollback restores files from TokenZero install manifests and is idempotent. Manifests store paths, hashes, operations, and backup paths. They do not store raw payload logs.

## Status

```bash
tokenzero clients detect --json
tokenzero clients plan --profile standard --json
tokenzero client-status --json
tokenzero install-smoke --json
tokenzero clients doctor --json
```

`client-status` reports installed, missing, mixed, and raw bypass risk states so you can verify whether a harness is actually using TokenZero.

`install-smoke` creates an isolated disposable project and home directory,
applies capability setup there, verifies generated config parses, starts the
generated MCP server, checks alias plus `tz_*` tools, and verifies rollback. It
must not write real global user config during smoke verification.

On Windows, `scripts/rust_windows_global_rehearsal.ps1` performs the stronger
preflight for a real machine: it copies existing global MCP merge targets into a
temporary home with spaces, applies global setup there, validates the merged
configs, and launches MCP through the installed runtime copy referenced by the
generated MCP config.

For the final Windows machine migration, `scripts/rust_windows_migrate.ps1`
emits a dry-run JSON plan by default. It requires both `-Apply` and
`-ConfirmMigration` before it archives the old checkout or writes real global
config.

## Unsupported Hosts

TokenZero still works when a host cannot be patched. Use any supported surface the host can access:

- MCP tools when available;
- local CLI commands through shell or exec;
- project instruction pointers;
- hook/event routing when enforceable.

## Repository Shape

The repository root is the development source. The installable package is the
public Rust Core runtime once release gates pass. The source checkout path is
for development, local tests, and package verification.

The public release branch is a curated release tree with clean history. Ignored local planning folders are maintainer-only source context. They are not package members or installation inputs.

Before publication, release-readiness verifies:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release -p tokenzero --locked
cargo package --workspace --locked
target/release/tokenzero package-audit --dist target/release --json
```

`package-audit` fails closed on ambiguous release archive metadata. Top-level
archives over the read budget, unsupported split ZIP, duplicate ZIP
end-of-central-directory candidates, ZIP central
directories embedded in EOCD comments, malformed, oversized, or overflowing
Zip64 metadata,
central/local ZIP name mismatches, central/local ZIP Unicode path alias
mismatches, mismatched ZIP data descriptors, overlapping ZIP local records, ZIP
payloads that overlap the central directory, unsupported or corrupt ZIP
payloads, ZIP archives whose aggregate uncompressed payload size exceeds the
audit budget, ZIP directory entries with payload bytes, unclaimed bytes outside
declared ZIP local records before the central directory, unsupported ZIP extra
fields, Zip64 extra fields without required size or offset sentinels,
executable/script archive payloads that reference dev launchers or external
runtimes, ZIP archive or entry comments, malformed tar metadata including
checksum mismatches, negative or oversized tar base-256 size fields, non-zero
trailing data after tar end markers, tar directory entries with payload bytes,
unsupported tar special member types, local owner metadata, unsupported PAX
metadata fields, duplicate PAX path or linkpath overrides, global PAX path or
linkpath overrides, path escapes, invalid UTF-8 or control characters in member
names or link targets, private tool-state members, and nested archive members are
blockers until the artifact is regenerated or the parser support is expanded
deliberately.

## ZeroRef store migration and rollback

Portable ZeroRef v1 support is limited to full-hash blob refs, including '#B'
and '#L' fragments. Inspect capabilities and effective roots, retain the
migration manifest, and dry-run before applying:

~~~bash
tokenzero capabilities --json
tokenzero cache migrate-refs --json
tokenzero cache migrate-refs --apply --json
tokenzero cache migrate-verify --json
tokenzero cache migrate-rollback --json
tokenzero cache migrate-rollback --apply --json
~~~

A shared store requires explicit opt-in. Project-local '.zerostack' takes
precedence over a shared pin; '--cache-path' and 'TOKENZERO_CACHE_PATH' remain
higher-priority cache overrides. Migration never broadens non-blob portability.
Retain the original cache and manifest until verification passes.
