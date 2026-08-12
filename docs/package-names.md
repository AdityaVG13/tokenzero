# Package name policy

Bead: `tokenzero-x67r`. Snapshot: 2026-08-12T10:41:51Z.

## Decision

Do **not** publish placeholder crates or npm packages. Placeholder publication
is an external, effectively permanent registry action. It requires a release
candidate, verified ownership credentials, supply-chain checks, and explicit
operator approval. GitHub Release remains the install authority until those
gates pass. A registry HTTP 404 means only "not found when checked"; it does
not prove that a name or npm scope can be claimed.

Canonical naming is fixed now so code and docs do not drift:

| Surface | Canonical rule | Current TokenZero names |
|---|---|---|
| Rust engine packages | `<engine>-<component>` | `tokenzero-cli`, `tokenzero-core`, `tokenzero-engine`, `tokenzero-filters`, `tokenzero-install`, `tokenzero-mcp-compat`, `tokenzero-pulse`, `tokenzero-recovery`, `tokenzero-runtime`, `tokenzero-test-support`, `tokenzero-worker` |
| User-facing Rust umbrella | `<engine>` when a release crate exists | `tokenzero` is reserved for a future umbrella; current executable package is `tokenzero-cli` |
| npm | `@<engine>/cli` | `@tokenzero/cli` |
| npm worker surface | `@<engine>/codemode` | `@tokenzero/codemode`; `@fszero/codemode`; `@graphzero/codemode` |
| npm fallback | `<engine>-<component>` | `tokenzero-codemode`; `fszero-codemode`; `graphzero-codemode` only if scoped publication is unavailable |
| Foundation crates | `zero-<component>` | Hub-owned; finalized by `tokenzero-kxze` before publication |

Peer-family targets follow the same rule: `fszero-*`, `graphzero-*`,
`@fszero/cli`, and `@graphzero/cli`. This record does not publish or claim
their names and does not change peer repositories.

The Rust worker naming intentionally differs from its rollout binary:

| Engine | Rust package target | Compatibility binary |
|---|---|---|
| TokenZero | `tokenzero-worker` | `tokenzero-codemode` |
| FSZero | `fszero-codemode` | `fszero-codemode` |
| GraphZero | `graphzero-codemode` | `graphzero-codemode` |

Do not introduce a second TokenZero Rust package named `tokenzero-codemode`;
the worker rename makes its planner-free role explicit while preserving the
existing executable name.

## Read-only registry snapshot

The crates.io API returned HTTP 404 for all current TokenZero Rust package
names above, plus `tokenzero`, `fszero`, the four current `fszero-*` component
names, `graphzero`, and the fifteen current `graphzero-*` component names. The
npm registry returned HTTP 404 for `@tokenzero/cli`, `@fszero/cli`,
`@graphzero/cli`, all three preferred scoped `@*/codemode` names, and the three
unscoped `*-codemode` fallbacks. The unscoped npm name `tokenzero` is occupied
by an unrelated third party. The crates.io name `zerostack` is also occupied
by an unrelated third party and is not a foundation target. Status for every
HTTP-404 name:
**available-observed, unreserved, and raceable**.

Reproduce without authentication or mutation:

```bash
curl -fsS -o /dev/null -w '%{http_code}\n' \
  https://crates.io/api/v1/crates/tokenzero-core
curl -fsS -o /dev/null -w '%{http_code}\n' \
  https://registry.npmjs.org/%40tokenzero%2Fcli
```

## Publication gate

Publish only when all conditions hold:

1. The exact release artifact and source revision pass release gates.
2. Registry owner/team credentials are verified without exposing secrets.
3. Names, versions, MSRV, provenance, and rollback/yank policy are reviewed.
4. The operator explicitly approves the external publication action.
5. After publication, install docs replace GitHub-only wording with verified
   registry commands and smoke output.

Until then, recheck availability before each release. Never treat this snapshot
as a reservation.
