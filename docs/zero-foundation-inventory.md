# Zero foundation inventory

Evidence snapshot from the tracked trees. This is an inventory only. It does not select an ownership or release model and does not extract code.

## Current TokenZero ownership (live HEAD) — compact move list

Canonical machine-readable per-file ownership truth remains hub-only at `../ZeroStack/tests/data/loc_ownership_v1.json`; no TokenZero-local JSON snapshot is created. That file is blob-bound classification input, not a cached measurement table. Per-file `code_lines` is not stored in the JSON; it is computed live from the bound `HEAD` blob via `tokei` in `../ZeroStack/tests/scripts/check_loc_majority.py`. Compatibility shims are represented canonically as `classification: thin-adapter` together with exact `rule` / `justification` / `hub_target`, not a redundant boolean field.

- TokenZero `HEAD`: `9b4df921fe72259975f08768c90f9fdafad539b8`.
- Canonical hub `loc_ownership_v1.json` currently binds TokenZero `56d10770b7f0b950708f75716007d6aa3217f776`; hub refresh to `9b4df92` is pending after current engine commits settle.
- Live current-`HEAD` measurement for the four audited TokenZero crates (`crates/tokenzero-core`, `crates/tokenzero-recovery`, `crates/tokenzero-runtime`, `crates/tokenzero-engine`): **147 files, 54,930 tokei `code` LOC**; of those **120 files / 44,080 LOC are `domain-local`** and **27 files / 10,850 LOC are `shared-candidate`** (classification from `check_loc_majority.py` reviewed rules; LOC from live blob `tokei`).

Grouped move table for those 27 `shared-candidate` files (counts use live `tokei` `code` LOC at `9b4df92`; destination is the pinned hub crate; adapter is the minimal retained TokenZero shim):

| group / rule | files | code LOC | hub destination | minimal retained local adapter |
|---|---:|---:|---|---|
| `operation-abi` | 9 | 2,327 | `zero-abi` | thin `tokenzero-core` catalog/digest/registry re-export pinning the TokenZero operation registry to hub `zero-abi` types |
| `raw-worker` | 4 | 2,397 | `zero-abi` protocol + `zero-codemode` lifecycle | protocol adapter (`raw_worker_v2_protocol`) delegating framing/digests to `zero-abi` and lifecycle adapter routing `raw_worker_v2_impl` through hub `zero-codemode` host |
| `telemetry` | 3 | 506 | `zero-abi` / `zero-ledger` counters | `usage_telemetry` + `telemetry` shim mapping hub `TelemetrySchema` counters to TokenZero model/accounting semantics (model identity stays local) |
| `codemode-host` | 4 | 1,272 | `zero-codemode` | `codemode_catalog` / `codemode_wire` + dispatcher test adapter registering TokenZero ops on the hub `zero-codemode` host |
| `session-discovery` | 1 | 437 | `zero-codemode` | `session_persist` thin Session/store-root adapter |
| `surface-protocol` | 1 | 223 | `zero-codemode` | `surface_handshake` handshake/capability adapter |
| `store-cas` | 3 | 2,830 | `zero-store` | `embedded_store` / `segment_store` / `shared_cas` CAS bridge delegating to hub `zero-store` CAS |
| `zeroref` (tests) | 2 | 858 | `zero-ref` + `ZeroStack/tests` shared suite | shared-conformance test shim consuming hub `zero-ref` vectors via `ZeroStack/tests` |

Reproduction (read-only, no Cargo/rustc, no staging):

```sh
python3 ../ZeroStack/tests/scripts/check_loc_majority.py --write-inventory --inventory /tmp/tokenzero-spwo-current-inventory.json ../ZeroStack ../FSZero ../GraphZero .
```

Then, live `tokei` `code` LOC per bound blob is obtained through `collect_sources` from the same script (iterates each repo's `HEAD` blobs, filters `EXCLUDED_SEGMENTS`/generated markers, runs `tokei --output json --files` on the materialized blob set, and joins the result to the inventory's `classification`/`rule`/`hub_target` by `(repo, path)`). No separate TokenZero inventory file is written.

## Snapshot and authority

| tree | recorded snapshot `HEAD` | tracked Rust files | Tokei code LOC |
|---|---|---:|---:|
| TokenZero | `a971153de108a871ab92dd3b0a0abf023d6dbb53` | 248 | 102,946 |
| FSZero | `6a5f5eef7138e8a01018a881b04bb94bb5de9fa0` | 246 | 54,601 |
| GraphZero | `548defaebc0de4da5cca975353700f77ac83a407` | 407 | 89,747 |
| ZeroStack | `ca8636b44a8f6b53ec06cc41def989e746ce6c3f` | 124 | 78,131 |

The ZeroStack row intentionally retains the recorded `ca8636b44a8f6b53ec06cc41def989e746ce6c3f` snapshot. The current ZeroStack checkout has newer commits; those newer commits do not alter this recorded snapshot, and this inventory still uses it with ZeroStack source paths checked at that commit.

Each count uses only `*.rs` paths materialized from the recorded commit with `git archive`, so Tokei reads committed bytes at that exact SHA. Paths under `target` and `vendor` are filtered out. Tracked dirty worktree files and untracked files are excluded because the measured bytes come from the commit archive, not the current worktree. Tokei `code` is formatted Rust source LOC, not a claim about executable behavior.

TokenZero already had modifications in `crates/tokenzero-engine/src/engine_expand.rs`, `crates/tokenzero-engine/src/recall.rs`, and `fuzz/Cargo.lock`. ZeroStack already had tracked modifications and untracked `.ee/` and `conformance/src/*` files. FSZero and GraphZero were clean. Those files were not edited; the dirty and untracked files are excluded from this measurement because the bytes come from the recorded commit archives.

### Recorded per-crate LOC breakdown

`production | test | generated/fixture | total`, from the exact-SHA archive/Tokei run:

| repository / crate | production | test | generated/fixture | total |
|---|---:|---:|---:|---:|
| TokenZero / `(root)` | 0 | 118 | 0 | 118 |
| TokenZero / `tokenzero` | 8,103 | 6,255 | 483 | 14,841 |
| TokenZero / `tokenzero-codemode` | 7,888 | 1,704 | 0 | 9,592 |
| TokenZero / `tokenzero-core` | 11,316 | 2,552 | 0 | 13,868 |
| TokenZero / `tokenzero-engine` | 18,490 | 2,440 | 323 | 21,253 |
| TokenZero / `tokenzero-filters` | 1,116 | 0 | 0 | 1,116 |
| TokenZero / `tokenzero-install` | 5,925 | 2,711 | 475 | 9,111 |
| TokenZero / `tokenzero-mcp-compat` | 5,212 | 6,541 | 0 | 11,753 |
| TokenZero / `tokenzero-pulse` | 2,393 | 0 | 0 | 2,393 |
| TokenZero / `tokenzero-recovery` | 14,046 | 3,470 | 0 | 17,516 |
| TokenZero / `tokenzero-runtime` | 1,287 | 0 | 0 | 1,287 |
| TokenZero / `tokenzero-test-support` | 0 | 98 | 0 | 98 |
| FSZero / `(root)` | 1,364 | 19,735 | 213 | 21,312 |
| FSZero / `fszero` | 32,514 | 0 | 160 | 32,674 |
| FSZero / `fszero-codemode` | 91 | 0 | 0 | 91 |
| FSZero / `fszero-mcp` | 11 | 0 | 0 | 11 |
| FSZero / `fszero-shim` | 488 | 0 | 0 | 488 |
| FSZero / `fszero-test-support` | 0 | 25 | 0 | 25 |
| GraphZero / `(root)` | 0 | 0 | 197 | 197 |
| GraphZero / `graphzero-cli` | 7,067 | 3,910 | 1,221 | 12,198 |
| GraphZero / `graphzero-codemode` | 801 | 83 | 0 | 884 |
| GraphZero / `graphzero-core` | 1,909 | 0 | 0 | 1,909 |
| GraphZero / `graphzero-coverage` | 829 | 311 | 0 | 1,140 |
| GraphZero / `graphzero-extract` | 2,691 | 228 | 0 | 2,919 |
| GraphZero / `graphzero-mcp-compat` | 283 | 0 | 0 | 283 |
| GraphZero / `graphzero-pack` | 918 | 293 | 0 | 1,211 |
| GraphZero / `graphzero-query` | 19,143 | 6,092 | 462 | 25,697 |
| GraphZero / `graphzero-reserve` | 1,064 | 560 | 0 | 1,624 |
| GraphZero / `graphzero-scip` | 805 | 0 | 48 | 853 |
| GraphZero / `graphzero-semantic` | 918 | 169 | 0 | 1,087 |
| GraphZero / `graphzero-store` | 27,894 | 6,283 | 135 | 34,312 |
| GraphZero / `graphzero-test-support` | 0 | 3,199 | 118 | 3,317 |
| GraphZero / `graphzero-types` | 305 | 33 | 0 | 338 |
| GraphZero / `graphzero-why` | 1,288 | 490 | 0 | 1,778 |
| ZeroStack / `(root)` | 1,233 | 5,747 | 221 | 7,201 |
| ZeroStack / `zero-abi` | 12,080 | 0 | 0 | 12,080 |
| ZeroStack / `zero-cert` | 2,055 | 486 | 729 | 3,270 |
| ZeroStack / `zero-codemode` | 7,616 | 4,689 | 595 | 12,900 |
| ZeroStack / `zero-gate` | 20,627 | 77 | 0 | 20,704 |
| ZeroStack / `zero-gauge` | 467 | 0 | 0 | 467 |
| ZeroStack / `zero-ledger` | 2,077 | 1,272 | 0 | 3,349 |
| ZeroStack / `zero-ref` | 506 | 217 | 194 | 917 |
| ZeroStack / `zero-store` | 4,136 | 0 | 0 | 4,136 |
| ZeroStack / `zero-testkit` | 9,838 | 0 | 427 | 10,265 |
| ZeroStack / `zerostack-machine-permit` | 1,708 | 1,134 | 0 | 2,842 |

Classification precedence is: `generated/fixture` first; then `test` when the path contains a `tests`, `benches`, or `fuzz` component, the basename ends in `_test.rs` or `_tests.rs`, or the crate name contains `test-support`; otherwise `production`. Inline `#[cfg(test)]` modules remain a file-level limitation: a mixed production/test file is assigned by its path, not by parsing item-level cfgs.

### Shared testkit consumer impact (2026-08-12)

TokenZero, FSZero, and GraphZero consume `zero-testkit` at ZeroStack revision
`b0978d037613d107fb060152500110cbaceb13e8` with default features disabled.
The consumer surface compiles 788 Rust source lines in `lib.rs`; 8,933 lines of
hub-only conformance modules and 1,117 lines of example binaries remain gated
behind `full`. The minimal normal dependency graph contains 23 packages instead
of 37 for `full`. Test-support crates are not linked into shipped product
binaries, so measured shipped-binary impact is **0 bytes**. Centralizing
`decode_worker_transcript` removes its duplicated implementations from TokenZero
and FSZero while retaining one unknown-field mutation gate in the hub.

### Rerunnable LOC measurement

Run from the TokenZero root. This is read-only and does not invoke Cargo or Rust tooling. Each repository is measured from a temporary archive of its recorded commit, so Tokei never reads current worktree bytes. The temporary archive and extraction directory are deleted after each repository.

```sh
python3 - <<'PY'
import json
import os
import subprocess
import tempfile
from collections import defaultdict
from pathlib import Path

specs = [
    ('.', 'TokenZero', 'a971153de108a871ab92dd3b0a0abf023d6dbb53'),
    ('../FSZero', 'FSZero', '6a5f5eef7138e8a01018a881b04bb94bb5de9fa0'),
    ('../GraphZero', 'GraphZero', '548defaebc0de4da5cca975353700f77ac83a407'),
    ('../ZeroStack', 'ZeroStack', 'ca8636b44a8f6b53ec06cc41def989e746ce6c3f'),
]

for root, label, commit in specs:
    with tempfile.TemporaryDirectory(prefix='tokenzero-loc-') as temp:
        archive = Path(temp) / 'source.tar'
        extracted = Path(temp) / 'tree'
        extracted.mkdir()
        with archive.open('wb') as stream:
            subprocess.run(
                ['git', '-C', root, 'archive', '--format=tar', commit, '--', '*.rs'],
                stdout=stream, check=True)
        subprocess.run([
            'tar', '-xf', os.fspath(archive), '-C', os.fspath(extracted),
            '--no-same-owner', '--no-same-permissions'], check=True)
        paths = sorted(
            str(path.relative_to(extracted))
            for path in extracted.rglob('*.rs')
            if not any(part in {'target', 'vendor'}
                       for part in path.relative_to(extracted).parts))
        tree = json.loads(subprocess.check_output(
            ['tokei', '--output', 'json', '--files', *paths], cwd=extracted))
        rows = []
        def visit(node):
            if isinstance(node, dict):
                name, stats = node.get('name'), node.get('stats')
                if (isinstance(name, str) and name.endswith('.rs') and
                        isinstance(stats, dict)):
                    rows.append((name, int(stats.get('code', 0))))
                for value in node.values():
                    visit(value)
            elif isinstance(node, list):
                for value in node:
                    visit(value)
        visit(tree)
        rows = dict(rows)
        by_crate = defaultdict(lambda: defaultdict(int))
        for path, code in rows.items():
            low = path.lower()
            if label == 'FSZero':
                crate = '(root)' if not path.startswith('src/') else 'fszero'
            else:
                crate = path.split('/')[1] if path.startswith('crates/') else '(root)'
            parts = low.split('/')
            basename = parts[-1]
            generated_or_fixture = any(x in low for x in
                                       ('fixture', 'fixtures', 'generated', 'corpus', 'gold'))
            test = (any(x in parts for x in ('tests', 'benches', 'fuzz')) or
                    basename.endswith('_test.rs') or basename.endswith('_tests.rs') or
                    'test-support' in crate.casefold())
            category = ('generated/fixture' if generated_or_fixture else
                        'test' if test else 'production')
            by_crate[crate][category] += code
        print(f'[{label}] commit={commit} files={len(paths)} '
              f'code={sum(rows.values())}')
        for crate in sorted(by_crate):
            x = by_crate[crate]
            print(f'  {crate}: production={x["production"]} '
                  f'test={x["test"]} generated/fixture={x["generated/fixture"]}')
PY
```

## Candidate seams

The hub direction in every row is one-way: an engine may consume the named ZeroStack contract crate; ZeroStack must not import an engine. The deletion ranges are expected **net production** LOC per migrated engine. They are evidence-based review estimates from the overlapping source surfaces below, after retaining engine adapters and subtracting test/generated/fixture code. They are not benchmark or percentage claims.

| seam | exact source/test anchors | minimal public API sketch; hub direction | expected net production LOC deleted |
|---|---|---|---:|
| ZeroRef grammar, selectors, and CAS | TZ `crates/tokenzero-recovery/src/shared_cas.rs`; FS `src/core/zeroref.rs`; GZ `crates/graphzero-store/src/store/zeroref.rs` and `crates/graphzero-store/src/store/shared_cas.rs`; hub `crates/zero-ref/src/lib.rs` and `crates/zero-store/src/cas.rs`; tests listed in the subsection below | `ZeroRef`, `Digest`, `ObjectId`, `SpanRef`, `RefSelector`, `CasStore::{put,get,has}` with canonical lower-hex validation. Consumers depend on `ZeroStack::zero-ref`; CAS implementation depends on `ZeroStack::zero-store`, not on an engine. | TokenZero **180--320**; FSZero **180--360**; GraphZero **220--420** |
| 1TP atoms and ACK/2 | TZ `crates/tokenzero-core/src/protocol_atoms.rs`; FS `src/core/op_result.rs`, `src/codemode/host.rs`; GZ `crates/graphzero-query/src/codemode/response.rs`; hub `crates/zero-abi/src/result.rs`; exact tests/fixtures below | `OneTokenAtom`, `OneTokenAtomSet`, `is_verified_one_token_atom`, `Ack2::{accepted,rejected}`, and a versioned fixture/hash accessor. Put wire contracts in `ZeroStack::zero-abi`; TokenZero retains tokenizer/provider verification. | TokenZero **80--180**; FSZero **20--80**; GraphZero **20--80**. The latter two ranges cover ACK/2 only, not tokenizer behavior. |
| QuickJS CodeMode host, sandbox, and plan wrapping | TZ engine-local runtime retired under Gate C; proof in `docs/gate-c-semantic-retirement.md`; FS/GZ historical anchors remain engine-owned until their cutovers; hub `crates/zero-codemode/src/host.rs`, `wrap.rs`, and `limits.rs` | `CodeModeHost`, `SandboxLimits`, `Plan`, `PlanStep`, `WrappedResult`, and `execute(plan, limits)`. Engines retain domain operations, aggregate bindings, and raw-worker adapters. | TokenZero **10,625 lines removed from the worker source candidate set**; FSZero **300--600**; GraphZero **300--650** |
| Telemetry and accounting | TZ `crates/tokenzero-engine/src/usage_telemetry.rs` and `crates/tokenzero-engine/src/metrics.rs`; FS `src/core/usage_telemetry.rs` and `src/core/telemetry.rs`; GZ `crates/graphzero-store/src/store/usage_telemetry.rs` and `crates/graphzero-store/src/store/telemetry.rs`; hub `crates/zero-abi/src/telemetry.rs`; inline tests and schemas below | `UsageRecord`, `AccountingEvent`, `Receipt`, `merge`, and validation against declared units. Depend on `ZeroStack::zero-abi`; engine code supplies provider/model/domain labels. No unlabeled percentages. | TokenZero **100--220**; FSZero **80--180**; GraphZero **80--180** |
| Error/result envelopes | TZ `crates/tokenzero-engine/src/codemode_wire.rs` retains aggregate envelope compatibility; raw-worker envelopes live in `raw_worker_v2_impl.rs`; FS/GZ anchors remain as listed in their repos; hub `crates/zero-abi/src/result.rs` | `ResultEnvelope<T>`, `ErrorEnvelope`, `ErrorCode`, `Ack2`, and stable serialization/version checks. Depend on `ZeroStack::zero-abi`; engines retain domain error construction and recovery policy. | TokenZero local plan-result persistence retired; FSZero **80--180**; GraphZero **80--180** |

### ZeroRef grammar, selectors, and CAS evidence

- **TokenZero:** `crates/tokenzero-recovery/src/shared_cas.rs` defines `SharedCas`, `SharedCasError`, `SharedCas::{publish,resolve,list_objects,repair_object}`, and GC records such as `GcConfig` and `GcCandidate`. The exact ZeroRef-facing claim tests are `crates/tokenzero-mcp-compat/src/tests/zeroref_claims.rs`, `crates/tokenzero-recovery/tests/zeroref_conformance_matrix.rs`, and `crates/tokenzero-recovery/tests/zeroref_lifecycle_smokes.rs`; CAS behavior is exercised by `crates/tokenzero-recovery/tests/shared_cas_gc_hygiene.rs`, `shared_cas_gc_publish_race.rs`, and `shared_cas_publish_lease.rs`. This is local CAS/interop evidence, not a claim that TokenZero owns the portable grammar.
- **FSZero:** `src/core/zeroref.rs` exposes `select_fragment` and imports the shared `ZeroRefV1`, `ZeroFragment`, `ZeroRefError`, and `ZeroScheme` contract types. `src/core/zeroref_fixture.rs` exposes `run_put`, `run_put_bytes`, `run_expand`, and `CasStore::for_store_root`. Exact tests are `tests/zeroref_expand.rs`, `tests/zeroref_fixture.rs`, `tests/zeroref_v1_contract.rs`, and `tests/cas.rs`.
- **GraphZero:** `crates/graphzero-store/src/store/zeroref.rs` imports `ZeroRefV1`, `ZeroFragment`, `ZeroRefError`, `ZeroScheme`, `content_hash_hex`, and `is_full_lower_hex`; `zeroref_capability.rs` defines `ZeroRefDescriptor`, `SharedCasCapability`, and `validate_peer_descriptor`; `shared_cas.rs` defines `SharedCas::{put,put_limited,get_verified}`. Exact tests are `crates/graphzero-store/tests/zeroref_capability_contract.rs`, `zeroref_conformance_gate.rs`, `zeroref_fragment_conformance.rs`, `zeroref_v1.rs`, `shared_cas_contract.rs`, and `shared_cas_gc_roots.rs`.
- **ZeroStack hub:** at the recorded snapshot, `crates/zero-ref/src/lib.rs` defines `ZeroRefV1::{parse,select,verify_and_select}`, `ZeroRefError`, `ZeroFragment`, `ZeroScheme`, `Digest`, `ObjectId`, `SpanRef`, `select_fragment`, `content_hash_hex`, and `is_full_lower_hex`. `crates/zero-store/src/cas.rs` defines `CasError`, `PutOutcome`, `SharedCas::{put,put_limited,put_prehashed,get_verified,list_objects}`, and publish/sweep locks. Exact tests are `crates/zero-ref/tests/golden_vectors.rs`, `property_identity.rs`, `span_ref.rs`, plus inline `#[cfg(test)]` modules in `crates/zero-store/src/cas.rs`; the shared fixture is `crates/zero-ref/fixtures/zeroref_v1_vectors.json`.

### 1TP atoms and ACK/2 evidence

- **TokenZero:** `crates/tokenzero-core/src/protocol_atoms.rs` defines `ProtocolTokenizer`, `PORTABLE_ONE_TOKEN_ATOMS`, `is_verified_one_token_atom`, `AckClass`, and `render_ack`. Exact tests are `crates/tokenzero-core/tests/protocol_atoms.rs`; exact fixtures are `crates/tokenzero-core/tests/fixtures/one-token-atoms.json` and `crates/tokenzero-core/tests/fixtures/ack2-golden.json`. `crates/tokenzero-mcp-compat/tests/jsonrpc_conformance.rs` is the additional wire-facing compatibility test. TokenZero retains tokenizer identity and verification.
- **FSZero:** no local tokenizer/1TP atom implementation is credited in this inventory. The exact ACK surfaces are `src/core/op_result.rs::visible_ack` and `src/codemode/host.rs::{ack_with_refs,payload_tool_result,plan_tool_result}`. Exact current tests are `tests/operation_abi.rs`, `tests/operation_abi_unit.rs`, and `tests/codemode_cli_envelope.rs`.
- **GraphZero:** no local tokenizer/1TP atom implementation is credited in this inventory. The exact wire/result surface is `crates/graphzero-query/src/codemode/response.rs` using `BindingResult`, `CodeModeResponse`, `CodeModeError`, and `CodeModeTelemetry`; exact current tests are `crates/graphzero-query/tests/operation_abi_contract.rs`, `codemode_e2e.rs`, and `raw_worker_v2_shared_conformance.rs`.
- **ZeroStack hub:** `crates/zero-abi/src/result.rs` defines `ZeroResultV1::{inline,reference,inline_value,reference_value,preview}`, `ZeroResultBuildError`, and `ZeroResultAccessError`; the `ack` field is validated by `validate_ack`. Exact tests are `tests/tests/zero_result_v1.rs`, `crates/zero-codemode/tests/host_contract.rs`, and the schema `tests/contracts/zero-result-v1.schema.json`. The hub supplies the wire envelope, not a model tokenizer.

### QuickJS CodeMode host, sandbox, and plan wrapping evidence

- **TokenZero:** Gate C removed the engine-local planner, sandbox, parser, journals, recipes, and executor hook. `crates/tokenzero-codemode/src/main.rs` is now the sole worker source and launches raw-worker v2. `crates/tokenzero-engine/src/codemode_wire.rs` retains aggregate envelope metadata but no execute hook. Exact current proof is `crates/tokenzero/tests/gate_c_retirement_contract.rs`, `crates/tokenzero/tests/raw_worker_v2_packaged_conformance.rs`, and `crates/tokenzero-engine/tests/codemode_bindings_dispatcher.rs`.
- **FSZero:** `src/codemode/host.rs` defines `ContractError`, `finish`, `finish_error`, `ack_with_refs`, `payload_tool_result`, and `plan_tool_result`; `js.rs` defines `JsHost`, `execute_js_plan`, and `with_host_boundary`; `plan.rs` defines `looks_like_js_plan` and `execute_plan`; `limits.rs` defines `effective_max_wall_ms`. Exact tests are `tests/codemode_bindings.rs`, `tests/codemode_cli_envelope.rs`, `tests/codemode_deadline.rs`, `tests/codemode_fusion.rs`, and `tests/codemode_limit_enforcement.rs`.
- **GraphZero:** `crates/graphzero-query/src/codemode/quickjs.rs` defines `QuickJsHostStateSlot`, `execute_code_plan`, `execute_quickjs_code_plan`, and `deny_ambient_js_capabilities`; `plan.rs` defines `PlanKind`, `classify_plan`, and `execute_recipe`; `response.rs` defines result persistence around `BindingResult`, `CodeModeError`, and `CodeModeTelemetry`. Exact tests are `crates/graphzero-query/tests/codemode_bindings_parity.rs`, `codemode_e2e.rs`, and `crates/graphzero-store/tests/codemode_ref_contract.rs`.
- **ZeroStack hub:** at the recorded snapshot, `crates/zero-codemode/src/host.rs` defines `Host`, `Connector`, `CapabilityDescriptor`, `Host::execute`, `Host::execute_with_cancel`, and `HostError`; `limits.rs` defines `HostLimits` and `LimitError`; `wrap.rs` defines `validate_plan`, `wrap_plan`, and `PlanError`. Exact tests are `crates/zero-codemode/tests/host_contract.rs`, `edit_protocol_conformance.rs`, and `worker_adapter.rs`. The hub can own neutral QuickJS host limits, plan wrapping, cancellation, and result normalization; engines retain domain connectors.

### Telemetry and accounting evidence

- **TokenZero:** `crates/tokenzero-engine/src/usage_telemetry.rs` defines `ExecutionPath`, `UsageRecord`, `record_usage`, `record_mcp_accounting`, `record_codemode_accounting`, `TelemetryInspection`, `AmplificationRecord`, and `replay_ta_table`; `metrics.rs` defines `ToolMetrics`. Exact current test paths are `crates/tokenzero-engine/src/usage_telemetry_inline_tests.rs` and `crates/tokenzero-engine/tests/zero_ledger_pin.rs`.
- **FSZero:** `src/core/usage_telemetry.rs` defines `ExecutionPath`, `UsageRecord`, `record_usage`, `record_path_accounting`, `record_mcp_accounting`, `record_codemode_accounting`, and `UsageTelemetryInspection`; `src/core/telemetry.rs` defines `LocalTokenCounters`, `TelemetryPayload`, and `inspect_telemetry`. Exact current test paths are inline `#[cfg(test)]` modules in `src/core/usage_telemetry.rs` and `src/core/telemetry.rs`; the tracked contract test is `tests/contract_live_matrix.rs`.
- **GraphZero:** `crates/graphzero-store/src/store/usage_telemetry.rs` defines `ExecutionPath`, `UsageRecord`, `record_usage`, `record_mcp_accounting`, `record_codemode_accounting`, and `UsageTelemetryInspection`; `telemetry.rs` defines `LocalTokenCounters`, `TelemetryPayload`, and `inspect_telemetry`; `crates/graphzero-query/src/accounting.rs` defines `PreventedReadAccounting` and `accounting_for_evidence_refs`. Exact current test paths are inline `#[cfg(test)]` modules in both telemetry source files and `crates/graphzero-query/tests/stage_histogram_sink.rs`.
- **ZeroStack hub:** at the recorded snapshot, `crates/zero-abi/src/telemetry.rs` defines `TelemetrySchema`, `ZeroTelemetryV1`, `TelemetryCounter`, `TelemetryOverflow`, `checked_accumulate`, and `checked_merge`. Exact current test paths are inline `#[cfg(test)]` tests in `crates/zero-abi/src/telemetry.rs`, `tests/contracts/telemetry.schema.json`, and `tests/contracts/telemetry.json`. The hub contract supplies bounded counters; engines supply provider/model/domain labels.

### Error/result envelope evidence

- **TokenZero:** `crates/tokenzero-engine/src/codemode_wire.rs` retains `CodeModeResult`, `CodeModeError`, `CodeModeTelemetry`, and operation classification as aggregate compatibility metadata. Raw-worker result/ref/effect/accounting truth lives in the engine raw-worker modules. Exact current tests are `crates/tokenzero/tests/gate_c_retirement_contract.rs`, `crates/tokenzero-engine/tests/codemode_bindings_dispatcher.rs`, `crates/tokenzero-mcp-compat/tests/jsonrpc_conformance.rs`, and the ACK fixture.
- **FSZero:** `src/core/op_result.rs` defines `visible_ack`; `src/codemode/zero_result.rs` defines `zero_result_from_fs_step`, `zero_result_to_wire`, `canonical_zeroref`, and `wrong_accessor_message`; `src/codemode/host.rs` defines `ContractError`, `finish`, and `finish_error`. Exact current tests are `tests/operation_abi.rs`, `tests/operation_abi_unit.rs`, `tests/codemode_cli_envelope.rs`, and `tests/raw_worker_v2_shared_conformance.rs`.
- **GraphZero:** `crates/graphzero-query/src/codemode/errors.rs` defines typed constructors including `validation_error`, `policy_error`, `cancelled_error`, `busy_error`, `deadline_exceeded_error`, `not_found_error`, `runtime_error`, and `sandbox_error`; `response.rs` handles `BindingResult`, `CodeModeError`, and `CodeModeTelemetry`; `types.rs` contains the shared response types. Exact current tests are `crates/graphzero-query/tests/operation_abi_contract.rs`, `codemode_e2e.rs`, and `raw_worker_v2_shared_conformance.rs`.
- **ZeroStack hub:** at the recorded snapshot, `crates/zero-abi/src/result.rs` defines `ZeroResultV1`, `ZeroResultBuildError`, `ZeroResultAccessError`, `inline`, `reference`, and strict accessors; `crates/zero-codemode/src/host.rs` defines `normalize_public_result`, `declared_zero_result`, `HostError`, and `public_result_ack`. Exact current tests are `tests/tests/zero_result_v1.rs`, `crates/zero-codemode/tests/host_contract.rs`, and `tests/contracts/zero-result-v1.schema.json`.

These ranges intentionally do not add overlapping seams together. A migration must measure the post-migration tree and remove only duplicate production code; fixtures, tests, adapters, and engine-specific policy are not deletion credit.

## Logic that must not move

- **TokenZero:** exact tokenizer identity; model/provider tokenization; stable-prefix geometry; Decision Views; provider eligibility versus reported hit; opaque reasoning-state transport; headroom; continuation classes; output novelty; model-specific expansion, recall, and ranking. `ProtocolTokenizer` cannot be replaced by a generic hub tokenizer.
- **FSZero:** byte/state ownership; filesystem durability and replacement; journal/recovery ordering; GC and store-root policy; byte-oriented CAS details that are not part of the shared contract.
- **GraphZero:** graph structure and mutation semantics; query planning/execution; indexes; graph-specific refs and traversal; graph consistency and domain accounting.
- **All engines:** domain adapters, capability/policy decisions, engine-specific limits, provider behavior, and recovery choices remain local. The hub can define contracts and neutral infrastructure, but must not absorb engine meaning.

## RFC cross-reference

`docs/zero-foundation-rfc.md` is draft/recommendation-only. Its boundary and supply-chain sections name ZeroStack as the hub, prohibit engine-to-engine and direct FSZero/GraphZero dependencies, and require engine consumers to pin a pushed immutable hub revision. Its ownership/release comparison evaluates alternatives but does not select one. This inventory therefore names candidate hub crates and APIs without choosing ownership, release, or extraction order. The current TokenZero manifests pin ZeroStack dependencies to pushed revision `fa253840910ab4051635e2de95f04ddf6043a000`; that dependency pin is separate from older recorded snapshots above.

## Reproduction and path checks

The following read-only check covers every source and test path named in the seam subsections. It uses `HEAD` for the three engine trees and the recorded immutable commit for ZeroStack.

```sh
check_paths() {
  root=$1
  commit=$2
  shift 2
  for path in "$@"; do
    git -C "$root" cat-file -e "$commit:$path" || {
      echo "missing: $root@$commit:$path" >&2
      return 1
    }
  done
}
check_paths . HEAD \
  crates/tokenzero-recovery/src/shared_cas.rs \
  crates/tokenzero-mcp-compat/src/tests/zeroref_claims.rs \
  crates/tokenzero-recovery/tests/zeroref_conformance_matrix.rs \
  crates/tokenzero-recovery/tests/zeroref_lifecycle_smokes.rs \
  crates/tokenzero-recovery/tests/shared_cas_gc_hygiene.rs \
  crates/tokenzero-recovery/tests/shared_cas_gc_publish_race.rs \
  crates/tokenzero-recovery/tests/shared_cas_publish_lease.rs \
  crates/tokenzero-core/src/protocol_atoms.rs \
  crates/tokenzero-core/tests/protocol_atoms.rs \
  crates/tokenzero-core/tests/fixtures/one-token-atoms.json \
  crates/tokenzero-core/tests/fixtures/ack2-golden.json \
  crates/tokenzero-mcp-compat/tests/jsonrpc_conformance.rs \
  crates/tokenzero-codemode/src/main.rs \
  crates/tokenzero-engine/src/codemode_wire.rs \
  crates/tokenzero/tests/gate_c_retirement_contract.rs \
  crates/tokenzero/tests/raw_worker_v2_packaged_conformance.rs \
  crates/tokenzero-engine/tests/codemode_bindings_dispatcher.rs \
  crates/tokenzero-engine/src/usage_telemetry.rs \
  crates/tokenzero-engine/src/usage_telemetry_inline_tests.rs \
  crates/tokenzero-engine/src/metrics.rs \
  crates/tokenzero-engine/tests/zero_ledger_pin.rs
check_paths ../FSZero HEAD \
  src/core/zeroref.rs src/core/zeroref_fixture.rs \
  tests/zeroref_expand.rs tests/zeroref_fixture.rs tests/zeroref_v1_contract.rs tests/cas.rs \
  src/core/op_result.rs src/codemode/host.rs src/codemode/zero_result.rs \
  src/codemode/js.rs src/codemode/plan.rs src/codemode/limits.rs \
  tests/operation_abi.rs tests/operation_abi_unit.rs tests/codemode_cli_envelope.rs \
  tests/raw_worker_v2_shared_conformance.rs tests/codemode_bindings.rs \
  tests/codemode_deadline.rs tests/codemode_fusion.rs tests/codemode_limit_enforcement.rs \
  src/core/usage_telemetry.rs src/core/telemetry.rs tests/contract_live_matrix.rs
check_paths ../GraphZero HEAD \
  crates/graphzero-store/src/store/zeroref.rs \
  crates/graphzero-store/src/store/zeroref_capability.rs \
  crates/graphzero-store/src/store/shared_cas.rs \
  crates/graphzero-store/tests/zeroref_capability_contract.rs \
  crates/graphzero-store/tests/zeroref_conformance_gate.rs \
  crates/graphzero-store/tests/zeroref_fragment_conformance.rs \
  crates/graphzero-store/tests/zeroref_v1.rs \
  crates/graphzero-store/tests/shared_cas_contract.rs \
  crates/graphzero-store/tests/shared_cas_gc_roots.rs \
  crates/graphzero-query/src/codemode/response.rs \
  crates/graphzero-query/src/codemode/errors.rs \
  crates/graphzero-query/src/codemode/types.rs \
  crates/graphzero-query/src/codemode/quickjs.rs \
  crates/graphzero-query/src/codemode/plan.rs \
  crates/graphzero-query/tests/operation_abi_contract.rs \
  crates/graphzero-query/tests/codemode_e2e.rs \
  crates/graphzero-query/tests/raw_worker_v2_shared_conformance.rs \
  crates/graphzero-query/tests/codemode_bindings_parity.rs \
  crates/graphzero-store/tests/codemode_ref_contract.rs \
  crates/graphzero-store/src/store/usage_telemetry.rs \
  crates/graphzero-store/src/store/telemetry.rs \
  crates/graphzero-query/src/accounting.rs \
  crates/graphzero-query/tests/stage_histogram_sink.rs
check_paths ../ZeroStack ca8636b44a8f6b53ec06cc41def989e746ce6c3f \
  crates/zero-ref/src/lib.rs crates/zero-ref/fixtures/zeroref_v1_vectors.json \
  crates/zero-ref/tests/golden_vectors.rs crates/zero-ref/tests/property_identity.rs \
  crates/zero-ref/tests/span_ref.rs crates/zero-store/src/cas.rs \
  crates/zero-abi/src/result.rs crates/zero-abi/src/telemetry.rs \
  crates/zero-codemode/src/host.rs crates/zero-codemode/src/limits.rs \
  crates/zero-codemode/src/wrap.rs crates/zero-codemode/tests/host_contract.rs \
  crates/zero-codemode/tests/edit_protocol_conformance.rs \
  crates/zero-codemode/tests/worker_adapter.rs \
  tests/tests/zero_result_v1.rs tests/contracts/zero-result-v1.schema.json \
  tests/contracts/telemetry.schema.json tests/contracts/telemetry.json

git -C . rev-parse HEAD
git -C ../FSZero rev-parse HEAD
git -C ../GraphZero rev-parse HEAD
git -C ../ZeroStack rev-parse HEAD
git diff --check
```

No Cargo, rustc, fuzzing, bead, papercut, staging, commit, push, or sibling-repository write was performed for this inventory.
