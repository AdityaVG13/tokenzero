use super::*;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn default_and_invalid_opt_in_stay_disabled() {
    assert!(!telemetry_env_enabled(None));
    assert!(!telemetry_env_enabled(Some("")));
    assert!(!telemetry_env_enabled(Some("0")));
    assert!(!telemetry_env_enabled(Some("false")));
    assert!(!telemetry_env_enabled(Some("off")));
    assert!(!telemetry_env_enabled(Some("no")));
    assert!(!telemetry_env_enabled(Some("invalid")));
    assert!(!telemetry_env_enabled(Some("maybe")));
    assert!(telemetry_env_enabled(Some("1")));
    assert!(telemetry_env_enabled(Some("ON")));
    assert!(telemetry_env_enabled(Some(" true ")));
    assert!(telemetry_env_enabled(Some("Yes")));
}

#[test]
fn disabled_recording_creates_no_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("usage-telemetry.jsonl");
    let record = UsageRecord::try_new(ExecutionPath::Mcp, 100, 40).unwrap();
    record_usage(&path, false, &record).unwrap();
    assert!(!path.exists(), "disabled path must not create a file");
    let inspection = inspect_usage_telemetry(&path, false).unwrap();
    assert!(!inspection.enabled);
    assert_eq!(inspection.exporter, "none");
    assert!(inspection.records.is_empty());
}

#[test]
fn explicit_opt_in_records_exactly_three_fields_for_mcp_and_codemode() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let path = usage_telemetry_path_for_cache(&cache);

    record_mcp_accounting(&cache, true, 200, 50);
    record_codemode_accounting(&cache, true, 400, 120);

    let inspection = inspect_usage_telemetry(&path, true).unwrap();
    assert!(inspection.enabled);
    assert_eq!(
        inspection.records,
        vec![
            UsageRecord {
                execution_path: ExecutionPath::Mcp,
                raw_tokens: 200,
                spent_tokens: 50,
            },
            UsageRecord {
                execution_path: ExecutionPath::Codemode,
                raw_tokens: 400,
                spent_tokens: 120,
            },
        ]
    );

    let raw = fs::read_to_string(&path).unwrap();
    for line in raw.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let obj = value.as_object().unwrap();
        let mut keys: Vec<_> = obj.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "execution_path".to_string(),
                "raw_tokens".to_string(),
                "spent_tokens".to_string()
            ]
        );
    }
}

#[test]
fn spent_exceeding_raw_is_rejected() {
    let err = UsageRecord::try_new(ExecutionPath::Mcp, 10, 11).unwrap_err();
    assert_eq!(
        err,
        UsageTelemetryError::SpentExceedsRaw {
            spent_tokens: 11,
            raw_tokens: 10,
        }
    );
    let dir = tempdir().unwrap();
    let path = dir.path().join("usage-telemetry.jsonl");
    let bad = UsageRecord {
        execution_path: ExecutionPath::Mcp,
        raw_tokens: 10,
        spent_tokens: 11,
    };
    let err = record_usage(&path, true, &bad).unwrap_err();
    assert!(matches!(err, UsageTelemetryError::SpentExceedsRaw { .. }));
    assert!(!path.exists());
}

#[test]
fn schema_rejects_non_allowlisted_fields() {
    assert_eq!(
        UsageRecord::ALLOWLISTED_FIELDS,
        &["execution_path", "raw_tokens", "spent_tokens"]
    );
    let with_extra = json!({
        "execution_path": "mcp",
        "raw_tokens": 1,
        "spent_tokens": 1,
        "tool": "read"
    });
    let err = serde_json::from_value::<UsageRecord>(with_extra).unwrap_err();
    assert!(
        err.to_string().contains("unknown field"),
        "expected deny_unknown_fields, got {err}"
    );

    let with_session = json!({
        "execution_path": "codemode",
        "raw_tokens": 2,
        "spent_tokens": 1,
        "session_id": "secret"
    });
    assert!(serde_json::from_value::<UsageRecord>(with_session).is_err());
}

#[test]
fn allowlisted_round_trip_snapshot() {
    let record = UsageRecord::try_new(ExecutionPath::Codemode, 99, 33).unwrap();
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(
        value,
        json!({
            "execution_path": "codemode",
            "raw_tokens": 99,
            "spent_tokens": 33
        })
    );
    let back: UsageRecord = serde_json::from_value(value).unwrap();
    assert_eq!(back, record);
}
