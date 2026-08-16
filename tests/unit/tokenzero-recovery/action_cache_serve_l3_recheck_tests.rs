    use super::*;
    use tempfile::tempdir;

    fn key(n: u8) -> String {
        format!("{n:064x}")
    }

    fn live_entry(n: u8) -> ActionCacheEntry {
        ActionCacheEntry {
            key: key(n),
            artifact_ref: format!("tz://blob/{}", key(n)),
            fszero_bookmark: None,
            dep_closure_ref: None,
            class: "must_block_revalidate".into(),
            verified: true,
            world_id: Some("w1".into()),
            tombstone: false,
            tombstoned_at_unix: None,
            l3_cold: false,
            cold_since_unix: None,
        }
    }

    #[test]
    fn serve_refuses_after_l3_loss() {
        let dir = tempdir().unwrap();
        let index = ActionCacheIndex::open(dir.path());
        let item = live_entry(7);
        index.put(item.clone()).unwrap();
        assert!(index.mark_l3_loss(&item.key, 1_000).unwrap());
        assert!(
            index.serve(&item.key).unwrap().is_none(),
            "serve must not return an L3-cold artifact_ref"
        );
    }

    #[test]
    fn put_refuses_empty_artifact_ref() {
        let dir = tempdir().unwrap();
        let index = ActionCacheIndex::open(dir.path());
        let mut item = live_entry(3);
        item.artifact_ref.clear();
        let err = index.put(item).expect_err("empty artifact_ref");
        assert!(
            matches!(err, ActionCacheError::Io(ref io) if io.kind() == io::ErrorKind::InvalidInput),
            "{err}"
        );
    }

    #[test]
    fn put_refuses_unexpanded_tilde_store_root() {
        let index = ActionCacheIndex::open(Path::new("~"));
        let err = index.put(live_entry(4)).expect_err("tilde store root");
        let msg = err.to_string();
        assert!(msg.contains("unexpanded ~ store path"), "{msg}");
    }

    #[test]
    fn serve_unlinks_pin_when_segment_is_unreadable() {
        let dir = tempdir().unwrap();
        let index = ActionCacheIndex::open(dir.path());
        let item = live_entry(9);
        index.put(item.clone()).unwrap();
        let segment = index
            .root()
            .join(&item.key[..2])
            .join(format!("{}.json", item.key));
        fs::write(&segment, b"not-json").unwrap();
        let err = index.serve(&item.key).expect_err("corrupt segment");
        assert!(
            !index.has_in_flight_serve(&item.key),
            "failed serve must not leave a pin that blocks GC: {err}"
        );
    }

