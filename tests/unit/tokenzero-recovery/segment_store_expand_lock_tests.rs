    use super::*;
    use tempfile::tempdir;

    #[test]
    fn expand_survives_concurrent_seal_and_evict() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut store = SegmentStore::create_shadow(cache.clone(), None).unwrap();
        store.set_segment_bytes(180);
        store.put("keep", b"live-bytes-xxxx", u64::MAX).unwrap();
        store.put("cold", b"cold-bytes-xxxx", 1).unwrap();
        store.seal().unwrap();

        let reader = cache.clone();
        let expander = std::thread::spawn(move || {
            let mut handle = SegmentStore::open(reader, None).unwrap();
            for _ in 0..32 {
                let got = handle.expand("keep").unwrap();
                assert_eq!(got.as_deref(), Some(b"live-bytes-xxxx".as_slice()));
            }
        });
        let mut evictor = SegmentStore::open(cache.clone(), None).unwrap();
        evictor.evict_expired(10).unwrap();
        expander.join().unwrap();
        let mut check = SegmentStore::open(cache, None).unwrap();
        assert_eq!(
            check.expand("keep").unwrap().as_deref(),
            Some(b"live-bytes-xxxx".as_slice())
        );
    }

    #[test]
    fn put_and_expand_zero_byte_payload() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut store = SegmentStore::create_shadow(cache, None).unwrap();
        store.put("empty", b"", u64::MAX).expect("0-byte payload");
        let got = store.expand("empty").expect("expand empty");
        assert_eq!(got.as_deref(), Some(b"".as_slice()));
        assert_eq!(
            recover_payload_len(0, 8),
            Some(0),
            "zero-length segment records must recover as 0 bytes, not None"
        );
    }

    #[test]
    fn put_refuses_empty_ref_and_unexpanded_tilde() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut store = SegmentStore::create_shadow(cache, None).unwrap();
        let err = store.put("", b"bytes", u64::MAX).expect_err("empty ref");
        assert!(err.to_string().contains("empty segment ref"), "{err}");

        let err = SegmentStore::create_shadow(PathBuf::from("~/recovery-cache.json"), None)
            .expect_err("tilde store");
        assert!(err.to_string().contains("unexpanded ~ store path"), "{err}");
    }

    #[test]
    fn write_index_unlinks_tmp_when_rename_fails() {
        let dir = tempdir().unwrap();
        let mut descriptor = desc(1);
        descriptor.index_file = "recovery.1.segment.index".into();
        fs::create_dir_all(dir.path().join(&descriptor.index_file)).unwrap();
        let err = write_index(dir.path(), &mut descriptor, &SegmentIndex::default())
            .expect_err("rename onto directory");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name();
                let name = name.to_string_lossy();
                name.contains(".tmp").then(|| name.into_owned())
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "failed index write must unlink tmp ({err}): {leftovers:?}"
        );
    }

    #[test]
    fn recover_payload_len_follows_written_bytes_not_default_segment() {
        let larger_than_legacy_cap = DEFAULT_SEGMENT_BYTES
            .checked_mul(4)
            .and_then(|n| n.checked_add(1))
            .expect("default segment * 4 fits u64");
        assert_eq!(
            recover_payload_len(larger_than_legacy_cap, larger_than_legacy_cap),
            usize::try_from(larger_than_legacy_cap).ok(),
            "configured segments larger than DEFAULT*4 must still recover"
        );
        assert_eq!(
            recover_payload_len(larger_than_legacy_cap, larger_than_legacy_cap - 1),
            None
        );
        assert_eq!(
            recover_payload_len(u64::MAX, u64::MAX),
            usize::try_from(u64::MAX).ok()
        );
    }

