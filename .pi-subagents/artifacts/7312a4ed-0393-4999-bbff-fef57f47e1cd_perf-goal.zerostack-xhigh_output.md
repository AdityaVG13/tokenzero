**P1 root cause:** `crates/tokenzero/tests/irx9_surface_bench_process.rs:measure_mcp_framing` writes to an already-exited `mcp-server` child, then `unwrap()` panics at line 260. Piped stderr is not surfaced, hiding the child’s actual startup failure.

**Minimal owned patch:** only that test file. Pipe stderr; replace write unwrap with an error branch that drops stdin, waits for the child, and reports exit status plus stderr.

**P2 validity issue:** `real_process_surface_bench_records_starts_cpu_serialization` deliberately starts an extra kill-test child, contaminating `PROCESS_STARTS`. Exclude it from benchmark accounting.

Focused acceptance:
```bash
cargo test -p tokenzero --features surface-codemode --no-default-features --test irx9_surface_bench_process real_process_surface_bench_records_starts_cpu_serialization -- --exact --nocapture
```