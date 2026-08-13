use super::*;

#[test]
fn filesystem_entry_absence_distinguishes_missing_from_existing() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("entry");
    assert!(filesystem_entry_is_absent(&path));
    fs::write(&path, b"present").unwrap();
    assert!(!filesystem_entry_is_absent(&path));
}

#[cfg(unix)]
#[test]
fn filesystem_entry_absence_rejects_dangling_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let path = temp.path().join("dangling");
    symlink(temp.path().join("missing-target"), &path).unwrap();
    assert!(!filesystem_entry_is_absent(&path));
}

#[test]
fn failure_diagnosis_requires_failed_command_telemetry() {
    let anchors = ["exit_code: 101", "tests::alpha"];
    let visible = "exit_code: 101 tests::alpha";
    let success = object!({"telemetry": {"command_success": true}});
    let failure = object!({"telemetry": {"command_success": false}});

    assert!(!one_shot_anchors_ok(
        "failure_diagnosis_anchor",
        &success,
        visible,
        &anchors
    ));
    assert!(one_shot_anchors_ok(
        "failure_diagnosis_anchor",
        &failure,
        visible,
        &anchors
    ));
    assert!(one_shot_anchors_ok(
        "warning_changed_file_anchor",
        &success,
        visible,
        &anchors
    ));
}
