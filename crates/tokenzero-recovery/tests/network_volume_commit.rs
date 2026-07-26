//! tokenzero-794r: a durable commit must not fail the whole plan just because
//! the filesystem cannot fsync.
//!
//! macOS smbfs does not implement the durability primitives POSIX advertises.
//! Verified against a live mount: `sync_all` on a DIRECTORY returns ENOTSUP
//! (os error 45), and on a file opened read-only returns EPERM (os error 13).
//! `persist_pending_durable` fsyncs both, so any plan touching a network-mounted
//! repo could abort AFTER its work had run.

use std::path::{Path, PathBuf};
use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;

/// A mounted SMB volume to test against, or None.
///
/// Deliberately discovered from the live mount table instead of hardcoding a
/// path: the bead was reported on /Volumes/sparkdata, but pinning that would
/// make the test pass vacuously anywhere it is absent.
fn smb_mount() -> Option<PathBuf> {
    let out = std::process::Command::new("/sbin/mount").output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.contains("smbfs") || line.contains("nfs"))
        .filter_map(|line| line.split(" on ").nth(1)?.split(" (").next())
        .map(PathBuf::from)
        .find(|p| p.is_dir() && std::fs::write(p.join(".tz794r_probe"), b"x").is_ok())
}

fn commit_under(dir: &Path) {
    let cache = dir.join("recovery-cache.json");
    let mut store = RecoveryStore::new(Some(cache.clone()));
    store.store_payload_deferred_batch("payload", ContentType::JsonConfig, None, None, None);

    store
        .persist_pending_durable()
        .unwrap_or_else(|e| panic!("commit must survive {}: {e}", dir.display()));

    // The tolerance must not be a silent no-op: the record still has to land.
    assert!(
        cache.exists() || cache.with_extension("json.journal").exists(),
        "commit reported success but wrote nothing under {}",
        dir.display()
    );
}

#[test]
fn commit_succeeds_on_a_network_volume_that_cannot_fsync() {
    let Some(mount) = smb_mount() else {
        eprintln!("skipped: no writable smbfs/nfs mount present");
        return;
    };
    let dir = mount.join(".tz794r").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // Prove the fixture really is a filesystem that rejects the fsync, so a
    // pass here cannot come from testing an ordinary local directory.
    let dir_sync = std::fs::File::open(&dir).and_then(|f| f.sync_all());
    let unsupported = dir_sync
        .as_ref()
        .err()
        .and_then(|e| e.raw_os_error())
        .is_some_and(|c| c == 45 || c == 102 || c == 13);
    if !unsupported {
        eprintln!("skipped: {} supports directory fsync", dir.display());
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    commit_under(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn commit_still_succeeds_on_a_local_volume() {
    // Guards the obvious over-correction: absorbing unsupported-fsync errors
    // must not turn the normal path into a no-op.
    let dir = tempfile::tempdir().unwrap();
    commit_under(dir.path());
}
