use std::fs;
use tokenzero_recovery::shared_cas::{GcConfig, run_gc};

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
}
