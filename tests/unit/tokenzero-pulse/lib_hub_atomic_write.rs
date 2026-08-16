    use super::*;

    #[test]
    fn write_sidecar_meta_round_trips_through_hub_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pulse-sync.json");
        let meta = PulseSyncMeta {
            schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
            source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
            ledger_sha256: "abc".to_string(),
            event_count: 2,
            skipped_lines: 0,
            updated_unix: 1,
        };
        write_sidecar_meta(&path, &meta).expect("hub atomic write");
        let got = read_sidecar_meta(&path).expect("read back");
        assert_eq!(got.ledger_sha256, "abc");
        assert_eq!(got.event_count, 2);
        assert!(path.is_file());
    }

