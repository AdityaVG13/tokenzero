use super::*;
use std::fs;
use tempfile::tempdir;

const SOAK_CYCLES: usize = 64;
const PAYLOAD: &[u8] = b"tokenzero-digest-before-fragment-soak-payload-v1";

fn put_blob(cas: &zero_store::SharedCas, bytes: &[u8]) -> String {
    cas.put(bytes).expect("put blob into local SharedCas")
}

fn corrupt_object(cas: &zero_store::SharedCas, hash: &str) {
    let path = cas.object_path(hash);
    // Flip stored bytes in place so the path identity stays the same while
    // content no longer matches the requested SHA-256.
    let mut corrupt = fs::read(&path).expect("read published object");
    corrupt.reverse();
    if corrupt == PAYLOAD {
        corrupt.push(0xff);
    }
    fs::write(&path, &corrupt).expect("inject corrupt CAS bytes");
}

#[test]
fn digest_before_fragment_soak_trips_only_on_corrupt_cas() {
    let dir = tempdir().expect("temp cas root");
    let cas = zero_store::SharedCas::open(dir.path());
    let hash = put_blob(&cas, PAYLOAD);
    let fragment_range = 7..31;

    // Rates: one Bernoulli failure from a cold monitor crosses 1/alpha; a long
    // success prefix must not trip. Counts below are exact event tallies, not %.
    let mut soak = AnytimeFailureMonitor::new(0.05, 0.01, 0.5).expect("soak monitor");
    let expected_fragment = &PAYLOAD[fragment_range.clone()];

    for cycle in 0..SOAK_CYCLES {
        let served = serve_fragment_after_digest(&cas, &hash, fragment_range.clone())
            .unwrap_or_else(|error| {
                panic!("valid cycle {cycle}: expected Ok fragment, got {error}")
            });
        assert_eq!(
            served, expected_fragment,
            "valid cycle {cycle}: fragment bytes must match post-digest slice"
        );
        let snapshot = soak.observe_outcome(false);
        assert!(
            !snapshot.tripped,
            "valid cycle {cycle}: e-process must not trip on digest-ok CAS serves \
             (events={}, failures={})",
            snapshot.events, snapshot.failures
        );
        assert_eq!(snapshot.failures, 0);
        assert_eq!(snapshot.events, (cycle as u64) + 1);
    }

    let after_valid = soak.snapshot();
    assert!(!after_valid.tripped);
    assert_eq!(after_valid.events, SOAK_CYCLES as u64);
    assert_eq!(after_valid.failures, 0);

    corrupt_object(&cas, &hash);
    let corrupt_error = serve_fragment_after_digest(&cas, &hash, fragment_range)
        .expect_err("corrupt CAS must fail closed before returning a fragment");
    match &corrupt_error {
        FragmentServeError::DigestMismatch { expected, actual } => {
            assert_eq!(expected, &hash);
            assert_ne!(actual, &hash);
            assert_eq!(actual.len(), 64);
            assert!(
                actual
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "actual digest must be lowercase hex sha256"
            );
        }
        other => panic!("expected DigestMismatch typed error, got {other:?}"),
    }

    // The soak stream records the corrupt serve as a failure without claiming
    // a crossing after a long success prefix (e-value was driven down by
    // SOAK_CYCLES successes). A cold monitor with the same rates trips on that
    // single corrupt-CAS failure outcome.
    let after_corrupt_observe = soak.observe_outcome(true);
    assert_eq!(after_corrupt_observe.failures, 1);
    assert_eq!(after_corrupt_observe.events, (SOAK_CYCLES as u64) + 1);
    assert!(
        !after_corrupt_observe.tripped,
        "soak monitor must not invent a crossing after {SOAK_CYCLES} successes + 1 failure"
    );

    let mut trip = AnytimeFailureMonitor::new(0.05, 0.01, 0.5).expect("trip monitor");
    let tripped = trip.observe_outcome(true);
    assert!(
        tripped.tripped,
        "single corrupt-CAS failure must trip a cold e-process under declared rates \
         (e_value={}, threshold={})",
        tripped.e_value, tripped.threshold
    );
    assert_eq!(tripped.failures, 1);
    assert_eq!(tripped.crossing_event, Some(1));
}

#[test]
fn digest_mismatch_returns_no_fragment_bytes() {
    let dir = tempdir().expect("temp cas root");
    let cas = zero_store::SharedCas::open(dir.path());
    let hash = put_blob(&cas, PAYLOAD);
    corrupt_object(&cas, &hash);

    let error = serve_fragment_after_digest(&cas, &hash, 0..PAYLOAD.len())
        .expect_err("corrupt object must not yield fragment bytes");
    assert!(matches!(
        error,
        FragmentServeError::DigestMismatch { .. }
    ));
}
