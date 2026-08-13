use super::*;
use tempfile::tempdir;

#[test]
fn gc_marker_throttles_second_run() {
    let directory = tempdir().unwrap();
    let engine = directory.path().join("tokenzero");
    fs::create_dir_all(&engine).unwrap();
    let cache = engine.join("recovery-cache.json");
    let marker = engine.join("gc.last");
    atomic_touch(&marker).unwrap();

    assert_eq!(gc_maintenance(&cache, false)["skipped"], "recent");
}

#[test]
fn journal_pruning_keeps_newest_count_and_recent_files() {
    let directory = tempdir().unwrap();
    let engine = directory.path().join("tokenzero");
    let journals = engine.join("plan-journals");
    fs::create_dir_all(&journals).unwrap();
    for index in 0..4 {
        fs::write(journals.join(format!("{index}.json")), b"{}").unwrap();
    }
    let report = prune_plan_journals_at(
        &engine.join("recovery-cache.json"),
        false,
        SystemTime::now() + Duration::from_secs(1),
        Duration::ZERO,
        2,
    );
    assert_eq!(report["removed"], 2);
    assert_eq!(fs::read_dir(journals).unwrap().count(), 3);
}
