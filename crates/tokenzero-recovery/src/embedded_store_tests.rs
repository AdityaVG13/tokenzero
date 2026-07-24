use super::*;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

#[test]
fn lifecycle_open_try_open_in_memory() {
    let mem = TokenZeroStore::in_memory();
    assert!(mem.root.is_none());
    assert!(mem.shared_cas().is_some());
    assert!(!mem.durable_degraded);

    let dir = tempdir().unwrap();
    let root = dir.path();
    let opened = TokenZeroStore::open(root);
    assert_eq!(opened.root, Some(root.to_path_buf()));
    assert!(!opened.durable_degraded);

    // try_open on the same root succeeds and sets up the cache directory.
    let tried = TokenZeroStore::try_open(root).unwrap();
    assert!(tried.recovery.persistence_path.is_some());
}

#[test]
fn put_expand_round_trip_byte_exact_via_shared_cas() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // Create the unified .zerostack layout so the shared CAS is attached.
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
    let mut store = TokenZeroStore::open(root);
    assert!(
        store.shared_cas().is_some(),
        "shared CAS should be attached in unified layout"
    );

    let bytes = b"hello ZeroRef v1 shared CAS";
    let ref_id = store.put(bytes, None).unwrap();
    assert!(ref_id.starts_with("tz://blob/"));
    assert_eq!(ref_id.len(), "tz://blob/".len() + 64);

    let resolved = store.expand(&ref_id).unwrap();
    assert_eq!(resolved, bytes);

    // Cross-engine alias schemes resolve the same bytes.
    let fz_ref = ref_id.replacen("tz://blob/", "fz://blob/", 1);
    let gz_ref = ref_id.replacen("tz://blob/", "gz://blob/", 1);
    assert_eq!(store.expand(&fz_ref).unwrap(), bytes);
    assert_eq!(store.expand(&gz_ref).unwrap(), bytes);
}

#[test]
fn isolated_roots_do_not_share_cas() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    std::fs::create_dir_all(a.path().join(".zerostack").join("tokenzero")).unwrap();
    std::fs::create_dir_all(b.path().join(".zerostack").join("tokenzero")).unwrap();

    let mut store_a = TokenZeroStore::open(a.path());
    let mut store_b = TokenZeroStore::open(b.path());
    let bytes = b"isolated payload";
    let ref_a = store_a.put(bytes, None).unwrap();

    // Store B should not resolve the ref because it points at a different CAS.
    assert!(matches!(
        store_b.expand(&ref_a),
        Err(TokenZeroStoreError::NotFound)
    ));
}

#[test]
fn shared_root_shares_cas_between_handles() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();

    let mut first = TokenZeroStore::open(root);
    let mut second = TokenZeroStore::open(root);
    let bytes = b"shared root payload";
    let ref_id = first.put(bytes, None).unwrap();

    assert_eq!(second.expand(&ref_id).unwrap(), bytes);
}

#[test]
fn explicit_shared_cas_is_shared_across_handles() {
    let cas_dir = tempdir().unwrap();
    let cas = SharedCas::new(cas_dir.path().to_path_buf());

    let mut first = TokenZeroStore::with_shared_cas(None, cas.clone());
    let mut second = TokenZeroStore::with_shared_cas(None, cas);
    let bytes = b"explicit shared CAS payload";
    let ref_id = first.put(bytes, None).unwrap();

    assert_eq!(second.expand(&ref_id).unwrap(), bytes);
}

#[test]
fn capability_descriptor_is_valid_and_matches_state() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
    let store = TokenZeroStore::open(root);

    let cap = store.capability_descriptor();
    assert_eq!(cap["schema_version"], DESCRIPTOR_SCHEMA_VERSION);
    assert_eq!(cap["descriptor_version"], DESCRIPTOR_VERSION);
    assert_eq!(cap["engine"], "tokenzero");
    assert_eq!(cap["zeroref_v1"]["version"], "v1");
    assert!(cap["zeroref_v1"]["enabled"].as_bool().unwrap());
    assert!(!cap["zeroref_v1"]["shared_cas"].as_bool().unwrap());
    assert!(!cap["zeroref_v1"]["shared_cas_writable"].as_bool().unwrap());
    assert!(cap["zeroref_v1"]["blob_ref_expand"].as_bool().unwrap());
    let schemes = cap["zeroref_v1"]["ref_schemes"].as_array().unwrap().clone();
    assert!(schemes.contains(&Value::String("tz://".to_string())));
    assert!(schemes.contains(&Value::String("fz://".to_string())));
    assert!(schemes.contains(&Value::String("gz://".to_string())));
}

#[test]
fn publish_capabilities_round_trips() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
    let mut store = TokenZeroStore::open(root);

    let descriptor = store.capability_descriptor();
    store.publish_capabilities();

    let digest = Sha256::digest(descriptor.to_string());
    let expected_hash = digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    let blob_ref = format!("tz://blob/{expected_hash}");
    let expanded = store.expand(&blob_ref).unwrap();
    let round_tripped: Value = serde_json::from_slice(&expanded).unwrap();
    assert_eq!(round_tripped["schema_version"], DESCRIPTOR_SCHEMA_VERSION);
    assert_eq!(round_tripped["engine"], "tokenzero");
}

#[test]
fn explicit_capability_publication_round_trips_through_shared_cas() {
    let mut store = TokenZeroStore::in_memory();
    let descriptor = store.capability_descriptor().to_string();
    store.publish_capabilities();
    let hash = Sha256::digest(descriptor.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expanded = store.expand(&format!("tz://blob/{hash}")).unwrap();
    assert_eq!(expanded, descriptor.as_bytes());
}

#[test]
fn max_object_bytes_limit_enforced() {
    let mut store = TokenZeroStore::in_memory();
    let bytes = b"too big";
    let err = store.put(bytes, Some(2)).unwrap_err();
    assert!(matches!(
        err,
        TokenZeroStoreError::PayloadTooLarge { size: 7, limit: 2 }
    ));
}

#[test]
fn root_report_reflects_memory_and_unified_modes() {
    let mem = TokenZeroStore::in_memory();
    let mem_report = mem.root_report();
    assert_eq!(mem_report["effective_root_mode"], "memory");
    assert_eq!(mem_report["store_db"], "memory");

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
    let unified = TokenZeroStore::open(root);
    let unified_report = unified.root_report();
    assert_eq!(unified_report["effective_root_mode"], "unified");
    assert!(
        !unified_report["store_health"]["cas_attached"]
            .as_bool()
            .unwrap()
    );
}

// --- PR22 review regressions ---

#[test]
fn expand_applies_byte_and_line_fragments_after_whole_object_verify() {
    let mut store = TokenZeroStore::in_memory();
    let payload = b"alpha\nbeta\ngamma\n";
    let ref_id = store.put(payload, None).unwrap();

    // #B0-5 → "alpha" (byte-exact)
    let b_ref = format!("{ref_id}#B0-5");
    assert_eq!(store.expand(&b_ref).unwrap(), b"alpha");

    // #L2-2 → "beta\n" with exact newline retention
    let l_ref = format!("{ref_id}#L2-2");
    assert_eq!(store.expand(&l_ref).unwrap(), b"beta\n");

    // Cross-engine schemes honor fragments too.
    let fz_b = format!(
        "fz://blob/{}#B6-11",
        ref_id.strip_prefix("tz://blob/").unwrap()
    );
    assert_eq!(store.expand(&fz_b).unwrap(), b"beta\n");

    let gz_l = format!(
        "gz://blob/{}#L1-L1",
        ref_id.strip_prefix("tz://blob/").unwrap()
    );
    assert_eq!(store.expand(&gz_l).unwrap(), b"alpha\n");
}

#[test]
fn expand_accepts_legacy_plus_byte_alias() {
    let mut store = TokenZeroStore::in_memory();
    let ref_id = store.put(b"abcdef", None).unwrap();
    let alias = format!("{ref_id}#B1+3");
    assert_eq!(store.expand(&alias).unwrap(), b"bcd");
}

#[test]
fn expand_fragment_out_of_range_is_typed_error() {
    let mut store = TokenZeroStore::in_memory();
    let ref_id = store.put(b"short", None).unwrap();
    let b_ref = format!("{ref_id}#B0-100");
    let err = store.expand(&b_ref).unwrap_err();
    match err {
        TokenZeroStoreError::Fragment(reason) => {
            assert!(
                reason.starts_with("fragment-out-of-range"),
                "reason={reason}"
            );
        }
        other => panic!("expected Fragment, got {other:?}"),
    }
}

#[test]
fn expand_full_hash_missing_is_not_found_not_none_fallback() {
    let mut store = TokenZeroStore::in_memory();
    let missing = "tz://blob/0000000000000000000000000000000000000000000000000000000000000000";
    assert!(matches!(
        store.expand(missing),
        Err(TokenZeroStoreError::NotFound)
    ));

    // Cross-engine full-hash missing also stays typed NotFound — no legacy
    // fallback that would return Ok/None-style silence.
    let missing_fz = "fz://blob/0000000000000000000000000000000000000000000000000000000000000000";
    assert!(matches!(
        store.expand(missing_fz),
        Err(TokenZeroStoreError::NotFound)
    ));
}

#[test]
fn expand_full_hash_corruption_never_falls_back_to_legacy() {
    let cas_dir = tempdir().unwrap();
    let cas = SharedCas::new(cas_dir.path().to_path_buf());
    let mut store = TokenZeroStore::with_shared_cas(None, cas.clone());

    let payload = b"honest-bytes";
    let ref_id = store.put(payload, None).unwrap();
    let hash = ref_id.strip_prefix("tz://blob/").unwrap();
    let object_path = cas
        .root()
        .join("blobs")
        .join("sha256")
        .join(&hash[..2])
        .join(hash);
    std::fs::write(&object_path, b"tampered-content").unwrap();

    // Full ref and fragment form both report Corruption, never legacy bytes.
    assert!(matches!(
        store.expand(&ref_id),
        Err(TokenZeroStoreError::Corruption)
    ));
    let frag = format!("{ref_id}#B0-5");
    assert!(matches!(
        store.expand(&frag),
        Err(TokenZeroStoreError::Corruption)
    ));

    // Even if the recovery store has a same-hash alias payload, full-hash
    // portable refs must not fall back.
    let _ = store
        .recovery_mut()
        .store_blob("legacy-poison", ContentType::Unknown);
    assert!(matches!(
        store.expand(&ref_id),
        Err(TokenZeroStoreError::Corruption)
    ));
}

#[test]
fn classify_root_mode_uses_path_components_not_substring() {
    // Exact .zerostack component → unified.
    let unified_cache = PathBuf::from("/tmp/project/.zerostack/tokenzero/recovery-cache.json");
    assert_eq!(classify_root_mode(&unified_cache), "unified");

    // Lookalike .zerostack-old must NOT classify as unified.
    let old_cache = PathBuf::from("/tmp/project/.zerostack-old/tokenzero/recovery-cache.json");
    assert_eq!(classify_root_mode(&old_cache), "legacy");

    // Windows-style separators still classify via components.
    let win = PathBuf::from(r"C:\Users\x\proj\.zerostack\tokenzero\recovery-cache.json");
    // On Unix this is a single component path string; structural check still
    // walks components, so a path whose file_name chain ends with
    // tokenzero under .zerostack is unified when components parse that way.
    // Build with join to guarantee component structure:
    let win_joined = PathBuf::from("C:")
        .join("Users")
        .join("x")
        .join("proj")
        .join(".zerostack")
        .join("tokenzero")
        .join("recovery-cache.json");
    assert_eq!(classify_root_mode(&win_joined), "unified");
    let _ = win; // silence unused in documentation of the bug class

    // Flat legacy cache.
    let legacy = PathBuf::from("/tmp/project/.tokenzero/recovery-cache.json");
    assert_eq!(classify_root_mode(&legacy), "legacy");
}

#[test]
fn root_report_classifies_zerostack_old_as_legacy() {
    let dir = tempdir().unwrap();
    // Create a lookalike root that substring matching would misclassify.
    let lookalike = dir.path().join(".zerostack-old").join("tokenzero");
    std::fs::create_dir_all(&lookalike).unwrap();
    let cache = lookalike.join("recovery-cache.json");
    // Attach via with_shared_cas so we control the cache path through open
    // of a synthetic recovery store: open() would choose legacy/unified by
    // .zerostack existence. Instead assert classify_root_mode directly and
    // via a handle whose persistence path is the lookalike.
    let cas = SharedCas::new(dir.path().join(".zerostack-old"));
    let mut store = TokenZeroStore::with_shared_cas(None, cas);
    // Manually point recovery at the lookalike cache path.
    store.recovery = RecoveryStore::new(Some(cache));
    assert_eq!(store.effective_root_mode(), "legacy");
    assert_eq!(store.root_report()["effective_root_mode"], "legacy");
}

#[test]
fn ambient_project_cas_is_not_advertised_as_shared() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".zerostack").join("tokenzero")).unwrap();
    let store = TokenZeroStore::open(dir.path());
    assert!(
        store.shared_cas().is_some(),
        "ambient CAS remains internally usable"
    );
    let cap = store.capability_descriptor();
    assert_eq!(cap["zeroref_v1"]["shared_cas"], false);
    assert_eq!(cap["zeroref_v1"]["shared_cas_writable"], false);

    let explicit = TokenZeroStore::in_memory();
    let cap = explicit.capability_descriptor();
    assert_eq!(cap["zeroref_v1"]["shared_cas"], true);
    assert_eq!(cap["zeroref_v1"]["shared_cas_writable"], true);
}

#[test]
fn cas_probe_uses_canonical_subtree_and_leaves_no_artifacts() {
    let dir = tempdir().unwrap();
    let cas_root = dir.path().join("cas");
    let store = TokenZeroStore::with_shared_cas(None, SharedCas::new(cas_root.clone()));
    assert!(store.cas_writable());
    assert!(
        !cas_root.join("blobs").exists(),
        "probe-created canonical subtree must be removed"
    );
}

#[test]
fn publish_preserves_containment_conflict_and_corruption_categories() {
    let dir = tempdir().unwrap();

    let contained_root = dir.path().join("contained");
    std::fs::create_dir_all(&contained_root).unwrap();
    std::fs::write(contained_root.join("blobs"), b"not-a-directory").unwrap();
    let mut contained = TokenZeroStore::with_shared_cas(None, SharedCas::new(contained_root));
    assert!(matches!(
        contained.put(b"payload", None),
        Err(TokenZeroStoreError::PublishContainment)
    ));

    let conflict_root = dir.path().join("conflict");
    let conflict_bytes = b"conflict";
    let hash = Sha256::digest(conflict_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let target = conflict_root
        .join("blobs")
        .join("sha256")
        .join(&hash[..2])
        .join(&hash);
    std::fs::create_dir_all(&target).unwrap();
    let mut conflict = TokenZeroStore::with_shared_cas(None, SharedCas::new(conflict_root));
    assert!(matches!(
        conflict.put(conflict_bytes, None),
        Err(TokenZeroStoreError::PublishConflict)
    ));

    let corrupt_root = dir.path().join("corrupt");
    let target = corrupt_root
        .join("blobs")
        .join("sha256")
        .join(&hash[..2])
        .join(&hash);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"wrong").unwrap();
    let mut corrupt = TokenZeroStore::with_shared_cas(None, SharedCas::new(corrupt_root));
    assert!(matches!(
        corrupt.put(conflict_bytes, None),
        Err(TokenZeroStoreError::Corruption)
    ));
}

#[test]
fn durable_is_false_when_cache_target_becomes_unusable() {
    let dir = tempdir().unwrap();
    let mut store = TokenZeroStore::try_open(dir.path()).unwrap();
    assert_eq!(store.capability_descriptor()["recovery"]["durable"], true);
    let cache = store.recovery().persistence_path.clone().unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    assert_eq!(store.capability_descriptor()["recovery"]["durable"], false);
    assert_eq!(store.root_report()["store_health"]["durable"], false);
    store.recovery = RecoveryStore::new(None);
}

fn assert_no_probe_artifacts(path: &Path) {
    if !path.exists() {
        return;
    }
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(!name.contains("write-probe"), "left probe artifact: {name}");
        if entry.file_type().unwrap().is_dir() {
            assert_no_probe_artifacts(&entry.path());
        }
    }
}

#[test]
fn concurrent_fresh_root_put_and_probe_accept_shared_ancestor_creation() {
    let dir = tempdir().unwrap();
    let cas = SharedCas::new(dir.path().join("fresh-cas"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let handles = (0..8)
        .map(|index| {
            let cas = cas.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                if index % 2 == 0 {
                    let mut store = TokenZeroStore::with_shared_cas(None, cas);
                    let payload = format!("concurrent-payload-{index}");
                    store.put(payload.as_bytes(), None).unwrap();
                } else {
                    assert!(probe_cas_writable(&cas));
                }
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_no_probe_artifacts(cas.root());
}

#[test]
fn cas_probe_preserves_existing_blob_and_is_concurrency_safe() {
    let dir = tempdir().unwrap();
    let cas = SharedCas::new(dir.path().join("cas"));
    let payload = b"existing canonical object";
    let hash = cas.publish(payload).unwrap();
    let object = cas
        .root()
        .join("blobs")
        .join("sha256")
        .join(&hash[..2])
        .join(&hash);

    let handles = (0..8)
        .map(|_| {
            let cas = cas.clone();
            std::thread::spawn(move || assert!(probe_cas_writable(&cas)))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(std::fs::read(&object).unwrap(), payload);
    assert_no_probe_artifacts(cas.root());
}

#[test]
fn durable_probe_preserves_live_cache_and_removes_owned_sibling() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let cache = root.join(".tokenzero").join("recovery-cache.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let original = b"{\"version\":1}";
    std::fs::write(&cache, original).unwrap();

    probe_durable_cache_target(root, &cache).unwrap();

    assert_eq!(std::fs::read(&cache).unwrap(), original);
    assert_no_probe_artifacts(cache.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn symlinked_cache_and_cas_ancestors_are_rejected() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let outside_cache = dir.path().join("outside-cache");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&outside_cache).unwrap();
    symlink(&outside_cache, workspace.join(".tokenzero")).unwrap();
    assert!(matches!(
        TokenZeroStore::try_open(&workspace),
        Err(TokenZeroStoreError::CacheDir(_))
    ));

    let cas_root = dir.path().join("cas");
    let outside_blobs = dir.path().join("outside-blobs");
    std::fs::create_dir_all(&cas_root).unwrap();
    std::fs::create_dir_all(&outside_blobs).unwrap();
    symlink(&outside_blobs, cas_root.join("blobs")).unwrap();
    let cas = SharedCas::new(cas_root);
    assert!(!probe_cas_writable(&cas));
    let mut store = TokenZeroStore::with_shared_cas(None, cas);
    assert!(matches!(
        store.put(b"must stay contained", None),
        Err(TokenZeroStoreError::PublishContainment)
    ));
}

#[cfg(unix)]
#[test]
fn cas_writable_false_when_root_not_writable() {
    let dir = tempdir().unwrap();
    let cas_root = dir.path().join("ro-cas");
    std::fs::create_dir_all(&cas_root).unwrap();
    // Make the CAS root read-only so create_dir_all(blobs/...) fails.
    let mut perms = std::fs::metadata(&cas_root).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(&cas_root, perms).unwrap();

    let cas = SharedCas::new(cas_root.clone());
    let mut store = TokenZeroStore::with_shared_cas(None, cas);
    assert!(store.shared_cas().is_some());
    assert!(
        !store.cas_writable(),
        "read-only CAS root must not advertise writability"
    );
    assert!(matches!(
        store.put(b"permission-denied", None),
        Err(TokenZeroStoreError::PublishPermission)
    ));
    let cap = store.capability_descriptor();
    assert_eq!(cap["zeroref_v1"]["shared_cas"], true);
    assert_eq!(cap["zeroref_v1"]["shared_cas_writable"], false);
    let report = store.root_report();
    assert_eq!(report["store_health"]["cas_attached"], true);
    assert_eq!(report["store_health"]["cas_writable"], false);

    // Restore writability so tempdir cleanup succeeds.
    let mut perms = std::fs::metadata(&cas_root).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&cas_root, perms).unwrap();
}

#[test]
fn with_shared_cas_mkdir_failure_sets_durable_degraded() {
    let dir = tempdir().unwrap();
    // Parent that cannot contain a new directory: use a file as "root".
    let file_root = dir.path().join("not-a-dir");
    std::fs::write(&file_root, b"x").unwrap();
    let cas = SharedCas::new(dir.path().join("cas"));
    let store = TokenZeroStore::with_shared_cas(Some(file_root), cas);
    assert!(
        store.durable_degraded,
        "mkdir failure must set durable_degraded"
    );
    assert!(
        store.recovery().persistence_path.is_none(),
        "must not claim a durable path after mkdir failure"
    );
    let report = store.root_report();
    assert_eq!(report["durable_degraded"], true);
    assert_eq!(report["store_health"]["durable"], false);
}

#[test]
fn root_report_redacts_absolute_paths() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
    let store = TokenZeroStore::open(root);
    let report = store.root_report();

    let workspace = report["workspace_root"].as_str().unwrap();
    let store_root = report["store_root"].as_str().unwrap();
    let store_db = report["store_db"].as_str().unwrap();
    let abs = root.to_string_lossy();

    assert!(
        !workspace.contains(abs.as_ref()),
        "workspace_root leaked absolute path: {workspace}"
    );
    assert!(
        !store_root.contains(abs.as_ref()),
        "store_root leaked absolute path: {store_root}"
    );
    assert!(
        !store_db.contains(abs.as_ref()),
        "store_db leaked absolute path: {store_db}"
    );
    assert!(
        workspace.starts_with("path:"),
        "workspace_root should be path: identity, got {workspace}"
    );
    assert!(
        store_root.starts_with("path:"),
        "store_root should be path: identity, got {store_root}"
    );
    assert!(
        store_db.starts_with("path:"),
        "store_db should be path: identity, got {store_db}"
    );

    // Nested capability descriptor must also avoid absolute paths.
    let cap = &report["capabilities"];
    if let Some(p) = cap["recovery"]["persistent_path"].as_str() {
        assert!(!p.contains(abs.as_ref()), "cap persistent_path leaked: {p}");
        assert!(p.starts_with("path:"), "cap persistent_path={p}");
    }
    if let Some(p) = cap["recovery"]["store_root"].as_str() {
        assert!(!p.contains(abs.as_ref()), "cap store_root leaked: {p}");
        assert!(p.starts_with("path:"), "cap store_root={p}");
    }

    // Redacted identity must not reverse to the original path string.
    assert_ne!(workspace, abs.as_ref());
    assert!(!workspace.contains('/'));
}

#[test]
fn expand_malformed_fragment_is_typed() {
    let mut store = TokenZeroStore::in_memory();
    let ref_id = store.put(b"abc", None).unwrap();
    let bad = format!("{ref_id}#Babc");
    match store.expand(&bad).unwrap_err() {
        TokenZeroStoreError::Fragment(reason) => assert_eq!(reason, "fragment-malformed"),
        other => panic!("expected Fragment, got {other:?}"),
    }
    let dup = format!("{ref_id}#B0-1#L1");
    match store.expand(&dup).unwrap_err() {
        TokenZeroStoreError::Fragment(reason) => assert_eq!(reason, "fragment-duplicate"),
        other => panic!("expected Fragment, got {other:?}"),
    }
}
