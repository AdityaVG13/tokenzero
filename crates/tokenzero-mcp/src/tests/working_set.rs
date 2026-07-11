use super::*;

#[test]
fn working_set_eviction_is_visible_in_session_metrics() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("large.rs");
    let body = (0..300)
        .map(|index| {
            format!(
                "line {index}: eviction telemetry fixture
"
            )
        })
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

    let metrics = engine.tool_metrics_snapshot();
    assert_eq!(metrics["working_set"]["evictions"], 1);
    assert!(metrics["working_set"]["bytes_evicted"].as_u64().unwrap() > 0);
    assert_eq!(metrics["working_set"]["refs_created"], 1);
}
