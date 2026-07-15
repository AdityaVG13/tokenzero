use super::*;

#[test]
fn session_working_set_eviction_is_visible_in_metrics() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("large.rs");
    let body = (0..300)
        .map(|index| format!("line {index}: eviction telemetry fixture\n"))
        .collect::<String>();
    fs::write(&file, body).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    *engine.working_set.lock().unwrap() = tokenzero_recovery::working_set::WorkingSet::new(1);

    let response = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);
    let visible = response.visible.unwrap().text;
    assert!(
        visible.starts_with("TZ-EVICT/1 ref=tz://blob/"),
        "{visible}"
    );
    assert!(!visible.contains(" symbol="), "{visible}");
    assert!(visible.contains(" lines=1-"), "{visible}");

    let repeated = engine.read(
        &[dir.path().join("large.rs")],
        Mode::Auto,
        None,
        None,
        false,
        20,
        4000,
    );
    let repeated_visible = repeated.visible.unwrap().text;
    assert!(
        repeated_visible.starts_with("TZ-EVICT/1 ref=tz://blob/"),
        "an eviction marker must not seed dedup with bytes that were never returned: {repeated_visible}"
    );

    let metrics = engine.tool_metrics_snapshot();
    assert_eq!(metrics["working_set"]["evictions"], 2);
    assert!(metrics["working_set"]["bytes_evicted"].as_u64().unwrap() > 0);
    assert_eq!(metrics["working_set"]["refs_created"], 2);
}

#[test]
fn working_set_admission_does_not_depend_on_session_dedup() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("dedup-off.rs");
    let body = (0..300)
        .map(|index| format!("line {index}: dedup-off working-set fixture\n"))
        .collect::<String>();
    fs::write(&file, body).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);
    *engine.working_set.lock().unwrap() = tokenzero_recovery::working_set::WorkingSet::new(1);

    let response = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);
    let visible = response.visible.unwrap().text;
    assert!(
        visible.starts_with("TZ-EVICT/1 ref=tz://blob/"),
        "dedup-off reads must still enter the working set: {visible}"
    );
    let metrics = engine.tool_metrics_snapshot();
    assert_eq!(metrics["working_set"]["evictions"], 1);
    assert_eq!(metrics["working_set"]["refs_created"], 1);
}

#[test]
fn session_expand_fault_is_byte_exact_and_updates_working_set_metrics() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("fault.rs");
    let body = (1..=300)
        .map(|line| format!("fault line {line}: exact recovery bytes\n"))
        .collect::<String>();
    fs::write(&file, &body).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    *engine.working_set.lock().unwrap() = tokenzero_recovery::working_set::WorkingSet::new(1);

    let read = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);
    let marker = read.visible.unwrap().text;
    let ref_id = marker
        .split_whitespace()
        .find_map(|field| field.strip_prefix("ref="))
        .expect("eviction marker must include ref")
        .to_string();
    let expected = RecoveryStore::new(Some(engine.config.cache_path.clone())).expand(
        &ref_id,
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    assert!(expected.found, "{}", expected.reason);
    let expanded = engine.expand(&ref_id, Some("raw"), None, None, None, None);

    assert!(expanded.error.is_none(), "{:?}", expanded.error);
    assert_eq!(
        expanded.visible.unwrap().text.as_bytes(),
        expected.content.as_bytes(),
        "rehydration must not alter explicit-expand bytes"
    );
    let metrics = engine.tool_metrics_snapshot();
    assert_eq!(metrics["working_set"]["lookups"], 1);
    assert_eq!(metrics["working_set"]["faults"], 1);
    assert_eq!(metrics["working_set"]["fault_rate"], 1.0);
    assert_eq!(metrics["working_set"]["rehydrations"], 1);
    assert_eq!(metrics["working_set"]["rehydration_latency"]["samples"], 1);
    assert!(metrics["working_set"]["churn"].as_u64().unwrap() >= 3);
}

#[test]
fn session_plain_store_expand_only_records_a_working_set_lookup() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let mut store = RecoveryStore::new(Some(engine.config.cache_path.clone()));
    let ref_id = store
        .store_blob("plain store payload", ContentType::Unknown)
        .unwrap();
    let expanded = engine.expand(&ref_id, Some("raw"), None, None, None, None);

    assert_eq!(expanded.visible.unwrap().text, "plain store payload");
    let metrics = engine.tool_metrics_snapshot();
    assert_eq!(metrics["working_set"]["lookups"], 1);
    assert_eq!(metrics["working_set"]["faults"], 0);
    assert_eq!(metrics["working_set"]["rehydration_latency"]["samples"], 0);
}
