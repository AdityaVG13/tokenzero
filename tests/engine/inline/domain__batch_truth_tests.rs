use super::*;
use crate::EngineConfig;

#[test]
fn direct_domain_batch_is_error_when_any_or_all_sub_operations_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let mixed = batch_response(
        &engine,
        &json!({
            "ops": [
                {"tool": "ingest", "args": {"text": "batch-retained"}},
                {"tool": "batch", "args": {"ops": []}}
            ]
        }),
    )
    .unwrap();
    assert_eq!(mixed.status, "error");
    assert_eq!(mixed.error.as_ref().unwrap().code, "batch_operation_failed");
    assert!(mixed.visible.as_ref().unwrap().text.contains("## 1 ingest"));
    assert!(mixed.accounting.is_some());
    assert!(
        !mixed.refs.is_empty(),
        "successful sub-op refs must survive"
    );
    let telemetry = mixed.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["ops"], 2);
    assert_eq!(telemetry["succeeded_ops"], 1);
    assert_eq!(telemetry["failed_ops"], 1);
    assert_eq!(telemetry["per_op"][1]["code"], "nested_batch");

    let success = batch_response(
        &engine,
        &json!({"ops": [{"tool": "ingest", "args": {"text": "success"}}]}),
    )
    .unwrap();
    assert_eq!(success.status, "ok");
    assert_eq!(success.telemetry.as_ref().unwrap()["succeeded_ops"], 1);
    assert_eq!(success.telemetry.as_ref().unwrap()["failed_ops"], 0);

    let all_failed = batch_response(
        &engine,
        &json!({
            "ops": [
                {"tool": "batch", "args": {"ops": []}},
                {"tool": "not_a_tool", "args": {}}
            ]
        }),
    )
    .unwrap();
    assert_eq!(all_failed.status, "error");
    assert_eq!(all_failed.telemetry.as_ref().unwrap()["failed_ops"], 2);
    assert_eq!(all_failed.telemetry.as_ref().unwrap()["succeeded_ops"], 0);
}
