use super::*;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use tempfile::tempdir;

struct FsPause {
    entered: Barrier,
    release: Barrier,
}

static FS_PAUSE: Mutex<Option<Arc<FsPause>>> = Mutex::new(None);

pub(super) fn pause_during_fs() {
    let pause = FS_PAUSE.lock().ok().and_then(|mut slot| slot.take());
    if let Some(pause) = pause {
        pause.entered.wait();
        pause.release.wait();
    }
}

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

#[test]
fn coalesced_maintenance_drops_state_before_marker_fs() {
    let directory = tempdir().unwrap();
    let engine = directory.path().join("tokenzero");
    fs::create_dir_all(&engine).unwrap();
    let cache = engine.join("recovery-cache.json");
    let pause = Arc::new(FsPause {
        entered: Barrier::new(2),
        release: Barrier::new(2),
    });
    *FS_PAUSE.lock().unwrap() = Some(Arc::clone(&pause));

    let worker = thread::spawn(move || cache_maintenance_coalesced(&cache, false));
    pause.entered.wait();
    let state_free = auto_maintenance_state().try_lock().is_ok();
    pause.release.wait();
    worker.join().unwrap();
    assert!(
        state_free,
        "auto_maintenance STATE must not stay held across marker_fresh I/O"
    );
}
