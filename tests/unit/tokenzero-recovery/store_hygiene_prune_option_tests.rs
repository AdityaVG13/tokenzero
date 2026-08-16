    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn prune_blob_sidecars_refuses_unreadable_snapshot() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        fs::write(&cache, [0xff, 0xfe, 0x00]).unwrap();
        let sidecar_dir = blob_sidecar_dir(&cache);
        fs::create_dir_all(&sidecar_dir).unwrap();
        let sidecar = sidecar_dir.join(format!("{}.txt", "a".repeat(64)));
        fs::write(&sidecar, b"payload-bytes-should-survive").unwrap();

        let err = prune_blob_sidecars(&cache, 0, false)
            .expect_err("unreadable snapshot must refuse prune");
        assert!(sidecar.is_file(), "sidecar must survive refused prune");
        let msg = err.to_string();
        assert!(
            msg.contains("unreadable"),
            "error must name unreadable snapshot, got {msg}"
        );
    }

    #[test]
    fn prune_blob_sidecars_missing_snapshot_is_empty_root_set() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let sidecar_dir = blob_sidecar_dir(&cache);
        fs::create_dir_all(&sidecar_dir).unwrap();
        let sidecar = sidecar_dir.join(format!("{}.txt", "b".repeat(64)));
        fs::write(&sidecar, b"orphan").unwrap();
        let report = prune_blob_sidecars(&cache, 0, false).unwrap();
        assert_eq!(report.removed_files, 1);
        assert!(!sidecar.exists());
    }

    #[test]
    fn prune_recovery_blobs_refuses_unreadable_snapshot() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        fs::write(&cache, [0xff, 0xfe, 0x00]).unwrap();
        let sidecar_dir = blob_sidecar_dir(&cache);
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
    fn persist_pending_refuses_unreadable_snapshot() {
        let dir = tempdir().unwrap();
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
        let dir = tempdir().unwrap();
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
    fn persist_pending_refuses_corrupt_ordinal_generation() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut sidecar = cache.as_os_str().to_os_string();
        sidecar.push(".ordinal-generation");
        fs::write(&sidecar, "not-a-number\n").unwrap();
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let err = store
            .persist_pending()
            .expect_err("corrupt ordinal generation must refuse persist");
        let msg = err.to_string();
        assert!(
            msg.contains("unreadable"),
            "error must name unreadable ordinal sidecar, got {msg}"
        );
        assert!(
            !cache.exists(),
            "persist must not create a snapshot after a corrupt generation sidecar"
        );
    }

    #[test]
    fn persist_pending_missing_snapshot_still_creates() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .persist_pending()
            .expect("missing snapshot must persist");
        assert!(
            cache.is_file(),
            "missing snapshot persist must create the file"
        );
    }

    #[test]
    fn sweep_stale_tmp_removes_expired_under_lock() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        fs::write(&cache, b"{}\n").unwrap();
        let tmp = dir.path().join(".recovery-cache.json.1.0.tmp");
        fs::write(&tmp, b"stale").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
        filetime_set_mtime(&tmp, old);
        let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, false);
        assert_eq!(report.removed, 1);
        assert!(!tmp.exists(), "expired tmp must be unlinked after lock");
    }

    fn filetime_set_mtime(path: &std::path::Path, at: SystemTime) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(at).unwrap();
    }

