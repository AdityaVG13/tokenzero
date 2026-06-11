use super::*;

#[test]
fn missing_cache_is_an_empty_store() {
    let outcome = recall_search(Path::new("/nonexistent/recall-cache.json"), "x", 10);
    assert!(!outcome.unreadable);
    assert!(outcome.hits.is_empty());
    assert_eq!(outcome.payloads_searched, 0);
}

#[test]
fn unparseable_cache_reports_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    std::fs::write(&path, "{not json").unwrap();
    let outcome = recall_search(&path, "x", 10);
    assert!(outcome.unreadable);
    assert!(outcome.hits.is_empty());
}

#[test]
fn identical_file_and_blob_content_reports_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "files": {"f1": {"ref_id": "tz://file/f1", "path": "a.rs", "text": "needle here"}},
            "blobs": {"b1": {"ref_id": "tz://blob/b1", "text": "needle here"}}
        })
        .to_string(),
    )
    .unwrap();
    let outcome = recall_search(&path, "needle", 10);
    assert_eq!(outcome.hits.len(), 1);
    assert_eq!(outcome.hits[0].ref_id, "tz://file/f1");
    assert_eq!(outcome.hits[0].label, "a.rs");
}

#[test]
fn session_pack_lists_recent_payloads_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "order": ["tz://file/f1", "tz://file/f2"],
            "files": {
                "f1": {"ref_id": "tz://file/f1", "path": "a.rs", "text": "alpha\nbeta"},
                "f2": {"ref_id": "tz://file/f2", "path": "b.rs", "text": "gamma"}
            },
            "blobs": {}
        })
        .to_string(),
    )
    .unwrap();

    let pack = build_session_pack(&path, 400).unwrap();
    // Most recent first (order array is append-ordered).
    let b_pos = pack.find("b.rs").unwrap();
    let a_pos = pack.find("a.rs").unwrap();
    assert!(b_pos < a_pos, "{pack}");
    assert!(pack.contains("tz://file/f2"), "{pack}");
    assert!(pack.contains("expand"), "{pack}");

    // A budget with room for exactly one entry lists the most recent
    // payload and summarizes the rest.
    let header_end = pack.find("\n- ").unwrap();
    let tight_budget = tokenzero_core::count_tokens(&pack[..header_end]) + 20;
    let tight = build_session_pack(&path, tight_budget).unwrap();
    assert!(tight.contains("b.rs"), "{tight}");
    assert!(!tight.contains("a.rs"), "{tight}");
    assert!(tight.contains("more stored payloads"), "{tight}");

    assert!(build_session_pack(Path::new("/nope/cache.json"), 400).is_none());
}

#[test]
fn hit_cap_marks_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    std::fs::write(
            &path,
            serde_json::json!({
                "files": {"f1": {"ref_id": "tz://file/f1", "path": "a.log", "text": "hit\nhit\nhit\nhit"}}
            })
            .to_string(),
        )
        .unwrap();
    let outcome = recall_search(&path, "HIT", 2);
    assert_eq!(outcome.hits.len(), 2);
    assert!(outcome.truncated);
}
