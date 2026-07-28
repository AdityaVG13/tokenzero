Run the focused acceptance command only after isolating the shared store root.

1. **Blocker -- gating:** `crates/tokenzero-mcp/src/codemode/bench.rs:508`, `bench_harness`. Nine tests invoke unavailable FastMCP rendering under `surface-codemode`. Gate the module/tests with `surface-mcp`; do not runtime-skip.
2. **High -- nondeterminism:** Updated bead evidence is 17/14/15 failures parallel versus nine stable gated failures serially. The ten intermittent shell/expand tests pass alone. Shared working-root recovery storage is the strongest candidate, but causality is not yet proven. Thread-local registries alone do not prove cross-test sharing.
3. **Ownership:** `exec.rs` and `e2e_tests.rs` already contain unstaged changes. Avoid overlapping edits. Minimal confirmed patch is only `bench.rs`; isolate store roots through existing test setup before touching production registries.
4. **Latency:** No direct warm raw-worker `token.shell` optimization was source-proven. Do not claim one from dirty `session.rs` or `raw_worker_inline_tests.rs`.

```sh
for i in 1 2 3; do cargo test -p tokenzero-mcp --lib --no-default-features --features surface-codemode -- --test-threads=8 || exit; done
```