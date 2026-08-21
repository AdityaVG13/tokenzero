use std::sync::Arc;

use tempfile::tempdir;
use tokenzero_kernel::ZeroTokenEngine;
use zero_abi::{
    CancellationProbe, CompressionRequest, EngineCallContext, EngineInvocation, ExpandOptions,
    KernelBudget, ProjectionRequest, TokenEngine,
};

struct NeverCancel;
impl CancellationProbe for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn invocation(root: &std::path::Path) -> EngineInvocation {
    EngineInvocation {
        context: EngineCallContext {
            workspace_root: root.to_path_buf(),
            project_root: root.to_path_buf(),
            session_id: "session".into(),
            cell_id: "cell".into(),
            trace_id: "trace".into(),
            deadline_unix_ms: u64::MAX,
            budget: KernelBudget {
                wall_ms: 1_000,
                cpu_ms: 1_000,
                memory_bytes: 64 * 1024 * 1024,
                call_limit: 64,
                task_limit: 8,
                output_byte_limit: 64 * 1024,
            },
        },
        cancellation: Arc::new(NeverCancel),
    }
}

#[test]
fn exact_tokenizer_measurement_and_inline_projection() {
    let store = tempdir().unwrap();
    let engine = ZeroTokenEngine::open(store.path(), Some("gpt-4o".into()));
    let call = invocation(store.path());
    let measured = engine.measure(&call, b"hello world").unwrap();
    assert!(measured.certified, "{measured:?}");
    assert!(measured.billed > 0);
    let result = engine
        .project(
            &call,
            ProjectionRequest {
                bytes: b"hello world".to_vec(),
                visible_byte_limit: 128,
                media_type: "text/plain".into(),
            },
        )
        .unwrap();
    assert_eq!(result.visible, "hello world");
    assert_eq!(result.visible_source_bytes, 11);
    assert!(result.exact.is_none());
}

#[test]
fn oversized_projection_is_bounded_and_exactly_expandable() {
    let store = tempdir().unwrap();
    let engine = ZeroTokenEngine::open(store.path(), Some("gpt-4o".into()));
    let call = invocation(store.path());
    let source = (0..2_000)
        .map(|index| format!("line-{index}\n"))
        .collect::<String>();
    let result = engine
        .project(
            &call,
            ProjectionRequest {
                bytes: source.as_bytes().to_vec(),
                visible_byte_limit: 256,
                media_type: "text/plain".into(),
            },
        )
        .unwrap();
    assert!(result.visible.len() <= 256);
    assert!(result.visible_source_bytes > 0);
    assert!(result.visible_source_bytes < source.len() as u64);
    let handle = result.exact.expect("exact handle");
    assert!(result.visible.contains(handle.as_str()));
    assert_eq!(
        engine
            .expand(&call, &handle, ExpandOptions::default())
            .unwrap(),
        source.as_bytes()
    );
    assert_eq!(
        engine
            .expand(
                &call,
                &handle,
                ExpandOptions {
                    line_start: Some(2),
                    line_end: Some(2),
                    ..ExpandOptions::default()
                }
            )
            .unwrap(),
        b"line-1\n"
    );
}
#[test]
fn compress_repetitive_input_yields_bounded_digest_and_honest_accounting() {
    let store = tempdir().unwrap();
    let engine = ZeroTokenEngine::open(store.path(), Some("gpt-4o".into()));
    let call = invocation(store.path());
    // 50x identical lines ~2300 bytes, 400 billed tokens in the papercut.
    // Any small max_tokens must force a bounded visible digest with honest
    // visible < billed accounting; the exact handle must still recover.
    let line = "the quick brown fox jumps over the lazy dog -- repeated line\n";
    let source = line.repeat(50);
    assert!(
        source.len() > 2000,
        "fixture must be oversized: {}",
        source.len()
    );
    let result = engine
        .compress(
            &call,
            CompressionRequest {
                bytes: source.as_bytes().to_vec(),
                max_tokens: 100,
                mode: String::new(),
                label: None,
                media_type: "text/plain".into(),
            },
        )
        .unwrap();
    assert!(
        result.visible.len() < source.len(),
        "visible must be bounded digest: visible {} vs source {}",
        result.visible.len(),
        source.len()
    );
    assert_ne!(result.visible.trim_end(), source.trim_end());
    assert!(
        result.accounting.visible < result.accounting.billed,
        "telemetry honesty: visible {} must be < billed {} (tokenizer {})",
        result.accounting.visible,
        result.accounting.billed,
        result.accounting.tokenizer
    );
    assert!(
        result.visible.contains("omitted")
            || result.visible.contains("repeated")
            || result.visible.contains("exact ref")
            || result.visible.contains(result.exact.as_str()),
        "digest must carry omitted-span or recovery marker: {}",
        result.visible
    );
    // Exact handle preserves full recovery.
    let expanded = engine
        .expand(&call, &result.exact, ExpandOptions::default())
        .unwrap();
    assert_eq!(expanded, source.as_bytes());
}
