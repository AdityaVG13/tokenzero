//! Phase 6 crash windows: persist / prune / WAL / tmp-rename / lock.
//!
//! In-process only. Do not invent subprocess abort injection.
//! CrashBoundary names live in `tokenzero_test_support::CrashBoundary`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokenzero_core::ContentType;
use tokenzero_recovery::{
    RecoveryStore, STALE_TMP_MAX_AGE, prune_blob_sidecars, prune_recovery_blobs,
    sweep_stale_tmp_files,
};

fn wal_path(cache: &Path) -> PathBuf {
    let mut os = cache.as_os_str().to_os_string();
    os.push(".wal");
    PathBuf::from(os)
}

fn sidecar_dir(cache: &Path) -> PathBuf {
    let mut os = cache.as_os_str().to_os_string();
    os.push(".blobs");
    PathBuf::from(os)
}

fn expand_raw(store: &mut RecoveryStore, ref_id: &str) -> tokenzero_recovery::ExpansionResult {
    store.expand(ref_id, Some("raw"), None, None, None, None)
}

fn set_mtime(path: &Path, at: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[test]
fn persist_pending_refuses_unreadable_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let poison = [0xff, 0xfe, 0x00];
    fs::write(&cache, poison).unwrap();
    let mut store = RecoveryStore::new(Some(cache.clone()));
    store
        .persist_pending()
        .expect_err("unreadable snapshot must refuse persist");
    assert_eq!(
        fs::read(&cache).unwrap(),
        poison,
        "persist must not overwrite an unreadable snapshot"
    );
}

#[test]
fn persist_pending_refuses_unparseable_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let poison = b"{not-json";
    fs::write(&cache, poison).unwrap();
    let mut store = RecoveryStore::new(Some(cache.clone()));
    store
        .persist_pending()
        .expect_err("unparseable snapshot must refuse persist");
    assert_eq!(
        fs::read(&cache).unwrap(),
        poison,
        "persist must not overwrite an unparseable snapshot"
    );
}

#[test]
fn expand_unreadable_snapshot_is_not_silent_ok() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    fs::write(&cache, [0xff, 0xfe, 0x00]).unwrap();
    let mut store = RecoveryStore::new(Some(cache));
    let got = expand_raw(&mut store, "tz://blob/deadbeefdeadbeef");
    assert!(!got.found, "unreadable snapshot must not expand as found");
    assert_eq!(
        got.reason, "unreadable-snapshot",
        "unreadable snapshot must fail loud, not a silent miss; got {}",
        got.reason
    );
}

#[test]
fn prune_blob_sidecars_refuses_unreadable_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    fs::write(&cache, [0xff, 0xfe, 0x00]).unwrap();
    let sidecar_dir = sidecar_dir(&cache);
    fs::create_dir_all(&sidecar_dir).unwrap();
    let sidecar = sidecar_dir.join(format!("{}.txt", "a".repeat(64)));
    fs::write(&sidecar, b"payload-bytes-should-survive").unwrap();

    let err =
        prune_blob_sidecars(&cache, 0, false).expect_err("unreadable snapshot must refuse prune");
    assert!(sidecar.is_file(), "sidecar must survive refused prune");
    let msg = err.to_string();
    assert!(
        msg.contains("unreadable"),
        "error must name unreadable snapshot, got {msg}"
    );
}

#[test]
fn prune_recovery_blobs_refuses_unreadable_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    fs::write(&cache, [0xff, 0xfe, 0x00]).unwrap();
    let sidecar_dir = sidecar_dir(&cache);
    fs::create_dir_all(&sidecar_dir).unwrap();
    let sidecar = sidecar_dir.join(format!("{}.txt", "c".repeat(64)));
    fs::write(&sidecar, b"keep-me").unwrap();

    let err = prune_recovery_blobs(&cache, 0, Duration::from_secs(0), false)
        .expect_err("unreadable snapshot must refuse prune");
    assert!(sidecar.is_file(), "sidecar must survive refused prune");
    let msg = err.to_string();
    assert!(
        msg.contains("unreadable"),
        "error must name unreadable snapshot, got {msg}"
    );
}

#[test]
fn second_process_persist_appends_journal_without_snapshot_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let first = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.store_blob("alpha\n", ContentType::Unknown).unwrap()
    };
    let snapshot_before = fs::read(&cache).unwrap();

    let second = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.store_blob("beta\n", ContentType::Unknown).unwrap()
    };
    assert_eq!(
        fs::read(&cache).unwrap(),
        snapshot_before,
        "snapshot must be untouched by a journaled persist"
    );
    assert!(wal_path(&cache).exists(), "session WAL sibling must exist");

    let mut restarted = RecoveryStore::new(Some(cache));
    for (ref_id, text) in [(&first, "alpha\n"), (&second, "beta\n")] {
        let expanded = expand_raw(&mut restarted, ref_id);
        assert!(expanded.found, "lost {ref_id}: {}", expanded.reason);
        assert_eq!(expanded.content, text);
    }
}

#[test]
fn missing_snapshot_replays_wal_persist_does_not_drop_it() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.store_blob("alpha\n", ContentType::Unknown).unwrap();
    }
    let wal_blob = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_blob("from-wal\n", ContentType::Unknown)
            .unwrap()
    };
    assert!(wal_path(&cache).is_file(), "journal append must create WAL");
    fs::remove_file(&cache).unwrap();

    let mut restarted = RecoveryStore::new(Some(cache.clone()));
    let expanded = expand_raw(&mut restarted, &wal_blob);
    assert!(
        expanded.found,
        "missing snapshot must still replay complete WAL; {}",
        expanded.reason
    );
    assert_eq!(expanded.content, "from-wal\n");

    restarted
        .persist_pending()
        .expect("WAL-only recover must be allowed to republish");
    assert!(cache.is_file(), "persist must recreate snapshot from WAL");

    let mut third = RecoveryStore::new(Some(cache));
    let again = expand_raw(&mut third, &wal_blob);
    assert!(again.found, "republish must not drop WAL records");
    assert_eq!(again.content, "from-wal\n");
}

#[test]
fn corrupt_journal_tail_keeps_complete_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.store_blob("alpha\n", ContentType::Unknown).unwrap();
    }
    let good = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.store_blob("good\n", ContentType::Unknown).unwrap()
    };
    let journal = wal_path(&cache);
    let mut bytes = fs::read(&journal).unwrap();
    bytes.extend_from_slice(b"{\"refs\":[\"tz://blob/torn");
    fs::write(&journal, bytes).unwrap();

    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = expand_raw(&mut restarted, &good);
    assert!(
        expanded.found,
        "complete journal entry poisoned by torn tail: {}",
        expanded.reason
    );
    assert_eq!(expanded.content, "good\n");
}

#[test]
fn kill_before_rename_keeps_previous_complete_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let blob = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_blob("complete\n", ContentType::Unknown)
            .unwrap()
    };
    let dest_before = fs::read(&cache).unwrap();
    let leftover = dir.path().join(".recovery-cache.json.tmp-99-1");
    fs::write(&leftover, b"partial-new-bytes-must-not-become-dest").unwrap();

    let mut restarted = RecoveryStore::new(Some(cache.clone()));
    let expanded = expand_raw(&mut restarted, &blob);
    assert!(
        expanded.found,
        "kill-before-rename lost dest: {}",
        expanded.reason
    );
    assert_eq!(expanded.content, "complete\n");
    assert_eq!(
        fs::read(&cache).unwrap(),
        dest_before,
        "leftover tmp must not replace the previous complete snapshot"
    );
}

#[test]
fn sweep_stale_tmp_removes_expired_under_lock() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    fs::write(&cache, b"{}\n").unwrap();
    let tmp = dir.path().join(".recovery-cache.json.1.0.tmp");
    fs::write(&tmp, b"stale").unwrap();
    let old = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
    set_mtime(&tmp, old);
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, false);
    assert_eq!(report.removed, 1);
    assert!(!tmp.exists(), "expired tmp must be unlinked after lock");
}

#[test]
fn sweep_stale_tmp_reclaims_zero_store_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    fs::write(&cache, b"{}\n").unwrap();
    let dest_before = fs::read(&cache).unwrap();
    let hub_tmp = dir.path().join(".recovery-cache.json.tmp-4242-7");
    fs::write(&hub_tmp, b"kill-before-rename leftover").unwrap();
    let old = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
    set_mtime(&hub_tmp, old);
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, false);
    assert_eq!(
        report.removed, 1,
        "hub atomic_write leftover .tmp-pid-seq must be swept"
    );
    assert!(!hub_tmp.exists(), "expired hub tmp must be unlinked");
    assert_eq!(
        fs::read(&cache).unwrap(),
        dest_before,
        "sweep must not touch the previous complete snapshot"
    );
}

#[test]
fn concurrent_persistence_preserves_all_thread_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.store_blob("seed\n", ContentType::Unknown).unwrap();
    }
    let cache_a = cache.clone();
    let cache_b = cache.clone();
    let t1 = std::thread::spawn(move || {
        let mut store = RecoveryStore::new(Some(cache_a));
        store.store_blob("from-a\n", ContentType::Unknown).unwrap()
    });
    let t2 = std::thread::spawn(move || {
        let mut store = RecoveryStore::new(Some(cache_b));
        store.store_blob("from-b\n", ContentType::Unknown).unwrap()
    });
    let a = t1.join().expect("thread a");
    let b = t2.join().expect("thread b");
    let mut restarted = RecoveryStore::new(Some(cache));
    for (ref_id, text) in [(a, "from-a\n"), (b, "from-b\n")] {
        let got = expand_raw(&mut restarted, &ref_id);
        assert!(got.found, "lost {ref_id}: {}", got.reason);
        assert_eq!(got.content, text);
    }
}

/// Must match `BLOB_EXTERNALIZE_MIN_BYTES` in recovery persist.
const BLOB_EXTERNALIZE_MIN_BYTES: usize = 64 * 1024;

fn snapshot_text(cache: &Path) -> String {
    String::from_utf8_lossy(&fs::read(cache).unwrap()).into_owned()
}

#[test]
fn small_blob_persist_keeps_inline_even_after_cas_publish() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let payload = "small-inline-body\n";
    let mut store = RecoveryStore::new(Some(cache.clone()));
    let ref_id = store.store_blob(payload, ContentType::Unknown).unwrap();
    store
        .publish_pending_cas()
        .expect("small blobs still publish to CAS for full-hash expand");
    let snap = snapshot_text(&cache);
    assert!(
        snap.contains("small-inline-body"),
        "bodies below the externalize floor must stay inline in the snapshot"
    );
    assert!(
        !snap.contains("tzx:v1:"),
        "small bodies must not be replaced with a CAS marker"
    );
    let expanded = expand_raw(&mut store, &ref_id);
    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content, payload);
}

#[test]
fn large_blob_persist_replaces_inline_with_cas_marker() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let payload = "L".repeat(BLOB_EXTERNALIZE_MIN_BYTES);
    let mut store = RecoveryStore::new(Some(cache.clone()));
    let ref_id = store
        .store_blob(&payload, ContentType::Unknown)
        .expect("large blob persist");
    let snap = snapshot_text(&cache);
    assert!(
        snap.contains("tzx:v1:"),
        "persist must marker-replace blobs at the externalize floor; snapshot still inline"
    );
    assert!(
        !snap.contains(&payload),
        "snapshot must not still carry the megabyte inline body"
    );
    let expanded = expand_raw(&mut store, &ref_id);
    assert!(
        expanded.found,
        "marker expand lost bytes: {}",
        expanded.reason
    );
    assert_eq!(expanded.content, payload);

    let mut restarted = RecoveryStore::new(Some(cache));
    let again = expand_raw(&mut restarted, &ref_id);
    assert!(
        again.found,
        "restart expand lost marker blob: {}",
        again.reason
    );
    assert_eq!(again.content, payload);
}
