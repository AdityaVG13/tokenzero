use super::*;
#[test]
fn every_inclusion_and_prefix_consistency_proof_verifies() {
    let mut log = MmrLog::default();
    let roots: Vec<_> = (0..19)
        .map(|i| {
            log.append(format!("event-{i}").as_bytes());
            log.root()
        })
        .collect();
    for i in 0..log.len() {
        assert!(log.inclusion_proof(i).unwrap().verify(&log.root()));
    }
    for old in 1..log.len() {
        assert!(
            log.consistency_proof(old)
                .unwrap()
                .verify(&roots[old - 1], &log.root())
        );
    }
    let mut tampered = log.inclusion_proof(3).unwrap();
    tampered.leaf_hash.push('0');
    assert!(!tampered.verify(&log.root()));
    let mut inconsistent = log.consistency_proof(7).unwrap();
    inconsistent.appended_leaf_hashes[0].push('0');
    assert!(!inconsistent.verify(&roots[6], &log.root()));
}
#[test]
fn concurrent_prefix_merge_preserves_both_suffixes() {
    let mut base = MmrLog::default();
    base.append(b"base");
    let mut a = base.clone();
    let mut b = base.clone();
    a.append(b"a");
    b.append(b"b");
    a.merge_concurrent(&b);
    assert_eq!(a.len(), 3);
    assert!(
        a.consistency_proof(1)
            .unwrap()
            .verify(&base.root(), &a.root())
    );
}
