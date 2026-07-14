use super::fixtures::*;
use super::*;

fn rejected(report: &serde_json::Value) -> &[serde_json::Value] {
    assert_eq!(report["ok"], false);
    report["issues"].as_array().unwrap()
}

fn has_issue(
    issues: &[serde_json::Value],
    code: &str,
    member: Option<&str>,
    reason: Option<&str>,
) -> bool {
    issues.iter().any(|issue| {
        issue["code"] == code
            && member.map_or(true, |value| issue["member"] == value)
            && reason.map_or(true, |value| issue["reason"] == value)
    })
}

fn write_raw_tar(path: &Path, header: [u8; 512]) {
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    fs::write(path, bytes).unwrap();
}

#[test]
fn package_audit_rejects_external_runtime() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("package.txt");
    fs::write(&artifact, format!("{} tokenzero", ["uv", " run"].concat())).unwrap();
    assert_eq!(package_audit(dir.path(), &[artifact])["ok"], false);
}

#[test]
fn package_audit_rejects_dev_target_launcher() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join(".tokenzero/bin/tokenzero.cmd");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(
        &artifact,
        "@echo off\r\n\"C:\\repo\\target\\release\\tokenzero.exe\" %*\r\n",
    )
    .unwrap();
    let report = package_audit(dir.path(), &[artifact]);
    assert!(has_issue(rejected(&report), "dev_runtime_launcher", None, None));
}

#[test]
fn package_audit_rejects_private_archive_members() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    write_test_tar(
        &artifact,
        &[
            "tokenzero-v0.1.1/._LICENSE",
            "tokenzero-v0.1.1/.tokenzero/config.json",
            "tokenzero-v0.1.1/.env",
            "tokenzero-v0.1.1/src/lib.rs",
        ],
    );
    let report = package_audit(dir.path(), &[artifact]);
    let issues = rejected(&report);
    for code in ["appledouble_metadata", "private_tool_state_member", "sensitive_member_name"] {
        assert!(has_issue(issues, code, None, None));
    }
}

#[test]
fn package_audit_rejects_local_generated_archive_members() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let members = [
        "tokenzero-v0.1.1/crash.dmp",
        "tokenzero-v0.1.1/prompt-transcript.md",
        "tokenzero-v0.1.1/chat-export.json",
        "tokenzero-v0.1.1/debug-report.txt",
        "tokenzero-v0.1.1/screenshot.png",
    ];
    write_test_tar(&artifact, &members);
    let report = package_audit(dir.path(), &[artifact]);
    let issues = rejected(&report);
    for member in members {
        assert!(
            has_issue(issues, "local_generated_member", Some(member), None),
            "missing local_generated_member issue for {member}"
        );
    }
}

#[test]
fn package_audit_fails_closed_on_archive_member_control_characters() {
    let dir = tempdir().unwrap();
    let tar_artifact = dir.path().join("release.tar");
    let zip_artifact = dir.path().join("release.zip");
    let tar_member = "tokenzero-v0.1.1/bin/tokenzero\nshim";
    let zip_member = "tokenzero-v0.1.1/bin/tokenzero\0shim";
    write_test_tar(&tar_artifact, &[tar_member]);
    write_test_zip(&zip_artifact, &[ZipTestEntry::file(zip_member, b"")]);
    let report = package_audit(dir.path(), &[tar_artifact, zip_artifact]);
    let issues = rejected(&report);
    assert!(has_issue(
        issues,
        "archive_member_name_uninspectable",
        Some(tar_member),
        Some("control_character"),
    ));
    assert!(has_issue(
        issues,
        "archive_member_name_uninspectable",
        Some(zip_member),
        Some("nul_byte"),
    ));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_tar_member_name() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let mut header = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 0, None);
    header[20] = 0xff;
    write_test_tar_checksum(&mut header);
    write_raw_tar(&artifact, header);
    let report = package_audit(dir.path(), &[artifact]);
    assert!(has_issue(
        rejected(&report),
        "archive_member_name_uninspectable",
        None,
        Some("invalid_utf8"),
    ));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_tar_link_target() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let mut header = test_tar_header(member, b'2', 0, Some("bin/tokenzero"));
    header[160] = 0xff;
    write_test_tar_checksum(&mut header);
    write_raw_tar(&artifact, header);
    let report = package_audit(dir.path(), &[artifact]);
    assert!(has_issue(
        rejected(&report),
        "archive_link_target_uninspectable",
        Some(member),
        Some("invalid_utf8"),
    ));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_zip_member_name() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, b"")]);
    let mut bytes = fs::read(&artifact).unwrap();
    let invalid_name_index = member.find("bin").unwrap();
    bytes[30 + invalid_name_index] = 0xff;
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    bytes[central + 46 + invalid_name_index] = 0xff;
    fs::write(&artifact, bytes).unwrap();
    let report = package_audit(dir.path(), &[artifact]);
    assert!(has_issue(
        rejected(&report),
        "archive_member_name_uninspectable",
        None,
        Some("invalid_utf8"),
    ));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_zip_symlink_target() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(member, b"bin/\xfftokenzero")],
    );
    let report = package_audit(dir.path(), &[artifact]);
    assert!(has_issue(
        rejected(&report),
        "archive_link_target_uninspectable",
        Some(member),
        Some("invalid_utf8"),
    ));
}
