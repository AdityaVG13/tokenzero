use std::sync::Arc;

use tempfile::tempdir;
use tokenzero_kernel::ZeroTokenEngine;
use zero_abi::{
    CancellationProbe, EngineCallContext, EngineInvocation, ExpandOptions, KernelBudget,
    ProjectionRequest, TokenEngine,
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
