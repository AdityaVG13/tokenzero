# Wind-tunnel replay MVP

Counterfactual replay gate for recorded TokenZero plan journals
(`tokenzero-wind-tunnel-replay-tyq`).

**MVP scope:** load journals -> apply baseline vs candidate **policy stubs** ->
diff action sequences (`index`, `id`, `method`) -> exit `1` on divergence.
No model re-execution, no multi-hour corpus runs.

## Quick smoke (fixtures)

```bash
# identity vs identity -> exit 0
python3 benchmarks/wind_tunnel/harness.py \
  --journals benchmarks/wind_tunnel/fixtures \
  --baseline identity --candidate identity --quiet

# identity vs drop_shell -> exit 1 (session-mixed has a shell op)
python3 benchmarks/wind_tunnel/harness.py \
  --journals benchmarks/wind_tunnel/fixtures \
  --baseline identity --candidate drop_shell --quiet
```

## Point at real journals

Plan journals are written under the unified store:

| Location | Typical path |
|----------|----------------|
| In-repo ZeroStack store | `.zerostack/tokenzero/plan-journals/` |
| Env override | `$ZEROSTACK_STORE_ROOT/tokenzero/plan-journals/` |
| Legacy local cache | `.tokenzero/` (ledger/recovery; journals prefer `.zerostack/`) |

```bash
# Small smoke against local corpus (keep --limit tiny)
python3 benchmarks/wind_tunnel/harness.py \
  --journals .zerostack/tokenzero/plan-journals \
  --baseline identity --candidate identity \
  --limit 32 --quiet

# Probe a candidate stub that rewrites compactMany
python3 benchmarks/wind_tunnel/harness.py \
  --journals .zerostack/tokenzero/plan-journals \
  --baseline identity --candidate collapse_compact_many \
  --limit 64 --output /tmp/wind-tunnel-report.json
```

Journal schema: `tokenzero.plan-journal.v1` (see
`crates/tokenzero-mcp/src/codemode/journal.rs`). Action atoms are the
`operations[]` entries; payload bytes stay out of the journal.

## Policy stubs

| Name | Behavior |
|------|----------|
| `identity` | Recorded sequence unchanged (baseline) |
| `drop_shell` | Drop `zero.token.shell` ops |
| `collapse_compact_many` | Rewrite `zero.token.compactMany` -> `zero.token.compact` |

Replace stubs later with real context policies behind the same
`POLICIES` map in `policies.py`.

## Tests

```bash
python3 -m unittest benchmarks.wind_tunnel.test_harness -v
```
