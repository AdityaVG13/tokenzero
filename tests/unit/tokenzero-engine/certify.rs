//! Certify primitive: re-measurement must be deterministic, catch tampered
//! bytes, and distinguish the BPE path from the lexical estimate path.

use std::path::PathBuf;
use std::sync::Arc;

use tokenzero_engine::ZeroTokenEngine;
use zero_abi::{
    CancellationProbe, EngineCallContext, EngineInvocation, KernelBudget, TokenAccounting,
    TokenEngine,
};

struct NoopCancel;
impl CancellationProbe for NoopCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn invocation() -> EngineInvocation {
    let root = tempfile::tempdir().unwrap();
    let context = EngineCallContext {
        workspace_root: root.path().to_path_buf(),
        project_root: root.path().to_path_buf(),
        session_id: "certify-test".into(),
        cell_id: "cell-1".into(),
        trace_id: "certify-test-cell-1".into(),
        deadline_unix_ms: u64::MAX,
        budget: KernelBudget {
            wall_ms: 1_000,
            cpu_ms: 1_000,
            memory_bytes: 64 * 1024 * 1024,
            call_limit: 64,
            task_limit: 8,
            output_byte_limit: 64 * 1024,
        },
    };
    EngineInvocation {
        context,
        cancellation: Arc::new(NoopCancel),
    }
}

fn engine() -> ZeroTokenEngine {
    // Fresh temp store per call keeps tests hermetic; count() never touches CAS.
    ZeroTokenEngine::open(tempfile::tempdir().unwrap().into_path(), None)
}

#[test]
fn certify_is_deterministic_and_matches_fresh_measurement() {
    let invocation = invocation();
    let engine = engine();
    let bytes = b"the quick brown fox jumps over the lazy dog";
    let claimed = engine.measure(&invocation, bytes).unwrap();
    let result = engine.certify(&invocation, bytes, &claimed).unwrap();
    assert!(result.matches, "identical bytes must match the claim");
    assert_eq!(result.recomputed, claimed);
}

#[test]
fn certify_detects_tampered_bytes() {
    let invocation = invocation();
    let engine = engine();
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
    let invocation = invocation();
    let engine = engine();
    let bytes = b"determinism probe for tokenizer identity";
    let mut forged = engine.measure(&invocation, bytes).unwrap();
    forged.tokenizer = "forged-tokenizer".into();
    let result = engine.certify(&invocation, bytes, &forged).unwrap();
    assert!(!result.matches, "tokenizer identity is part of the claim");
    assert_ne!(result.recomputed.tokenizer, "forged-tokenizer");
}
