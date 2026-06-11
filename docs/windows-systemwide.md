# Windows Systemwide Migration

This runbook is for moving a Windows development machine from the old Python
TokenZero checkout to the Rust Core runtime and MCP server.

## Current Approval Boundary

Do not delete, archive, or overwrite an existing Python checkout until the
operator approves the migration. Do not run real global install against the
user profile until the Windows verifier and disposable-home global install pass.

## Verified Windows Gates

Run from the Rust checkout:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/rust_windows_verify.ps1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release -p tokenzero-cli --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked --release
cargo doc --workspace --no-deps --locked
cargo package --workspace --locked --allow-dirty
cargo deny check
target\release\tokenzero.exe mcp-smoke --json
target\release\tokenzero.exe install-smoke --json
target\release\tokenzero.exe package-audit --dist target\release --json
cd package\npm
npm pack --dry-run
$env:TOKENZERO_BIN = "..\..\target\release\tokenzero.exe"
npm run smoke
```

The Windows verifier must report `status: "ok"` and all steps must have
`ok: true`, including the global install rehearsal artifact at
`results/current/rust_windows_global_rehearsal.json` and the migration dry-run
artifact at `results/current/rust_windows_migration_plan.json`. `cargo deny
check` must return exit code 0; a duplicate-version warning is acceptable when
policy leaves `multiple-versions = "warn"`. The migration dry-run also checks
the selected remote branch with `git ls-remote`; failures there mean the branch
argument or network/authentication needs to be fixed before any archive/apply
step is allowed.

## Disposable-Home Global Proof

Before touching the real home directory, run the automated rehearsal. It copies
existing real MCP merge targets into a disposable home path that contains
spaces, applies the Rust global installer there, validates merged configs, and
launches MCP through the installed runtime copy referenced by the generated MCP
config:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/rust_windows_global_rehearsal.ps1
```

For a manual proof, apply into a disposable home path that contains spaces:

```powershell
$root = "C:/tmp/tokenzero global proof"
target\release\tokenzero.exe install --global --apply --mcp --shell --cli --root $root --json
```

Confirm the launcher uses an installed runtime copy, then launch MCP through
that installed runtime path:

```powershell
$runtime = Get-ChildItem "$root/.tokenzero/bin/tokenzero-runtime-*.exe" | Select-Object -First 1
if (Select-String -LiteralPath "$root/.tokenzero/bin/tokenzero.cmd" -Pattern "target\\release" -Quiet) {
  throw "launcher still points at target\release"
}
& $runtime.FullName mcp-server --allowed-root $root --cache-path "$root/.tokenzero/recovery-cache.json"
```

Send an `initialize` JSON-RPC message on stdin and verify the server responds
with `serverInfo.name: "tokenzero"`.

## Approved Migration Steps

After approval:

The scripted migration path is:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/rust_windows_migrate.ps1
```

The command above is a dry run. To execute the migration after reviewing the
JSON plan, run it with explicit apply confirmation:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/rust_windows_migrate.ps1 -Apply -ConfirmMigration -Branch main
```

Use `-SourceUrl` if the release branch should be cloned from a different remote
than the current checkout's `origin`.

The script verifies the Rust source checkout, rehearses the real global config,
checks that the selected remote branch exists, archives the Python checkout,
clones the selected branch into `C:\Users\you\tokenzero`, builds the final
release binary there, applies the global MCP/shell/CLI install, and verifies
the launcher plus MCP initialize response. Use `-Branch main` after the Windows
fixes have landed on `main`; for a pre-merge dogfood run, pass the published
release branch instead. If a guarded apply fails, the JSON report includes the
completed steps plus rollback hints for restoring the archived checkout and, if
needed, rolling back global install metadata. Use the manual sequence below only
if you need to perform the steps by hand.

1. Rename the old checkout, for example:

   ```powershell
   Rename-Item C:\Users\you\tokenzero C:\Users\you\tokenzero-python-old-2026-06-02
   ```

2. Clone or move the verified Rust checkout into `C:\Users\you\tokenzero`.

3. Build the release binary:

   ```powershell
   cargo build --release -p tokenzero-cli --locked
   ```

   Running the remaining install commands copies a versioned runtime into
   `C:\Users\you\.tokenzero\bin`, so generated MCP clients use the installed
   copy instead of pinning the source checkout's `target\release` binary.

4. Preview global writes:

   ```powershell
   target\release\tokenzero.exe install --global --plan --mcp --shell --cli --root C:\Users\you --json
   ```

5. Apply global writes:

   ```powershell
   target\release\tokenzero.exe install --global --apply --mcp --shell --cli --root C:\Users\you --json
   ```

6. Verify the global launcher and MCP server:

   ```powershell
   C:\Users\you\.tokenzero\bin\tokenzero.cmd --version
   C:\Users\you\.tokenzero\bin\tokenzero.cmd doctor --root C:\Users\you\tokenzero --runtime --json
   ```

7. Restart MCP clients such as Codex so they load the Rust MCP config.

## Rollback

If global install needs to be undone:

```powershell
C:\Users\you\.tokenzero\bin\tokenzero.cmd install --rollback latest --root C:\Users\you --json
```

Then restore the archived Python checkout if needed:

```powershell
Rename-Item C:\Users\you\tokenzero C:\Users\you\tokenzero-rust-rolled-back
Rename-Item C:\Users\you\tokenzero-python-old-2026-06-02 C:\Users\you\tokenzero
```
