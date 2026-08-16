    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_ref_index_entries_unlinks_tmp_when_rename_fails() {
        let dir = tempdir().unwrap();
        let shard = dir.path().join("abc.ndjson");
        fs::create_dir_all(&shard).unwrap();
        let entry = RefIndexEntry {
            ref_id: "tz://blob/abcdef0123456789".into(),
            store_path: "/tmp/store".into(),
            ts: 1,
            content_class: ContentClass::Unknown,
            expanded: false,
            expansion_count: 0,
            last_expanded_ts: None,
            metadata_migrated: false,
            commit: None,
        };
        write_ref_index_entries(&shard, std::iter::once(&entry))
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
            "failed ref-index rewrite must unlink tmp: {leftovers:?}"
        );
    }

