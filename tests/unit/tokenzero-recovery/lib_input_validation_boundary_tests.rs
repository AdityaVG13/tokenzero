    use super::*;
    use tokenzero_core::ContentType;

    #[test]
    fn persist_refuses_unexpanded_tilde_store_path() {
        let mut store = RecoveryStore::new(Some(PathBuf::from("~/recovery-cache.json")));
        store.store_blob_deferred("payload", ContentType::Unknown);
        let err = store.persist_pending().expect_err("tilde store path");
        let msg = err.to_string();
        assert!(
            msg.contains("unexpanded ~ store path"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn store_alias_refuses_empty_refs() {
        let mut store = RecoveryStore::new(None);
        let err = store
            .store_alias(
                "",
                "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .expect_err("empty alias");
        assert!(err.to_string().contains("non-empty"), "{err}");
        let err = store
            .store_alias("tz://s/abcdabcdabcdabcd", "")
            .expect_err("empty target");
        assert!(err.to_string().contains("non-empty"), "{err}");
    }

    #[test]
    fn recovery_config_zero_load_cap_fails_loud() {
        let err = RecoveryConfig {
            max_load_bytes: 0,
            ..RecoveryConfig::default()
        }
        .validate()
        .expect_err("zero max_load_bytes");
        assert!(
            err.to_string().contains("max_load_bytes must be nonzero"),
            "{err}"
        );
    }

    #[test]
    fn blob_ref_proven_on_disk_rejects_non_hex_identity() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let fake = "g".repeat(64);
        fs::create_dir_all(blob_sidecar_dir(&cache)).unwrap();
        fs::write(
            blob_sidecar_dir(&cache).join(format!("{fake}.txt")),
            "not a blob",
        )
        .unwrap();
        assert!(
            !blob_ref_proven_on_disk(&cache, &format!("tz://blob/{fake}")),
            "non-hex 64-char sidecar must not prove presence"
        );
    }

    #[test]
    fn blob_marker_hash_is_typed_lowercase_digest() {
        let lower = format!("{BLOB_MARKER_PREFIX}{}:4:", "a".repeat(64));
        assert_eq!(parse_blob_marker(&lower), Some((&*"a".repeat(64), 4)));
        let upper = format!("{BLOB_MARKER_PREFIX}{}:4:", "A".repeat(64));
        assert!(
            parse_blob_marker(&upper).is_none(),
            "uppercase hex is not a digest identity"
        );
        let mixed = format!(
            "{BLOB_MARKER_PREFIX}{}:4:",
            format!("{}{}", "a".repeat(63), "F")
        );
        assert!(parse_blob_marker(&mixed).is_none());
    }

    #[test]
    fn malformed_blob_marker_prefix_is_not_inline_content() {
        let mut store = RecoveryStore::new(None);
        let blob_id = "ab".repeat(32);
        let marker = format!("{BLOB_MARKER_PREFIX}{}:1:", "A".repeat(64));
        store
            .state
            .blobs
            .insert(format!("tz://blob/{blob_id}"), BlobEntry::Inline(marker));
        let expanded = store.expand(
            &format!("tz://blob/{blob_id}"),
            Some("raw"),
            None,
            None,
            None,
            None,
        );
        assert!(
            !expanded.found,
            "typed marker prefix with invalid digest must not surface as Found content"
        );
        assert!(
            expanded.reason.contains("decode-failed"),
            "expected decode-failed, got {:?}",
            expanded.reason
        );
    }

    #[test]
    fn unexpanded_tilde_path_matches_hub_literal_root_rule() {
        assert!(unexpanded_tilde_path(Path::new(
            "~/tokenzero/recovery-cache.json"
        )));
        assert!(unexpanded_tilde_path(Path::new("~")));
        assert!(!unexpanded_tilde_path(Path::new("/var/~/store")));
        assert!(!unexpanded_tilde_path(Path::new(
            "/tmp/recovery-cache.json"
        )));
    }

    #[test]
    fn ref_index_shard_paths_stay_under_root() {
        let root = Path::new("/var/tokenzero/ref-index");
        for id in [
            "../",
            "..",
            "/",
            "//",
            "/etc/passwd",
            r"..\",
            r"..\..\windows",
            "tz://blob/../",
            "tz://blob/..\\evil",
        ] {
            let path = ref_index_shard_path(root, id);
            let rel = path.strip_prefix(root).unwrap_or_else(|_| {
                panic!(
                    "ref-index shard escaped root for {id:?}: {}",
                    path.display()
                )
            });
            assert!(
                rel.components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
                "unsafe relative shard for {id:?}: {}",
                rel.display()
            );
            let name = rel.file_name().and_then(|n| n.to_str()).unwrap();
            assert!(name.ends_with(".ndjson"), "{name}");
            assert!(
                name.trim_end_matches(".ndjson")
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric()),
                "shard file {name} is not a safe prefix"
            );
        }
        let hex = ref_index_shard_path(root, "tz://blob/abcdef0123456789");
        assert_eq!(hex.file_name().and_then(|n| n.to_str()), Some("abc.ndjson"));
    }

