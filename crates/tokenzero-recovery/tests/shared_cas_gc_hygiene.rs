use std::fs;
use tokenzero_recovery::shared_cas::{
    GcConfig, GcVerdict, SharedCas, project_id, publish_reachability_snapshot, run_gc,
};

#[test]
fn scans_references_from_all_engine_namespaces() {
    let store = tempfile::tempdir().unwrap();
    let cas = SharedCas::new(store.path().to_path_buf());
    let project = project_id(store.path()).unwrap();
    let mut hashes = Vec::new();
    for engine in ["tokenzero", "fszero", "graphzero"] {
        let hash = cas.publish(engine.as_bytes()).unwrap();
        publish_reachability_snapshot(
            store.path(),
            engine,
            &project,
            1,
            std::slice::from_ref(&hash),
        )
        .unwrap();
        hashes.push(hash);
    }
    let report = run_gc(store.path(), &GcConfig::default()).unwrap();
    for hash in hashes {
        let object = report
            .objects
            .iter()
            .find(|object| object.blob_hash == hash)
            .unwrap();
        assert_eq!(object.verdict, GcVerdict::Retain);
    }
}

#[test]
fn completed_gc_reports_are_a_fixed_ring() {
    let store = tempfile::tempdir().unwrap();
    fs::create_dir_all(store.path().join("gc/roots")).unwrap();
    for index in 0..5 {
        let config = GcConfig {
            run_id: format!("ring-{index}"),
            report_limit: 3,
            ..GcConfig::default()
        };
        run_gc(store.path(), &config).unwrap();
    }
    let mut reports = fs::read_dir(store.path().join("gc/reports"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    reports.sort_by_key(|entry| entry.file_name());
    assert_eq!(reports.len(), 3);
    assert!(store.path().join("gc/reports/ring-4.json").is_file());

    let config = GcConfig {
        run_id: "current-report".into(),
        report_limit: 0,
        ..GcConfig::default()
    };
    run_gc(store.path(), &config).unwrap();
    assert!(
        store
            .path()
            .join("gc/reports/current-report.json")
            .is_file()
    );
}
