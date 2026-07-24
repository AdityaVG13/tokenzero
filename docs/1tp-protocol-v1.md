# ZeroStack 1TP Protocol v1

Status: normative for TokenZero; adoption target for FSZero and GraphZero.

1TP minimizes the visible wire cost of a common action to one verified token, or zero tokens for a successful pure mutation. It does not claim that arbitrary payloads cost one token. Payloads remain recoverable through stable references, while protocol atoms describe control and outcome.

## 1. Conformance language

The key words MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY are normative. A conforming engine MUST pass the fixtures and tests named in each section. Wire changes require a new protocol version or a fixture revision; silently changing an existing atom is forbidden.

## 2. Identities and gauge

ZeroStack separates three identities:

- **Byte identity**: the content hash of exact stored bytes. Equivalent aliases MUST expand byte-identically.
- **Wire identity**: the canonical encoded reference or protocol atom emitted to the model. Wire identity includes the scheme (`tz://`, `fz://`, or `gz://`), generation, ordinal/selector, and protocol version.
- **Semantic identity**: the meaning of the recovered object after decoding. Semantic equality MUST NOT be used as proof of byte equality.

A **gauge orbit** is the set of full, short, and ordinal aliases for one byte identity. Changing gauge MUST preserve recoverability and selector semantics. Ordinal namespaces are engine-scoped; the same integer in `tz://`, `fz://`, and `gz://` is not the same object.

TokenZero grounds this contract in the dense, store-arbitrated session ordinal implementation from commit `636d0c4`. Allocation is linearizable per generation, and aliases are generation-bound. Conformance: `crates/tokenzero-recovery/tests/session_ordinals.rs` (byte-identical full/short/ordinal expansion and concurrent allocation) and `docs/zeroref-v1-contract.md`.

## 3. Verified atoms

An atom used as a one-token opcode or ACK MUST be certified for the active tokenizer table. Certification is a versioned artifact, not a runtime substring heuristic. The portable v1 intersection is the ASCII digits `0` through `9`; provider-specific extensions MUST NOT be emitted unless the negotiated tokenizer table certifies them.

Grounding: commit `e1e73bb`, `crates/tokenzero-core/src/protocol_atoms.rs`, and `fixtures/one-token-atoms.json`. Conformance: `crates/tokenzero-core/tests/protocol_atoms.rs` verifies every protocol atom against every supported table (Anthropic, o200k, Gemini, and Kimi).

## 4. ACK/2

ACK/2 is deterministic class-1 outcome grammar:

| Atom | Class |
|---|---|
| empty | successful pure mutation when silence is permitted |
| `0` | success |
| `1` | validation / invalid arguments / parse |
| `2` | policy / sandbox / permission |
| `3` | substrate / store / not found / I/O |
| `4` | retryable |
| `9` | internal |

A detail reference MAY accompany an ACK in its dedicated envelope field. Detail bytes MUST NOT be concatenated to the atom. The same class, content, tokenizer, and protocol revision MUST render byte-identically. Silence is success only for a completed pure mutation; reads, partial mutations, and failures MUST emit an atom or structured result.

Grounding: commits `70cf40f` and `14b4dbd`, `AckClass`/`render_ack` in `crates/tokenzero-core/src/protocol_atoms.rs`, and MCP envelope wiring. Conformance: `fixtures/ack2-golden.json`, `crates/tokenzero-core/tests/protocol_atoms.rs`, and ACK cases in `crates/tokenzero-mcp/src/codemode/e2e_tests.rs`.

## 5. Recipe envelopes

A recipe is a versioned server-side plan identified by name and revision. Every recipe MUST declare a composed worst-case visible-token envelope. Composition uses min-plus bounds: sequential visible costs add; alternative branches take their declared maximum after policy selection. The host MUST reject a call whose declared envelope exceeds the caller budget before executing side effects.

Discovery MUST expose recipe version, side-effect class, and visible-token bound. An implementation MUST NOT substitute a recipe revision after reservation without invalidating that reservation.

Grounding: commits `5d08320` and `29b78f6`, `crates/tokenzero-mcp/src/codemode/recipe_registry.rs`, and `fixtures/codemode-recipes.json`. Conformance: recipe registry and envelope rejection tests in `crates/tokenzero-mcp/src/codemode/e2e_tests.rs` and `crates/tokenzero-mcp/src/codemode/exec.rs`.

## 6. Sentinel channel

A sentinel call is a single verified atom interpreted only in an explicitly enabled takeover channel. Its opcode table is declared in the cached prefix and maps atom -> recipe id, recipe revision, side-effect class, and expected ACK class.

Sentinel classes are:

1. **Observe**: read-only, no reservation required.
2. **Derive**: deterministic computation over already-authorized data, no durable mutation.
3. **Stage**: creates an ephemeral candidate; it MUST NOT publish durable state.
4. **Mutate**: durable or externally visible mutation; requires an armed reservation bound to session, recipe revision, opcode, and expiry.

Outside takeover mode, an isolated glyph is ordinary text and MUST NOT execute. In takeover mode, an unknown, uncertified, stale, or unreserved mutating glyph MUST fail closed with ACK/2 validation or policy class. Reservation consumption MUST be atomic and single-use. A successful call injects one ACK atom (or permitted silence); detail is ref-backed.

The normative executable surface lands with bead `tokenzero-87ew`; until then this section is the adoption contract. Conformance requires a metered end-to-end sentinel call with single-digit total visible tokens and an interlock test proving a stray glyph cannot mutate.

## 7. TZ-EVICT/1

TZ-EVICT/1 is the eviction control message for a session-scoped alias or cached prefix. It contains protocol revision, generation, target ordinal/range, reason class, and optional recovery reference. Eviction changes residency, never byte identity.

- A receiver MUST reject an eviction whose generation does not match.
- Protected or reserved entries MUST NOT be evicted.
- A successful pure eviction MAY use silent ACK/2 success; otherwise it emits `0`.
- If bytes remain recoverable, the recovery reference MUST address the same byte identity.
- If recovery is impossible, the operation MUST declare destructive/lossy policy before execution and MUST NOT masquerade as ordinary eviction.
- Replaying the same eviction is idempotent.

Implementations MAY batch contiguous ordinals, but the wire order and resulting residency state MUST be deterministic. This contract composes with TokenZero's eviction scheduler and recovery store; adopters MUST retain their own engine-scoped ordinal namespace.

## 8. Prefix stability and omission safety

The cached opcode table, recipe declarations, and all preceding cacheable bytes are governed by the production prefix-stability guard from commit `c631eaa` (`crates/tokenzero-recovery/src/prefix_stability.rs` and `context_view.rs`). Identical content, level, and tokenizer MUST render byte-identically; existing provider breakpoints MUST remain byte-stable.

Omitted payload bytes are governed by commit `fb86821`: each omission MUST be exact-ref-backed, protected-anchor-backed, or explicitly lossy with a stable policy id and recoverability warning. Free-text summarization is not a recovery mechanism. Grounding and tests: `docs/racc.md`, `crates/tokenzero-core/src/tests/capsule.rs`, and renderer tests in `crates/tokenzero-engine`.

## 9. Adoption requirements

FSZero and GraphZero adopters MUST:

- use engine-scoped ordinals while preserving the byte/wire/semantic identity split;
- consume the same versioned atom certification fixture or prove an equivalent table;
- emit ACK/2 without engine-specific prose in the atom channel;
- expose recipe envelopes before execution and fail closed on budget violations;
- implement the sentinel interlock before enabling mutating opcodes;
- preserve selector semantics across full, short, and ordinal gauge forms;
- publish cross-engine conformance fixtures without assuming ordinal equality across schemes.

## 10. Review checklist

- [ ] Every emitted control atom is certified by `fixtures/one-token-atoms.json` for the negotiated tokenizer.
- [ ] Full, short, and ordinal aliases expand to byte-identical content within one generation.
- [ ] Byte, wire, and semantic identities are represented separately in code and tests.
- [ ] ACK/2 output matches `fixtures/ack2-golden.json`; pure-mutation silence cannot hide failure or partial execution.
- [ ] Recipe version, side-effect class, and min-plus visible-token envelope are discoverable before execution.
- [ ] Over-budget recipes are rejected before side effects.
- [ ] Sentinel takeover is explicit; stray/unknown glyphs cannot execute.
- [ ] Mutating sentinels require an atomic, single-use, session/revision/opcode-bound reservation.
- [ ] TZ-EVICT/1 is generation-safe, idempotent, and preserves byte identity when recovery is claimed.
- [ ] Prefix-stability goldens cover cached protocol declarations and provider breakpoints.
- [ ] Every omission is ref-backed, anchor-backed, or explicitly lossy.
- [ ] FSZero/GraphZero fixtures retain scheme-scoped ordinal namespaces and common selector semantics.
