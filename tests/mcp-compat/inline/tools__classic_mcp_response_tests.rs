use super::*;

#[test]
fn opt_in_mcp_usage_writes_only_the_closed_usage_record() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = crate::EngineConfig::for_root(dir.path());
    config.cache_path = dir.path().join("recovery-cache.json");
    config.telemetry_enabled = Some(true);
    let engine = TokenZeroEngine::new(config);
    let response = inline_response("read", Mode::Auto, "ok".to_string(), 2);

    record_opt_in_mcp_usage(&engine, &response);

    let usage_path = crate::usage_telemetry_path_for_cache(&engine.config.cache_path);
    let record: Value =
        serde_json::from_str(std::fs::read_to_string(usage_path).unwrap().trim()).unwrap();
    let mut fields = record
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    fields.sort();
    assert_eq!(fields, ["execution_path", "raw_tokens", "spent_tokens"]);
    assert!(!engine
        .config
        .cache_path
        .with_file_name("token-amplification.jsonl")
        .exists());
}
