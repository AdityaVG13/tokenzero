//! zerostack-rhd: a publisher must not be able to republish an object that an
//! in-flight sweep is about to unlink.
//!
//! The window is the gap between the sweeper's final "is anything still
//! referencing this hash" recheck and its `remove_file`. Before the fix a
//! publisher could slip in there, observe the object present, return Ok(hash),
//! and have the object unlinked underneath it -- silent data loss with a
//! dangling reference from a committed root.
//!
//! This is made deterministic with the `before_unlink` hook plus channels
//! rather than sleeps: GC is pinned at the exact window, and the assertion is
//! that a concurrent publish cannot complete while it is pinned.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime};
use tokenzero_recovery::shared_cas::{
    GcConfig, SharedCas, project_id, publish_reachability_snapshot, run_gc,
};

#[test]
fn publish_cannot_interleave_with_the_sweepers_unlink_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_root = dir.path().to_path_buf();
    let cas = SharedCas::new(store_root.clone());

    // A live root has to exist, otherwise the mark phase is "uncertain" and
    // retains everything rather than reaching the unlink path at all.
    let anchor = cas.publish(b"rhd anchor object").expect("anchor publish");
    let project = project_id(&store_root).expect("project id");
    publish_reachability_snapshot(
        &store_root,
        "tokenzero",
        &project,
        1,
        std::slice::from_ref(&anchor),
    )
    .expect("publish roots");

    let payload = b"zerostack-rhd interleaving payload";
    let hash = cas.publish(payload).expect("seed publish");
    assert!(cas.contains(&hash), "seeded object must exist");

    // GC signals when it is pinned at the unlink window; the test signals back
    // once it has confirmed the publisher is blocked.
    let (at_window_tx, at_window_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();

    let publish_completed = Arc::new(AtomicBool::new(false));

    let hook_completed = Arc::clone(&publish_completed);
    // Channel endpoints are Send but not Sync, and the hook is a Fn + Sync.
    let at_window_tx = Mutex::new(at_window_tx);
    let release_rx = Mutex::new(release_rx);
    let config = GcConfig {
        run_id: "rhd-window".into(),
        grace_seconds: 0,
        min_age_seconds: 0,
        apply: true,
        now: SystemTime::now() + Duration::from_secs(86_400),
        before_unlink: Some(Arc::new(move |_hash: &str| {
            at_window_tx
                .lock()
                .expect("window sender")
                .send(())
                .expect("signal window");
            release_rx
                .lock()
                .expect("release receiver")
                .recv()
                .expect("await release");
            // The publisher must still be blocked at this point. If it had
            // completed, it would have observed an object GC is about to
            // delete.
            assert!(
                !hook_completed.load(Ordering::SeqCst),
                "publish completed while the sweeper held the unlink window: \
                 the shared coordinator lock is not excluding publishers"
            );
        })),
        ..GcConfig::default()
    };

    let gc_root = store_root.clone();
    let gc = std::thread::spawn(move || run_gc(&gc_root, &config));

    at_window_rx.recv().expect("gc reaches unlink window");

    let publisher_root = store_root.clone();
    let publisher_flag = Arc::clone(&publish_completed);
    let publisher = std::thread::spawn(move || {
        let cas = SharedCas::new(publisher_root);
        let result = cas.publish(b"zerostack-rhd interleaving payload");
        publisher_flag.store(true, Ordering::SeqCst);
        result
    });

    // The publisher must not be able to finish while GC is pinned. Before the
    // fix it completed immediately here.
    std::thread::sleep(Duration::from_millis(250));
    assert!(
        !publish_completed.load(Ordering::SeqCst),
        "publish returned during the sweeper's unlink window; \
         publishers are not taking the shared coordinator lock"
    );

    release_tx.send(()).expect("release gc");
    gc.join().expect("gc thread").expect("gc run");
    let republished = publisher.join().expect("publisher thread");

    // Once GC has finished, the publish is allowed to proceed -- and because it
    // now runs after the unlink rather than inside the window, it must have
    // recreated the object rather than handing back a hash to a deleted file.
    let hash = republished.expect("publish succeeds after gc releases");
    assert!(
        cas.contains(&hash),
        "publish returned Ok({hash}) but the object is not on disk"
    );
    let bytes = cas.resolve(&hash).expect("republished object readable");
    assert_eq!(bytes, payload, "republished bytes must round-trip");
}
