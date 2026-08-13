use super::*;
use tempfile::tempdir;

fn key(n: u8) -> String {
    format!("{n:064x}")
}

fn entry(n: u8, artifact: &str) -> ActionCacheEntry {
    ActionCacheEntry {
        key: key(n),
        artifact_ref: artifact.to_string(),
        fszero_bookmark: None,
        dep_closure_ref: None,
        class: "must_block_revalidate".into(),
        verified: true,
        world_id: Some("w1".into()),
        tombstone: false,
        tombstoned_at_unix: None,
    }
}

#[test]
fn tzqjfi_put_get_roundtrip_and_tombstone() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let first = entry(1, "tz://blob/aaa");
    index.put(first.clone()).unwrap();
    assert_eq!(index.get(&key(1)).unwrap().as_ref(), Some(&first));
    assert_eq!(index.live_keys().unwrap(), vec![key(1)]);
    assert_eq!(
        index.live_artifact_refs().unwrap(),
        vec!["tz://blob/aaa".to_string()]
    );

    assert!(index.tombstone(&key(1)).unwrap());
    assert!(index.get(&key(1)).unwrap().is_none());
    assert!(index.live_keys().unwrap().is_empty());
    assert!(index.live_artifact_refs().unwrap().is_empty());
}

#[test]
fn tzqjfi_refuses_newer_major_segment() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let item = entry(2, "tz://blob/bbb");
    let path = index.segment_path(&item.key);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bad = serde_json::json!({
        "schema": "tokenzero.store",
        "major": 9,
        "minor": 0,
        "entry": item,
    });
    fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
    let err = index.get(&item.key).unwrap_err();
    assert!(
        matches!(err, ActionCacheError::Schema(SchemaSkewError::NewerMajor { found }) if found.major == 9),
        "{err}"
    );
}

#[test]
fn tzqjfi_tokenzero_owned_fields_do_not_require_sibling_pointers() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let mut item = entry(3, "tz://blob/ccc");
    item.fszero_bookmark = None;
    item.dep_closure_ref = None;
    index.put(item.clone()).unwrap();
    let got = index.get(&key(3)).unwrap().unwrap();
    assert!(got.fszero_bookmark.is_none());
    assert!(got.dep_closure_ref.is_none());
    assert_eq!(got.artifact_ref, "tz://blob/ccc");
    assert!(got.verified);
}

#[test]
fn tzgvxc_eviction_tombstones_index_before_blob_and_honors_grace() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let artifact = format!("tz://blob/{}", key(10));
    index.put(entry(10, &artifact)).unwrap();
    index.put(entry(11, &artifact)).unwrap();

    let early = index.prepare_blob_eviction(&artifact, 1_000, 60).unwrap();
    assert_eq!(early.tombstoned_keys.len(), 2);
    assert!(!early.may_delete_blob, "grace has not elapsed");
    assert!(index.get(&key(10)).unwrap().is_none());
    assert!(
        index
            .protects_hash(artifact_full_hash(&artifact).unwrap(), 1_000, 60)
            .unwrap(),
        "tombstoned entries still pin during grace"
    );

    let ready = index.prepare_blob_eviction(&artifact, 1_070, 60).unwrap();
    assert!(ready.may_delete_blob);
    assert!(ready.waiting_grace.is_empty());
}

#[test]
fn tzgvxc_concurrent_serve_never_sees_dangling_ref() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let dir = tempdir().unwrap();
    let index = Arc::new(ActionCacheIndex::open(dir.path()));
    let artifact = format!("tz://blob/{}", key(20));
    index.put(entry(20, &artifact)).unwrap();
    let blobs = Arc::new(Mutex::new(vec![artifact.clone()]));
    let dangling = Arc::new(Mutex::new(false));

    let server = {
        let index = Arc::clone(&index);
        let blobs = Arc::clone(&blobs);
        let dangling = Arc::clone(&dangling);
        let artifact = artifact.clone();
        thread::spawn(move || {
            for _ in 0..200 {
                match index.serve(&key(20)).unwrap() {
                    Some((entry, _pin)) => {
                        let live = blobs.lock().unwrap();
                        if !live.iter().any(|blob| blob == &entry.artifact_ref) {
                            *dangling.lock().unwrap() = true;
                        }
                        assert_eq!(entry.artifact_ref, artifact);
                    }
                    None => {}
                }
            }
        })
    };
    let gc = {
        let index = Arc::clone(&index);
        let blobs = Arc::clone(&blobs);
        thread::spawn(move || {
            let plan = index.prepare_blob_eviction(&artifact, 5_000, 0).unwrap();
            if plan.may_delete_blob {
                blobs.lock().unwrap().retain(|blob| blob != &artifact);
            }
        })
    };
    server.join().unwrap();
    gc.join().unwrap();
    assert!(
        !*dangling.lock().unwrap(),
        "serve must not observe a tombstoned or deleted blob"
    );
}
