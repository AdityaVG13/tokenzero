use super::*;
use crate::working_set::WorkingSet;
use crate::RecoveryStore;
use std::fs;
use tempfile::tempdir;
use tokenzero_core::ContentType;

fn request(verb: MemoryVerb) -> MemoryVerbRequest {
    MemoryVerbRequest {
        verb,
        ref_ids: Vec::new(),
        payload: None,
        label: None,
    }
}

#[test]
fn tzfmeo_six_verbs_name_a_substrate_and_do_not_apply() {
    let names: Vec<_> = MemoryVerb::ALL.iter().map(|v| v.as_str()).collect();
    assert_eq!(
        names,
        [
            "store",
            "commit_session",
            "update_capsule",
            "forget_visible",
            "promote_anchor",
            "link_refs"
        ]
    );
    for verb in MemoryVerb::ALL {
        assert!(!verb.substrate_target().is_empty(), "{verb:?}");
        let effect = describe_memory_verb(&MemoryVerbRequest {
            verb,
            ref_ids: vec!["tz://blob/deadbeef".into()],
            payload: None,
            label: None,
        });
        assert!(!effect.applied, "{verb:?} stub must not apply");
        assert_eq!(effect.substrate, verb.substrate_target());
    }
}

#[test]
fn unknown_verb_name_fails_loud() {
    match MemoryVerb::from_name("eat_memory") {
        Err(MemoryVerbError::UnknownVerb(name)) => assert_eq!(name, "eat_memory"),
        other => panic!("expected UnknownVerb, got {other:?}"),
    }
}

#[test]
fn apply_store_and_update_mutate_working_set_describe_stays_false() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(8192);
    let mut store_req = request(MemoryVerb::Store);
    store_req.payload = Some("first capsule body\n".into());
    store_req.label = Some("src/capsule.rs".into());
    let effect = apply_memory_verb(&mut set, &mut store, &store_req).unwrap();
    assert!(effect.applied);
    assert_eq!(effect.substrate, "working_set.admit");
    assert_eq!(set.telemetry().admissions, 1);
    assert_eq!(set.visible_lines(), vec!["first capsule body\n"]);
    assert!(!describe_memory_verb(&store_req).applied);

    let mut update = request(MemoryVerb::UpdateCapsule);
    update.payload = Some("rewritten capsule body\n".into());
    update.label = Some("src/capsule.rs".into());
    let effect = apply_memory_verb(&mut set, &mut store, &update).unwrap();
    assert!(effect.applied);
    assert_eq!(effect.substrate, "working_set.rewrite_render");
    assert_eq!(set.telemetry().render_rewrites, 1);
    assert_eq!(set.visible_lines(), vec!["rewritten capsule body\n"]);
}

#[test]
fn apply_promote_forget_and_link_mutate_or_fail_loud() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(8192);
    let mut store_req = request(MemoryVerb::Store);
    store_req.payload = Some("visible text that forget_visible must hide\n".into());
    store_req.label = Some("src/keep.rs".into());
    apply_memory_verb(&mut set, &mut store, &store_req).unwrap();

    let mut missing = request(MemoryVerb::PromoteAnchor);
    missing.ref_ids = vec!["999".into()];
    match apply_memory_verb(&mut set, &mut store, &missing) {
        Err(MemoryVerbError::NotApplied { verb, .. }) => assert_eq!(verb, "promote_anchor"),
        other => panic!("expected NotApplied, got {other:?}"),
    }

    let mut promote = request(MemoryVerb::PromoteAnchor);
    promote.ref_ids = vec!["1".into()];
    let effect = apply_memory_verb(&mut set, &mut store, &promote).unwrap();
    assert!(effect.applied);
    assert_eq!(effect.substrate, "working_set.touch");

    let mut forget = request(MemoryVerb::ForgetVisible);
    forget.ref_ids = vec!["1".into()];
    let effect = apply_memory_verb(&mut set, &mut store, &forget).unwrap();
    assert!(effect.applied);
    assert_eq!(effect.substrate, "working_set.evict");
    assert_eq!(set.telemetry().evictions, 1);
    assert!(!set.visible_lines()[0].contains("visible text"));
    let source = set
        .evicted_refs()
        .keys()
        .next()
        .expect("forget_visible must record a ref")
        .clone();

    let mut link = request(MemoryVerb::LinkRefs);
    link.ref_ids = vec![source.clone(), "tz://blob/alias-link".into()];
    let effect = apply_memory_verb(&mut set, &mut store, &link).unwrap();
    assert!(effect.applied);
    assert_eq!(effect.substrate, "working_set.evicted_refs");
    assert_eq!(
        set.evicted_refs().get(&source),
        set.evicted_refs().get("tz://blob/alias-link")
    );
    assert_eq!(
        store.alias_target("tz://blob/alias-link").as_deref(),
        Some(source.as_str())
    );
    let recovered = set
        .rehydrate_ref(&mut store, "tz://blob/alias-link", None, None)
        .unwrap()
        .expect("linked alias must demand-page the source span");
    assert!(!recovered.partial);
    assert!(set.evicted_refs().get("tz://blob/alias-link").is_none());
    assert!(store.alias_target("tz://blob/alias-link").is_none());
}

#[test]
fn apply_commit_session_persists_and_missing_path_fails_loud() {
    let mut mem = RecoveryStore::new(None);
    let mut set = WorkingSet::new(8192);
    match apply_memory_verb(&mut set, &mut mem, &request(MemoryVerb::CommitSession)) {
        Err(MemoryVerbError::NotApplied { verb, .. }) => assert_eq!(verb, "commit_session"),
        other => panic!("missing persist path must fail loud, got {other:?}"),
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("recovery-cache.json");
    let mut store = RecoveryStore::new(Some(path.clone()));
    store.store_blob_deferred("session bytes", ContentType::Unknown);
    let effect =
        apply_memory_verb(&mut set, &mut store, &request(MemoryVerb::CommitSession)).unwrap();
    assert!(effect.applied);
    assert_eq!(effect.substrate, "recovery_store.persist");
    assert!(path.exists());

    let snapshot_meta = fs::metadata(&path).unwrap();
    let snapshot_fp = (snapshot_meta.len(), snapshot_meta.modified().ok());
    store.store_blob_deferred("wal-appended session bytes", ContentType::Unknown);
    let effect =
        apply_memory_verb(&mut set, &mut store, &request(MemoryVerb::CommitSession)).unwrap();
    assert!(effect.applied, "WAL append after first snapshot must count as applied");
    let after_meta = fs::metadata(&path).unwrap();
    assert_eq!(
        (after_meta.len(), after_meta.modified().ok()),
        snapshot_fp,
        "second commit WAL-appends; snapshot len+mtime stays the old detector's identity"
    );
}

#[test]
fn apply_store_without_payload_fails_loud() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(8192);
    match apply_memory_verb(&mut set, &mut store, &request(MemoryVerb::Store)) {
        Err(MemoryVerbError::NotApplied { verb, .. }) => assert_eq!(verb, "store"),
        other => panic!("expected NotApplied, got {other:?}"),
    }
}
