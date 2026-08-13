//! tokenzero-8tdg: `publish_lease_record` had exactly one occurrence in the
//! whole workspace -- its own definition. Nothing ever wrote a lease, so the
//! designed publish-then-commit protection window, which `load_mark_state`
//! reads from `gc/leases/**` with a 60s floor, protected nothing at runtime.
//!
//! These tests pin the behaviour the mechanism was built for: an object that is
//! published but not yet referenced by any root must survive a sweep while its
//! lease is live, and must become collectable once the lease is released.

use std::time::{Duration, SystemTime};

use tokenzero_recovery::shared_cas::{
    GcConfig, GcVerdict, SharedCas, project_id, publish_reachability_snapshot,
};

fn verdict_for(store: &std::path::Path, hash: &str, now: SystemTime) -> GcVerdict {
    let config = GcConfig {
        run_id: "0123456789abcdef".into(),
        now,
        min_age_seconds: 0,
        ..GcConfig::default()
    };
    tokenzero_recovery::shared_cas::run_gc(store, &config)
        .unwrap()
        .objects
        .into_iter()
        .find(|object| object.blob_hash == hash)
        .unwrap_or_else(|| panic!("gc report never evaluated {hash}"))
        .verdict
}

#[test]
fn leased_publish_survives_a_sweep_before_any_root_commits() {
    let store = tempfile::tempdir().unwrap();
    let cas = SharedCas::new(store.path().to_path_buf());
    let project = project_id(store.path()).unwrap();
    // Without a reachability root the sweep marks EVERYTHING RetainUncertain and
    // a lease test would pass vacuously. Anchor one unrelated rooted object so
    // the mark phase is certain and the verdicts below actually mean something.
    let anchor = cas.publish(b"anchor").unwrap();
    publish_reachability_snapshot(
        store.path(),
        "tokenzero",
        &project,
        1,
        std::slice::from_ref(&anchor),
    )
    .unwrap();

    // Publish WITHOUT committing a root: exactly the window the lease exists to
    // cover. An unleased publish is unprotected here.
    let unleased = cas.publish(b"unleased payload").unwrap();
    let leased = cas
        .publish_leased(b"leased payload", &project, "0123456789abcdef", 300)
        .unwrap();

    let now = SystemTime::now();
    assert_eq!(
        verdict_for(store.path(), &leased, now),
        GcVerdict::Retain,
        "a live lease must keep an unrooted object out of the sweep"
    );
    // The contrast is the point: same store, same sweep, no lease.
    assert_ne!(
        verdict_for(store.path(), &unleased, now),
        GcVerdict::Retain,
        "unleased publish should not be retained, or the test proves nothing"
    );
}

#[test]
fn released_lease_stops_protecting_once_grace_expires() {
    let store = tempfile::tempdir().unwrap();
    let cas = SharedCas::new(store.path().to_path_buf());
    let project = project_id(store.path()).unwrap();
    // Without a reachability root the sweep marks EVERYTHING RetainUncertain and
    // a lease test would pass vacuously. Anchor one unrelated rooted object so
    // the mark phase is certain and the verdicts below actually mean something.
    let anchor = cas.publish(b"anchor").unwrap();
    publish_reachability_snapshot(
        store.path(),
        "tokenzero",
        &project,
        1,
        std::slice::from_ref(&anchor),
    )
    .unwrap();
    let hash = cas
        .publish_leased(b"leased payload", &project, "fedcba9876543210", 300)
        .unwrap();

    cas.release_lease(&project, "fedcba9876543210").unwrap();

    assert_ne!(
        verdict_for(store.path(), &hash, SystemTime::now()),
        GcVerdict::Retain,
        "releasing the lease must actually drop the protection"
    );
}

#[test]
fn expired_lease_still_protects_through_the_grace_floor() {
    let store = tempfile::tempdir().unwrap();
    let cas = SharedCas::new(store.path().to_path_buf());
    let project = project_id(store.path()).unwrap();
    // Without a reachability root the sweep marks EVERYTHING RetainUncertain and
    // a lease test would pass vacuously. Anchor one unrelated rooted object so
    // the mark phase is certain and the verdicts below actually mean something.
    let anchor = cas.publish(b"anchor").unwrap();
    publish_reachability_snapshot(
        store.path(),
        "tokenzero",
        &project,
        1,
        std::slice::from_ref(&anchor),
    )
    .unwrap();
    let hash = cas
        .publish_leased(b"grace payload", &project, "abcdefabcdef0123", 1)
        .unwrap();

    // Past expiry but inside GC_MIN_GRACE_SECONDS: a crashed publisher must not
    // lose its object the instant the lease lapses.
    let just_after_expiry = SystemTime::now() + Duration::from_secs(70);
    assert_eq!(
        verdict_for(store.path(), &hash, just_after_expiry),
        GcVerdict::Retain,
        "grace window must outlive the lease itself"
    );
}

#[test]
fn tzmb70_lease_seconds_are_clamped_off_system_time_overflow() {
    use tokenzero_recovery::shared_cas::{
        GC_MIN_GRACE_SECONDS, MAX_GC_GRACE_SECONDS, MAX_LEASE_SECONDS, clamp_grace_seconds,
        clamp_lease_seconds,
    };
    assert_eq!(clamp_lease_seconds(0), GC_MIN_GRACE_SECONDS);
    assert_eq!(clamp_lease_seconds(u64::MAX), MAX_LEASE_SECONDS);
    assert_eq!(clamp_lease_seconds(300), 300);
    assert_eq!(clamp_grace_seconds(u64::MAX), MAX_GC_GRACE_SECONDS);
    assert_eq!(clamp_grace_seconds(1), GC_MIN_GRACE_SECONDS);
}

#[test]
fn tzmb70_stale_lease_record_is_unlinked_after_grace() {
    use tokenzero_recovery::shared_cas::{
        GC_MIN_GRACE_SECONDS, GC_RECORD_TYPE_LEASE, GC_SCHEMA_VERSION, LeaseOwner, LeaseRecord,
        gc_contract_digest_hex, prune_stale_lease_records,
    };

    let store = tempfile::tempdir().unwrap();
    let hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let dir = store.path().join("gc/leases/tokenzero").join(hash);
    std::fs::create_dir_all(&dir).unwrap();
    let lease = LeaseRecord {
        schema_version: GC_SCHEMA_VERSION.into(),
        record_type: GC_RECORD_TYPE_LEASE.into(),
        engine: "tokenzero".into(),
        project_id: hash.into(),
        store_contract_digest: Some(gc_contract_digest_hex()),
        operation_id: "stale-op".into(),
        epoch: 1,
        owner: LeaseOwner {
            pid: 1,
            host: "test".into(),
        },
        started_at: "2020-01-01T00:00:00Z".into(),
        expires_at: "2020-01-01T00:05:00Z".into(),
        grace_seconds: GC_MIN_GRACE_SECONDS,
        blob_hashes: vec![hash.into()],
    };
    let path = dir.join("stale-op.json");
    std::fs::write(&path, serde_json::to_vec(&lease).unwrap()).unwrap();

    let removed = prune_stale_lease_records(store.path(), SystemTime::now(), GC_MIN_GRACE_SECONDS);
    assert_eq!(removed, 1);
    assert!(!path.exists(), "expired lease past grace must be unlinked");
}
