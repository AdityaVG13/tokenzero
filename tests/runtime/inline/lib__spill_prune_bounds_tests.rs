use super::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn prune_spill_dir_respects_scan_budget_before_sort() {
    let dir = tempdir().unwrap();
    for i in 0..64 {
        let path = dir.path().join(format!("tokenzero-{i}-stdout.log"));
        fs::write(&path, vec![b'x'; (i % 8) + 1]).unwrap();
    }
    let report = prune_spill_dir_bounded(dir.path(), DEFAULT_SPILL_TTL, 0, true, 16, None);
    assert_eq!(report.scan_budget, 16);
    assert!(report.scan_truncated);
    assert!(!report.deadline_elapsed);
    assert_eq!(report.scanned_files, 16);
    // Non-zero fresh spills with max_total_bytes=0 are all reclaimable.
    assert_eq!(report.removed_files, 16);
    assert_eq!(report.kept_files, 0);
}

#[test]
fn prune_spill_dir_aborts_when_deadline_already_elapsed() {
    let dir = tempdir().unwrap();
    for i in 0..8 {
        fs::write(dir.path().join(format!("tokenzero-{i}-stdout.log")), []).unwrap();
    }
    let report = prune_spill_dir_bounded(
        dir.path(),
        DEFAULT_SPILL_TTL,
        DEFAULT_SPILL_MAX_TOTAL_BYTES,
        true,
        128,
        Some(Instant::now() - Duration::from_millis(1)),
    );
    assert!(report.deadline_elapsed);
    assert_eq!(report.scanned_files, 0);
    assert!(!report.scan_truncated);
}
