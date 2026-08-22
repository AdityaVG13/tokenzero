//! Certify primitive: re-measurement must be deterministic, catch tampered
//! bytes, and distinguish the BPE path from the lexical estimate path.

use tokenzero_engine::ZeroTokenEngine;
use zerostack_test_support::{TempWorkspace, test_invocation};
use zero_abi::{EngineInvocation, TokenAccounting, TokenEngine};

fn workspace() -> TempWorkspace {
    TempWorkspace::new("tz-certify").unwrap()
}

/// Engine bound to the shared hub scaffolding; store lives in the hermetic
/// workspace so tests stay isolated without hand-built invocations.
fn engine(ws: &TempWorkspace) -> ZeroTokenEngine {
    ZeroTokenEngine::open(ws.store(), None)
}

fn invocation_for(ws: &TempWorkspace) -> EngineInvocation {
    test_invocation(ws.root(), "certify-test", "cell-1")
}

#[test]
fn certify_is_deterministic_and_matches_fresh_measurement() {
    let ws = workspace();
    let invocation = invocation_for(&ws);
    let engine = engine(&ws);
    let bytes = b"the quick brown fox jumps over the lazy dog";
    let claimed = engine.measure(&invocation, bytes).unwrap();
    let result = engine.certify(&invocation, bytes, &claimed).unwrap();
    assert!(result.matches, "identical bytes must match the claim");
    assert_eq!(result.recomputed, claimed);
}

#[test]
fn certify_detects_tampered_bytes() {
    let ws = workspace();
    let invocation = invocation_for(&ws);
    let engine = engine(&ws);
    let original = b"alpha beta gamma delta epsilon zeta eta theta";
    let claimed = engine.measure(&invocation, original).unwrap();

    // Appending tokens must change the lexical count, so the claim for the
    // shorter text cannot match the tampered bytes.
    let tampered = b"alpha beta gamma delta epsilon zeta eta theta plus five more words";
    let result = engine.certify(&invocation, tampered, &claimed).unwrap();
    assert!(!result.matches, "tampered bytes must not match the claim");
}

#[test]
fn certify_rejects_mismatched_tokenizer_claim() {
    let ws = workspace();
    let invocation = invocation_for(&ws);
    let engine = engine(&ws);
    let bytes = b"determinism probe for tokenizer identity";
    let mut forged = engine.measure(&invocation, bytes).unwrap();
    forged.tokenizer = "forged-tokenizer".into();
    let result = engine.certify(&invocation, bytes, &forged).unwrap();
    assert!(!result.matches, "tokenizer identity is part of the claim");
    assert_ne!(result.recomputed.tokenizer, "forged-tokenizer");
}
