#[cfg(test)]
fn find_zip_eocd(bytes: &[u8]) -> Option<usize> {
    zip_eocd_candidates(bytes).into_iter().next()
}

use super::*;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn package_audit_rejects_external_runtime() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("package.txt");
    fs::write(&artifact, format!("{} tokenzero", ["uv", " run"].concat())).unwrap();
    let report = package_audit(dir.path(), &[artifact]);
    assert_eq!(report["ok"], false);
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

    assert_eq!(report["ok"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "dev_runtime_launcher")
    );
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
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(
        issues
            .iter()
            .any(|issue| issue["code"] == "appledouble_metadata")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue["code"] == "private_tool_state_member")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue["code"] == "sensitive_member_name")
    );
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
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    for member in members {
        assert!(
            issues.iter().any(|issue| {
                issue["code"] == "local_generated_member" && issue["member"] == member
            }),
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
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_name_uninspectable"
            && issue["member"] == tar_member
            && issue["reason"] == "control_character"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_name_uninspectable"
            && issue["member"] == zip_member
            && issue["reason"] == "nul_byte"
    }));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_tar_member_name() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");

    let mut header = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 0, None);
    header[20] = 0xff;
    write_test_tar_checksum(&mut header);
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_name_uninspectable" && issue["reason"] == "invalid_utf8"
    }));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_tar_link_target() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero";

    let mut header = test_tar_header(member, b'2', 0, Some("bin/tokenzero"));
    header[160] = 0xff;
    write_test_tar_checksum(&mut header);
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_uninspectable"
            && issue["member"] == member
            && issue["reason"] == "invalid_utf8"
    }));
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
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    bytes[central_directory_offset + 46 + invalid_name_index] = 0xff;
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_name_uninspectable" && issue["reason"] == "invalid_utf8"
    }));
}

#[test]
fn package_audit_fails_closed_on_non_utf8_zip_symlink_target() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let target = b"bin/\xfftokenzero";
    write_test_zip(&artifact, &[ZipTestEntry::symlink(member, target)]);

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_uninspectable"
            && issue["member"] == member
            && issue["reason"] == "invalid_utf8"
    }));
}

#[test]
fn package_audit_rejects_tar_archive_dev_target_launcher_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let payload = b"#!/bin/sh\nexec target/release/tokenzero \"$@\"\n";

    write_test_tar_entries(&artifact, &[TarTestEntry::new(member, b'0', payload)]);

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(
        issues
            .iter()
            .any(|issue| { issue["code"] == "dev_runtime_launcher" && issue["member"] == member })
    );
}

#[test]
fn package_audit_rejects_zip_archive_external_runtime_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero.cmd";
    let payload = b"@echo off\r\nuv run tokenzero %*\r\n";
    let compressed_payload = deflate_bytes(payload);

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, &compressed_payload).with_method(8)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "external_runtime_dependency" && issue["member"] == member
    }));
}

#[test]
fn package_audit_fails_closed_on_archive_link_target_control_characters() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let link_target = "bin/tokenzero\rshim";

    write_test_tar_entries(
        &artifact,
        &[TarTestEntry::new(member, b'2', b"").with_link_target(link_target)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_uninspectable"
            && issue["member"] == member
            && issue["link_target"] == link_target
            && issue["reason"] == "control_character"
    }));
}

#[test]
fn package_audit_rejects_private_gzip_tar_members_in_process() {
    let dir = tempdir().unwrap();
    let tar_path = dir.path().join("release.tar");
    let artifact = dir.path().join("release.tar.gz");
    write_test_tar(
        &tar_path,
        &[
            "tokenzero-v0.1.1/._LICENSE",
            "tokenzero-v0.1.1/.tokenzero/config.json",
        ],
    );
    fs::write(&artifact, gzip_bytes(&fs::read(&tar_path).unwrap())).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(
        issues
            .iter()
            .any(|issue| issue["code"] == "appledouble_metadata")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue["code"] == "private_tool_state_member")
    );
}

#[test]
fn package_audit_rejects_concatenated_gzip_tar_members() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar.gz");
    let visible_fragment = test_tar_entry_bytes("tokenzero-v0.1.1/LICENSE", b"MIT");
    let mut hidden_fragment =
        test_tar_entry_bytes("tokenzero-v0.1.1/.tokenzero/config.json", b"{}");
    hidden_fragment.extend_from_slice(&[0u8; 1024]);

    let mut bytes = gzip_bytes(&visible_fragment);
    bytes.extend_from_slice(&gzip_bytes(&hidden_fragment));
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member"
            && issue["member"] == "tokenzero-v0.1.1/.tokenzero/config.json"
    }));
}

#[test]
fn package_audit_fails_closed_on_tar_missing_end_marker() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    fs::write(
        &artifact,
        test_tar_entry_bytes("tokenzero-v0.1.1/LICENSE", b"MIT"),
    )
    .unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_metadata_malformed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("end-of-archive marker"))
    }));
}

#[test]
fn package_audit_fails_closed_on_tar_trailing_data_after_end_marker() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    write_test_tar(&artifact, &["tokenzero-v0.1.1/LICENSE"]);

    let mut bytes = fs::read(&artifact).unwrap();
    bytes.extend_from_slice(&test_tar_entry_bytes(
        "tokenzero-v0.1.1/.tokenzero/config.json",
        b"{}",
    ));
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_trailing_data"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("end-of-archive marker"))
    }));
}

#[test]
fn package_audit_fails_closed_on_tar_private_owner_metadata() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/LICENSE";

    let mut header = test_tar_header(member, b'0', 0, None);
    write_tar_octal(&mut header[108..116], 501);
    write_tar_octal(&mut header[116..124], 20);
    header[265..271].copy_from_slice(b"aditya");
    header[297..302].copy_from_slice(b"staff");
    write_test_tar_checksum(&mut header);
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&[0u8; 1024]);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    let issue = issues
        .iter()
        .find(|issue| {
            issue["code"] == "archive_private_owner_metadata" && issue["member"] == member
        })
        .unwrap_or_else(|| panic!("missing owner metadata issue: {report:#}"));
    let fields = issue["fields"].as_array().unwrap();
    for field in ["uid", "gid", "uname", "gname"] {
        assert!(
            fields.iter().any(|value| value == field),
            "missing {field} field in {issue:#}"
        );
    }
}

#[test]
fn package_audit_fails_closed_on_tar_special_member_types() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let char_device = "tokenzero-v0.1.1/dev/null";
    let fifo = "tokenzero-v0.1.1/run/install.fifo";
    let sparse_launcher = "tokenzero-v0.1.1/bin/tokenzero";

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new(char_device, b'3', b""),
            TarTestEntry::new(fifo, b'6', b""),
            TarTestEntry::new(sparse_launcher, b'S', b"target/release/tokenzero"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    for (member, reason) in [
        (char_device, "character_device"),
        (fifo, "fifo"),
        (sparse_launcher, "sparse_file"),
    ] {
        assert!(
            issues.iter().any(|issue| {
                issue["code"] == "archive_unsupported_member_type"
                    && issue["member"] == member
                    && issue["reason"] == reason
            }),
            "missing unsupported type issue for {member}: {report:#}"
        );
    }
}

#[test]
fn package_audit_rejects_gnu_longlink_sensitive_member() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let long_member = format!(
        "tokenzero-v0.1.1/{}/{}/{}/.env",
        "a".repeat(90),
        "b".repeat(90),
        "c".repeat(90)
    );

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("././@LongLink", b'L', format!("{long_member}\0").as_bytes()),
            TarTestEntry::new("payload.txt", b'0', b""),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_member_name" && issue["member"] == long_member
    }));
}

#[test]
fn package_audit_rejects_pax_path_private_member() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let pax_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax_payload = pax_record("path", pax_member);

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config.json", b'x', &pax_payload),
            TarTestEntry::new("config.json", b'0', b""),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == pax_member
    }));
}

#[test]
fn package_audit_accepts_empty_pax_path_delete_with_header_name() {
    let dir = tempfile::Builder::new()
        .prefix("tokenzero-test-")
        .tempdir()
        .unwrap();
    let artifact = dir.path().join("release.tar");
    let pax_payload = pax_record("path", "");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/LICENSE", b'x', &pax_payload),
            TarTestEntry::new("tokenzero-v0.1.1/LICENSE", b'0', b"MIT"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);

    assert_eq!(report["ok"], true, "{report:#}");
}

#[test]
fn package_audit_accepts_empty_pax_linkpath_delete_with_header_target() {
    let dir = tempfile::Builder::new()
        .prefix("tokenzero-test-")
        .tempdir()
        .unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let safe_link_target = "bin/tokenzero";
    let pax_payload = pax_record("linkpath", "");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/tokenzero-link", b'x', &pax_payload),
            TarTestEntry::new(member, b'2', b"").with_link_target(safe_link_target),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);

    assert_eq!(report["ok"], true, "{report:#}");
}

#[test]
fn package_audit_empty_pax_path_suppresses_global_pax_path_for_member() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";

    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_record("path", global_path)),
            TarTestEntry::new("./PaxHeaders.0/payload.bin", b'x', &pax_record("path", "")),
            TarTestEntry::new("payload.bin", b'0', &inner_bytes),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_global_pax_override_present" && issue["field"] == "path"
    }));
    assert!(
        !issues.iter().any(|issue| {
            issue["code"] == "private_tool_state_member" && issue["member"] == nested_member
        }),
        "global PAX path should be deleted for the payload member: {report:#}"
    );
}

#[test]
fn package_audit_empty_pax_linkpath_suppresses_global_pax_linkpath_for_member() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let global_link_target = "../.env";
    let safe_link_target = "bin/tokenzero";

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new(
                "./GlobalHead.0",
                b'g',
                &pax_record("linkpath", global_link_target),
            ),
            TarTestEntry::new(
                "./PaxHeaders.0/tokenzero-link",
                b'x',
                &pax_record("linkpath", ""),
            ),
            TarTestEntry::new(member, b'2', b"").with_link_target(safe_link_target),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_global_pax_override_present" && issue["field"] == "linkpath"
    }));
    assert!(
        !issues.iter().any(|issue| {
            issue["code"] == "archive_link_target_escape"
                && issue["member"] == member
                && issue["link_target"] == global_link_target
        }),
        "global PAX linkpath should be deleted for the symlink member: {report:#}"
    );
}

#[test]
fn package_audit_fails_closed_on_invalid_tar_size() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");

    let mut header = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 0, None);
    header[124..136].copy_from_slice(b"not-octal\0\0\0");
    write_test_tar_checksum(&mut header);
    fs::write(&artifact, header).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_size_malformed"
            && issue["member"] == "tokenzero-v0.1.1/LICENSE"
    }));
}

#[test]
fn package_audit_reads_bounded_tar_base256_size() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/.env";
    let payload = b"license";

    let mut bytes = test_tar_entry_bytes(member, payload);
    write_tar_base256(&mut bytes[124..136], payload.len() as u128);
    write_test_tar_checksum_bytes(&mut bytes[0..512]);
    bytes.extend_from_slice(&[0u8; 1024]);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(
        issues
            .iter()
            .any(|issue| { issue["code"] == "sensitive_member_name" && issue["member"] == member }),
        "bounded base-256 tar size should allow member inspection: {report:#}"
    );
    assert!(
        !issues
            .iter()
            .any(|issue| issue["code"] == "archive_member_size_malformed"),
        "bounded base-256 tar size should be inspected, not rejected: {report:#}"
    );
}

#[test]
fn package_audit_fails_closed_on_negative_tar_base256_size() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");

    let mut header = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 0, None);
    header[124..136].fill(0xff);
    write_test_tar_checksum(&mut header);
    fs::write(&artifact, header).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_size_malformed"
            && issue["member"] == "tokenzero-v0.1.1/LICENSE"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("negative base-256"))
    }));
}

#[test]
fn package_audit_fails_closed_on_oversized_tar_base256_size() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");

    let mut header = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 0, None);
    header[124..136].fill(0);
    header[124] = 0x81;
    write_test_tar_checksum(&mut header);
    fs::write(&artifact, header).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_size_malformed"
            && issue["member"] == "tokenzero-v0.1.1/LICENSE"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("too large"))
    }));
}

#[test]
fn package_audit_fails_closed_on_invalid_tar_checksum() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");

    let mut header = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 0, None);
    header[148..156].copy_from_slice(b"000000\0 ");
    fs::write(&artifact, header).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_metadata_malformed"
            && issue["member"] == "tokenzero-v0.1.1/LICENSE"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("checksum"))
    }));
}

#[test]
fn package_audit_fails_closed_on_truncated_tar_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");

    let mut bytes = test_tar_header("tokenzero-v0.1.1/LICENSE", b'0', 16, None).to_vec();
    bytes.extend_from_slice(b"partial");
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_payload_truncated"
            && issue["member"] == "tokenzero-v0.1.1/LICENSE"
    }));
}

#[test]
fn package_audit_fails_closed_on_malformed_pax_path() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let hidden_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let malformed_pax = format!("999 path={hidden_member}\n");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config.json", b'x', malformed_pax.as_bytes()),
            TarTestEntry::new("config.json", b'0', b""),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_metadata_malformed"
            && issue["member"] == "./PaxHeaders.0/config.json"
    }));
}

#[test]
fn package_audit_fails_closed_on_duplicate_pax_overrides() {
    let dir = tempdir().unwrap();
    let path_artifact = dir.path().join("duplicate-path.tar");
    let linkpath_artifact = dir.path().join("duplicate-linkpath.tar");
    let hidden_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let safe_member = "tokenzero-v0.1.1/config.json";
    let hidden_link_target = "../.env";
    let safe_link_target = "config.json";

    let mut duplicate_path = pax_record("path", hidden_member);
    duplicate_path.extend_from_slice(&pax_record("path", safe_member));
    write_test_tar_entries(
        &path_artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config.json", b'x', &duplicate_path),
            TarTestEntry::new("config.json", b'0', b"{}"),
        ],
    );

    let mut duplicate_linkpath = pax_record("linkpath", hidden_link_target);
    duplicate_linkpath.extend_from_slice(&pax_record("linkpath", safe_link_target));
    write_test_tar_entries(
        &linkpath_artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config-link", b'x', &duplicate_linkpath),
            TarTestEntry::new("config-link", b'2', b"").with_link_target(safe_link_target),
        ],
    );

    let report = package_audit(dir.path(), &[path_artifact, linkpath_artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    for (member, duplicate_field) in [
        ("./PaxHeaders.0/config.json", "path"),
        ("./PaxHeaders.0/config-link", "linkpath"),
    ] {
        assert!(
            issues.iter().any(|issue| {
                issue["code"] == "archive_member_metadata_malformed"
                    && issue["member"] == member
                    && issue["detail"]
                        .as_str()
                        .is_some_and(|detail| detail.contains(duplicate_field))
            }),
            "missing duplicate {duplicate_field} issue: {report:#}"
        );
    }
}

#[test]
fn package_audit_fails_closed_on_pax_private_metadata_fields() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let mut pax_payload = pax_record("uname", "builder");
    pax_payload.extend_from_slice(&pax_record("comment", "/tmp/example/release"));

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/LICENSE", b'x', &pax_payload),
            TarTestEntry::new("tokenzero-v0.1.1/LICENSE", b'0', b"MIT"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    let issue = issues
        .iter()
        .find(|issue| {
            issue["code"] == "archive_pax_metadata_present"
                && issue["member"] == "./PaxHeaders.0/LICENSE"
        })
        .unwrap_or_else(|| panic!("missing PAX metadata issue: {report:#}"));
    let fields = issue["fields"].as_array().unwrap();
    for field in ["uname", "comment"] {
        assert!(
            fields.iter().any(|value| value == field),
            "missing {field} field in {issue:#}"
        );
    }
    let serialized = serde_json::to_string(issue).unwrap();
    assert!(!serialized.contains("builder"));
    assert!(!serialized.contains("/tmp/example"));
}

#[test]
fn package_audit_fails_closed_on_global_pax_metadata_fields() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let pax_payload = pax_record("SCHILY.xattr.com.apple.quarantine", "local-machine");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new("tokenzero-v0.1.1/LICENSE", b'0', b"MIT"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_pax_metadata_present"
            && issue["member"] == "./GlobalHead.0"
            && issue["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "SCHILY.xattr.*")
    }));
}

#[test]
fn package_audit_fails_closed_on_benign_global_pax_path_override() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/LICENSE";
    let pax_payload = pax_record("path", global_path);

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new("LICENSE", b'0', b"MIT"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    let issue = issues
        .iter()
        .find(|issue| {
            issue["code"] == "archive_global_pax_override_present"
                && issue["member"] == "./GlobalHead.0"
                && issue["field"] == "path"
        })
        .unwrap_or_else(|| panic!("missing global PAX path issue: {report:#}"));
    let serialized = serde_json::to_string(issue).unwrap();
    assert!(!serialized.contains(global_path));
}

#[test]
fn package_audit_fails_closed_on_benign_global_pax_linkpath_override() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let global_link_target = "bin/tokenzero";
    let pax_payload = pax_record("linkpath", global_link_target);

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new(member, b'2', b"").with_link_target(global_link_target),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    let issue = issues
        .iter()
        .find(|issue| {
            issue["code"] == "archive_global_pax_override_present"
                && issue["member"] == "./GlobalHead.0"
                && issue["field"] == "linkpath"
        })
        .unwrap_or_else(|| panic!("missing global PAX linkpath issue: {report:#}"));
    let serialized = serde_json::to_string(issue).unwrap();
    assert!(!serialized.contains(global_link_target));
}

#[test]
fn package_audit_applies_global_pax_path_to_nested_archive_payload() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax_payload = pax_record("path", global_path);

    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new("payload.bin", b'0', &inner_bytes),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member"
            && issue["path"]
                .as_str()
                .is_some_and(|path| path.contains("release.tar!") && path.contains(global_path))
            && issue["member"] == nested_member
    }));
}

#[test]
fn package_audit_applies_global_pax_path_to_duplicate_detection() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/LICENSE";
    let pax_payload = pax_record("path", global_path);

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new("first.txt", b'0', b"first"),
            TarTestEntry::new("second.txt", b'0', b"second"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "tar_duplicate_member_name" && issue["member"] == global_path
    }));
}

#[test]
fn package_audit_applies_global_pax_linkpath_to_following_links() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let global_link_target = "../.env";
    let header_link_target = "bin/tokenzero";
    let pax_payload = pax_record("linkpath", global_link_target);

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new(member, b'2', b"").with_link_target(header_link_target),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == member
            && issue["link_target"] == global_link_target
    }));
}

#[test]
fn package_audit_fails_closed_on_duplicate_tar_member_names() {
    let dir = tempdir().unwrap();
    let tar_artifact = dir.path().join("release.tar");
    let gzip_artifact = dir.path().join("release.tar.gz");
    let member = "tokenzero-v0.1.1/LICENSE";

    write_test_tar_entries(
        &tar_artifact,
        &[
            TarTestEntry::new(member, b'0', b"first"),
            TarTestEntry::new(member, b'0', b"second"),
        ],
    );
    fs::write(
        &gzip_artifact,
        gzip_bytes(&fs::read(&tar_artifact).unwrap()),
    )
    .unwrap();

    let report = package_audit(dir.path(), &[tar_artifact.clone(), gzip_artifact.clone()]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    for artifact in [tar_artifact, gzip_artifact] {
        let artifact_path = artifact.display().to_string();
        assert!(
            issues.iter().any(|issue| {
                issue["code"] == "tar_duplicate_member_name"
                    && issue["path"] == artifact_path
                    && issue["member"] == member
            }),
            "missing duplicate tar member issue for {artifact_path}: {report:#}"
        );
    }
}

#[test]
fn package_audit_rejects_archive_member_path_escape() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let parent_member = "tokenzero-v0.1.1/../.env";
    let absolute_member = "/tmp/tokenzero/LICENSE";
    let windows_member = "C:/Users/example/.ssh/id_ed25519";

    write_test_tar(&artifact, &[parent_member, absolute_member, windows_member]);

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_path_escape"
            && issue["member"] == parent_member
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_path_escape"
            && issue["member"] == absolute_member
            && issue["reason"] == "absolute_path"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_path_escape"
            && issue["member"] == windows_member
            && issue["reason"] == "windows_drive_path"
    }));
}

#[test]
fn package_audit_rejects_tar_link_target_escape() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero";
    let hardlink_member = "tokenzero-v0.1.1/cache/recovery-cache.json";

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new(symlink_member, b'2', b"").with_link_target("../.env"),
            TarTestEntry::new(hardlink_member, b'1', b"")
                .with_link_target("/home/example/.tokenzero/recovery-cache.json"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == symlink_member
            && issue["link_kind"] == "symlink"
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == "../.env"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == hardlink_member
            && issue["link_kind"] == "hardlink"
            && issue["reason"] == "absolute_path"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_link_target"
            && issue["member"] == hardlink_member
            && issue["link_target"] == "/home/example/.tokenzero/recovery-cache.json"
    }));
}

#[test]
fn package_audit_rejects_private_dotdir_directory_leaf_members() {
    let dir = tempdir().unwrap();
    let tar_artifact = dir.path().join("release.tar");
    let zip_artifact = dir.path().join("release.zip");
    let tar_private_dir = "tokenzero-v0.1.1/.tokenzero";
    let zip_private_dir = "tokenzero-v0.1.1/.cursor/";

    write_test_tar_entries(
        &tar_artifact,
        &[TarTestEntry::new(tar_private_dir, b'5', b"")],
    );
    write_test_zip(&zip_artifact, &[ZipTestEntry::file(zip_private_dir, b"")]);

    let report = package_audit(dir.path(), &[tar_artifact, zip_artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == tar_private_dir
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == zip_private_dir
    }));
}

#[test]
fn package_audit_rejects_private_dotdir_link_target_leaf() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let symlink_member = "tokenzero-v0.1.1/config-link";

    write_test_tar_entries(
        &artifact,
        &[TarTestEntry::new(symlink_member, b'2', b"").with_link_target(".tokenzero")],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == ".tokenzero"
    }));
}

#[test]
fn package_audit_rejects_pax_global_private_metadata() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/.tokenzero/config.json";
    let global_linkpath = "../.env";
    let mut pax_payload = pax_record("path", global_path);
    pax_payload.extend_from_slice(&pax_record("linkpath", global_linkpath));

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
            TarTestEntry::new("tokenzero-v0.1.1/config.json", b'0', b"{}"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == global_path
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == "./GlobalHead.0"
            && issue["link_target"] == global_linkpath
            && issue["reason"] == "parent_directory"
    }));
}

#[test]
fn package_audit_rejects_pax_and_gnu_link_targets() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let pax_member = "tokenzero-v0.1.1/config";
    let pax_target = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax_payload = pax_record("linkpath", pax_target);
    let gnu_member = "tokenzero-v0.1.1/ssh-key";
    let gnu_target = format!("../{}/id_ed25519", "private".repeat(20));
    let gnu_target_payload = format!("{gnu_target}\0");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config", b'x', &pax_payload),
            TarTestEntry::new(pax_member, b'2', b""),
            TarTestEntry::new("././@LongLink", b'K', gnu_target_payload.as_bytes()),
            TarTestEntry::new(gnu_member, b'2', b""),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_link_target"
            && issue["member"] == pax_member
            && issue["link_target"] == pax_target
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == gnu_member
            && issue["link_target"] == gnu_target
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_link_target"
            && issue["member"] == gnu_member
            && issue["link_target"] == gnu_target
    }));
}

#[test]
fn package_audit_fails_closed_on_conflicting_tar_name_overrides() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let safe_long_member = "tokenzero-v0.1.1/config.json";
    let private_pax_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax_payload = pax_record("path", private_pax_member);
    let long_payload = format!("{safe_long_member}\0");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config.json", b'x', &pax_payload),
            TarTestEntry::new("././@LongLink", b'L', long_payload.as_bytes()),
            TarTestEntry::new("config.json", b'0', b""),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == private_pax_member
    }));
}

#[test]
fn package_audit_fails_closed_on_conflicting_tar_link_overrides() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let symlink_member = "tokenzero-v0.1.1/config-link";
    let safe_long_target = "tokenzero-v0.1.1/config.json";
    let private_pax_target = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax_payload = pax_record("linkpath", private_pax_target);
    let long_payload = format!("{safe_long_target}\0");

    write_test_tar_entries(
        &artifact,
        &[
            TarTestEntry::new("./PaxHeaders.0/config-link", b'x', &pax_payload),
            TarTestEntry::new("././@LongLink", b'K', long_payload.as_bytes()),
            TarTestEntry::new(symlink_member, b'2', b"").with_link_target("config.json"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == private_pax_target
    }));
}

#[test]
fn package_audit_rejects_zip_symlink_target_escape() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, b"../.env")],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == symlink_member
            && issue["link_kind"] == "symlink"
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == "../.env"
    }));
}

#[test]
fn package_audit_fails_closed_on_unreadable_zip_symlink_target() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/config-link";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, b"not-deflated").with_method(8)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_symlink_target_unreadable"
            && issue["member"] == symlink_member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("deflate"))
    }));
}

#[test]
fn package_audit_rejects_deflated_zip_symlink_target_escape() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero";
    let compressed_target = deflate_bytes(b"../.env");

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, &compressed_target).with_method(8)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == symlink_member
            && issue["link_kind"] == "symlink"
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == "../.env"
    }));
}

#[test]
fn package_audit_reads_zip_symlink_target_with_data_descriptor() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, b"../.env").with_data_descriptor()],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == symlink_member
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == "../.env"
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_data_descriptor_crc_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let target = b"bin/tokenzero";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, target).with_data_descriptor()],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let local_header = zip_local_header(&bytes, 0)
        .unwrap_or_else(|error| panic!("{}", zip_payload_error_detail(error)));
    let descriptor_crc_offset = local_header.data_start + target.len() + 4;
    let wrong_crc = zip_crc32(target) ^ u32::MAX;
    set_zip_u32_at(&mut bytes, descriptor_crc_offset, wrong_crc);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_data_descriptor_mismatch"
            && issue["member"] == symlink_member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("CRC"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_data_descriptor_local_size_disagreement() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let target = b"bin/tokenzero";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, target).with_data_descriptor()],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let wrong_size = u32::try_from(target.len() + 1).unwrap();
    set_zip_u32_at(&mut bytes, 18, wrong_size);
    set_zip_u32_at(&mut bytes, 22, wrong_size);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_local_header_metadata_mismatch"
            && issue["member"] == symlink_member
            && issue["field"] == "data_descriptor_sizes"
            && issue["central_compressed_size"] == target.len()
            && issue["local_compressed_size"] == wrong_size
            && issue["central_uncompressed_size"] == target.len()
            && issue["local_uncompressed_size"] == wrong_size
    }));
}

#[test]
fn package_audit_fails_closed_on_zip64_data_descriptor_size_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/config-link";
    let target = b"config.json";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, target)
            .with_data_descriptor()
            .with_zip64_extra_fields()],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    let name_len = zip_u16_at(&bytes, central_directory_offset + 28).unwrap() as usize;
    let zip64_extra_offset = central_directory_offset + 46 + name_len;
    assert_eq!(
        zip_u16_at(&bytes, zip64_extra_offset).unwrap(),
        ZIP64_EXTENDED_INFORMATION_EXTRA
    );
    set_zip_u64_at(
        &mut bytes,
        zip64_extra_offset + 4,
        u32::MAX as u64 + 1 + target.len() as u64,
    );
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_data_descriptor_mismatch"
            && issue["member"] == symlink_member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("zip64 descriptor"))
    }));
}

#[test]
fn package_audit_reads_zip_symlink_target_with_unsigned_data_descriptor() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/bin/tokenzero";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(symlink_member, b"../.env").with_unsigned_data_descriptor()],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_link_target_escape"
            && issue["member"] == symlink_member
            && issue["reason"] == "parent_directory"
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "sensitive_link_target"
            && issue["member"] == symlink_member
            && issue["link_target"] == "../.env"
    }));
    assert!(
        !issues
            .iter()
            .any(|issue| issue["code"] == "zip_data_descriptor_mismatch")
    );
}

#[test]
fn package_audit_fails_closed_on_zip_stored_size_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, b"MIT")]);

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    set_zip_u32_at(&mut bytes, 22, 4);
    set_zip_u32_at(&mut bytes, central_directory_offset + 24, 4);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(
        issues.iter().any(|issue| {
            issue["code"] == "zip_entry_size_mismatch" && issue["member"] == member
        })
    );
}

#[test]
fn package_audit_fails_closed_on_zip_symlink_payload_size_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/config-link";
    let target = b"config.json";
    write_test_zip(&artifact, &[ZipTestEntry::symlink(symlink_member, target)]);

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    set_zip_u32_at(&mut bytes, 22, target.len() as u32 + 1);
    set_zip_u32_at(
        &mut bytes,
        central_directory_offset + 24,
        target.len() as u32 + 1,
    );
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_symlink_target_unreadable"
            && issue["member"] == symlink_member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("uncompressed size mismatch"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_payload_overlap_with_central_directory() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, b"MIT")]);

    let mut bytes = fs::read(&artifact).unwrap();
    let local_header = zip_local_header(&bytes, 0)
        .unwrap_or_else(|error| panic!("{}", zip_payload_error_detail(error)));
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    let overlapping_size = central_directory_offset - local_header.data_start + 1;
    set_zip_u32_at(&mut bytes, 18, overlapping_size as u32);
    set_zip_u32_at(&mut bytes, 22, overlapping_size as u32);
    set_zip_u32_at(
        &mut bytes,
        central_directory_offset + 20,
        overlapping_size as u32,
    );
    set_zip_u32_at(
        &mut bytes,
        central_directory_offset + 24,
        overlapping_size as u32,
    );
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_local_record_overlap"
            && issue["member"] == member
            && issue["field"] == "central_directory"
    }));
}

#[test]
fn package_audit_fails_closed_on_overlapping_zip_local_records() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let first = "tokenzero-v0.1.1/LICENSE";
    let second = "tokenzero-v0.1.1/NOTICE";
    write_test_zip(
        &artifact,
        &[
            ZipTestEntry::file(first, b"first"),
            ZipTestEntry::file(second, b"second"),
        ],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let first_header = zip_local_header(&bytes, 0)
        .unwrap_or_else(|error| panic!("{}", zip_payload_error_detail(error)));
    let second_header_offset = first_header.data_start + b"first".len();
    let overlapping_size = second_header_offset - first_header.data_start + 1;
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    set_zip_u32_at(&mut bytes, 18, overlapping_size as u32);
    set_zip_u32_at(&mut bytes, 22, overlapping_size as u32);
    set_zip_u32_at(
        &mut bytes,
        central_directory_offset + 20,
        overlapping_size as u32,
    );
    set_zip_u32_at(
        &mut bytes,
        central_directory_offset + 24,
        overlapping_size as u32,
    );
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_local_record_overlap"
            && issue["member"] == first
            && issue["field"] == "local_record"
            && issue["next_member"] == second
    }));
}

#[test]
fn package_audit_fails_closed_on_missing_zip_data_descriptor_before_central_directory() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let target = b"bin/tokenzero";
    write_test_zip(
        &artifact,
        &[ZipTestEntry::symlink(member, target).with_data_descriptor()],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let local_header = zip_local_header(&bytes, 0)
        .unwrap_or_else(|error| panic!("{}", zip_payload_error_detail(error)));
    let descriptor_start = local_header.data_start + target.len();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    assert_eq!(central_directory_offset - descriptor_start, 16);
    bytes.drain(descriptor_start..central_directory_offset);
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u32_at(&mut bytes, eocd_offset + 16, descriptor_start as u32);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_data_descriptor_mismatch"
            && issue["member"] == member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("before the central directory"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_symlink_crc_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let symlink_member = "tokenzero-v0.1.1/config-link";
    let target = b"config.json";

    write_test_zip(&artifact, &[ZipTestEntry::symlink(symlink_member, target)]);

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    let wrong_crc = zip_crc32(target) ^ u32::MAX;
    set_zip_u32_at(&mut bytes, 14, wrong_crc);
    set_zip_u32_at(&mut bytes, central_directory_offset + 16, wrong_crc);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_symlink_target_unreadable"
            && issue["member"] == symlink_member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("CRC mismatch"))
    }));
}

#[test]
fn package_audit_recurses_into_nested_archives() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let outer_member = "tokenzero-v0.1.1/artifacts/inner.tar";

    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_zip(&artifact, &[ZipTestEntry::file(outer_member, &inner_bytes)]);

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member"
            && issue["path"]
                .as_str()
                .is_some_and(|path| path.contains("release.zip!") && path.contains(outer_member))
            && issue["member"] == nested_member
    }));
}

#[test]
fn package_audit_recurses_into_deflated_nested_zip_archives() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let outer_member = "tokenzero-v0.1.1/artifacts/inner.tar";

    write_test_tar(&inner, &[nested_member]);
    let compressed_inner = deflate_bytes(&fs::read(&inner).unwrap());
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(outer_member, &compressed_inner).with_method(8)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member"
            && issue["path"]
                .as_str()
                .is_some_and(|path| path.contains("release.zip!") && path.contains(outer_member))
            && issue["member"] == nested_member
    }));
}

#[test]
fn package_audit_fails_closed_on_nested_zip_archive_crc_mismatch() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    let outer_member = "tokenzero-v0.1.1/artifacts/inner.tar";

    write_test_tar(&inner, &["tokenzero-v0.1.1/LICENSE"]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_zip(&artifact, &[ZipTestEntry::file(outer_member, &inner_bytes)]);

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    let wrong_crc = zip_crc32(&inner_bytes) ^ u32::MAX;
    set_zip_u32_at(&mut bytes, 14, wrong_crc);
    set_zip_u32_at(&mut bytes, central_directory_offset + 16, wrong_crc);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "nested_archive_member_unreadable"
            && issue["member"] == outer_member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("CRC mismatch"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_local_header_name_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let central_member = "tokenzero-v0.1.1/config.json";
    let local_member = "tokenzero-v0.1.1/.tokenzero/config.json";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(central_member, b"{}").with_local_name(local_member)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_local_header_name_mismatch"
            && issue["member"] == central_member
            && issue["local_member"] == local_member
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == local_member
    }));
}

#[test]
fn package_audit_rejects_zip_central_unicode_path_extra_private_member() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let visible_member = "tokenzero-v0.1.1/config.json";
    let unicode_member = "tokenzero-v0.1.1/.tokenzero/config.json";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(visible_member, b"{}").with_central_unicode_path(unicode_member)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == unicode_member
    }));
}

#[test]
fn package_audit_rejects_zip_local_unicode_path_extra_private_member() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let visible_member = "tokenzero-v0.1.1/config.json";
    let unicode_member = "tokenzero-v0.1.1/.tokenzero/config.json";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(visible_member, b"{}").with_local_unicode_path(unicode_member)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member" && issue["member"] == unicode_member
    }));
}

#[test]
fn package_audit_fails_closed_on_conflicting_zip_unicode_path_extra_fields() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let visible_member = "tokenzero-v0.1.1/config.json";
    let central_unicode_member = "tokenzero-v0.1.1/config-central.json";
    let local_unicode_member = "tokenzero-v0.1.1/config-local.json";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(visible_member, b"{}")
            .with_central_unicode_path(central_unicode_member)
            .with_local_unicode_path(local_unicode_member)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_local_header_metadata_mismatch"
            && issue["member"] == visible_member
            && issue["field"] == "unicode_path"
            && issue["central"] == central_unicode_member
            && issue["local"] == local_unicode_member
    }));
}

#[test]
fn package_audit_fails_closed_on_malformed_zip_unicode_path_extra() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let visible_member = "tokenzero-v0.1.1/config.json";
    let malformed_unicode_path = vec![0x75, 0x70, 0x05, 0x00, 1, 0, 0, 0, 0];

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(visible_member, b"{}").with_central_extra(malformed_unicode_path)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("unicode path extra field"))
    }));
}

#[test]
fn package_audit_recurses_into_zip_unicode_path_extra_nested_archive() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    let visible_member = "tokenzero-v0.1.1/artifacts/payload.bin";
    let unicode_member = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";

    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(visible_member, &inner_bytes)
            .with_central_unicode_path(unicode_member)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member"
            && issue["path"]
                .as_str()
                .is_some_and(|path| path.contains("release.zip!") && path.contains(unicode_member))
            && issue["member"] == nested_member
    }));
}

#[test]
fn package_audit_rejects_zip_unicode_path_extra_dotdir_directory() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let visible_member = "tokenzero-v0.1.1/metadata";
    let unicode_member = "tokenzero-v0.1.1/.idea/";

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(visible_member, b"").with_central_unicode_path(unicode_member)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "non_public_dotdir_member" && issue["member"] == unicode_member
    }));
}

#[test]
fn package_audit_fails_closed_on_split_zip_archive() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u16_at(&mut bytes, eocd_offset + 4, 1);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("multi-disk"))
    }));
}

#[test]
fn package_audit_fails_closed_on_duplicate_zip_eocd_candidates() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(
            "tokenzero-v0.1.1/.tokenzero/config.json",
            b"{}",
        )],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let original_eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u16_at(&mut bytes, original_eocd_offset + 20, 22);
    push_zip_u32(&mut bytes, 0x0605_4b50);
    push_zip_u16(&mut bytes, 0);
    push_zip_u16(&mut bytes, 0);
    push_zip_u16(&mut bytes, 0);
    push_zip_u16(&mut bytes, 0);
    push_zip_u32(&mut bytes, 0);
    push_zip_u32(&mut bytes, 0);
    push_zip_u16(&mut bytes, 0);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("plausible end-of-central-directory"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_central_directory_inside_eocd_comment() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let bytes = fs::read(&artifact).unwrap();
    let original_eocd_offset = find_zip_eocd(&bytes).unwrap();
    let original_directory_offset = zip_u32_at(&bytes, original_eocd_offset + 16).unwrap() as usize;
    let central_directory = bytes[original_directory_offset..original_eocd_offset].to_vec();
    let mut reordered = bytes[..original_directory_offset].to_vec();
    let new_eocd_offset = reordered.len();
    push_zip_u32(&mut reordered, 0x0605_4b50);
    push_zip_u16(&mut reordered, 0);
    push_zip_u16(&mut reordered, 0);
    push_zip_u16(&mut reordered, 1);
    push_zip_u16(&mut reordered, 1);
    push_zip_u32(&mut reordered, central_directory.len() as u32);
    push_zip_u32(&mut reordered, (new_eocd_offset + 22) as u32);
    push_zip_u16(&mut reordered, central_directory.len() as u16);
    reordered.extend_from_slice(&central_directory);
    fs::write(&artifact, reordered).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("central directory overlaps or follows"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip64_entry_field_sentinel() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    set_zip_u32_at(&mut bytes, central_directory_offset + 42, u32::MAX);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("zip64 extended information"))
    }));
}

#[test]
fn package_audit_reads_zip64_entry_extra_fields() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT").with_zip64_extra_fields()],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["archives_checked"], 1);
    assert!(
        !issues
            .iter()
            .any(|issue| issue["code"] == "archive_member_listing_failed"),
        "{report:#}"
    );
}

#[test]
fn package_audit_recurses_into_zip64_nested_archive() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    let outer_member = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";

    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(outer_member, &inner_bytes).with_zip64_extra_fields()],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "private_tool_state_member"
            && issue["path"]
                .as_str()
                .is_some_and(|path| path.contains("release.zip!") && path.contains(outer_member))
            && issue["member"] == nested_member
    }));
}

#[test]
fn package_audit_fails_closed_on_duplicate_zip64_extra_field() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let mut duplicate_zip64 = zip64_extended_info_extra_bytes(&[3, 3, 0]);
    duplicate_zip64.extend_from_slice(&zip64_extended_info_extra_bytes(&[3, 3, 0]));

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")
            .with_central_extra(duplicate_zip64)],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    set_zip_u32_at(&mut bytes, central_directory_offset + 20, u32::MAX);
    set_zip_u32_at(&mut bytes, central_directory_offset + 24, u32::MAX);
    set_zip_u32_at(&mut bytes, central_directory_offset + 42, u32::MAX);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("duplicated"))
    }));
}

#[test]
fn package_audit_fails_closed_on_duplicate_unhandled_zip_extra_fields() {
    let dir = tempdir().unwrap();
    let central_artifact = dir.path().join("central-extra-duplicate.zip");
    let local_artifact = dir.path().join("local-extra-duplicate.zip");
    let mut duplicate_extra = zip_extra_field_bytes(0x5455, &[1, 0, 0, 0, 0]);
    duplicate_extra.extend_from_slice(&zip_extra_field_bytes(0x5455, &[1, 0, 0, 0, 0]));

    write_test_zip(
        &central_artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")
            .with_central_extra(duplicate_extra.clone())],
    );
    write_test_zip(
        &local_artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/NOTICE", b"MIT").with_local_extra(duplicate_extra)],
    );

    let report = package_audit(dir.path(), &[central_artifact, local_artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    let duplicate_extra_issues = issues
        .iter()
        .filter(|issue| {
            issue["code"] == "archive_member_listing_failed"
                && issue["detail"].as_str().is_some_and(|detail| {
                    detail.contains("0x5455") && detail.contains("duplicated")
                })
        })
        .count();
    assert_eq!(
        duplicate_extra_issues, 2,
        "expected both central and local duplicate extra fields to fail closed: {report:#}"
    );
}

#[test]
fn package_audit_fails_closed_on_zip64_surplus_sentinel_fields() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    let zip64_with_surplus = zip64_extended_info_extra_bytes(&[3, 3, 0, 42]);

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, b"MIT").with_central_extra(zip64_with_surplus)],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    set_zip_u32_at(&mut bytes, central_directory_offset + 20, u32::MAX);
    set_zip_u32_at(&mut bytes, central_directory_offset + 24, u32::MAX);
    set_zip_u32_at(&mut bytes, central_directory_offset + 42, u32::MAX);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"].as_str().is_some_and(|detail| {
                detail.contains("zip64") && detail.contains("unclaimed bytes")
            })
    }));
}

#[test]
fn package_audit_fails_closed_on_zip64_directory_offset_sentinel() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u32_at(&mut bytes, eocd_offset + 16, u32::MAX);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("zip64"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip64_locator_offset_overflow() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    append_zip64_eocd(&mut bytes);
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let locator_offset = eocd_offset - 20;
    set_zip_u16_at(&mut bytes, eocd_offset + 8, u16::MAX);
    set_zip_u16_at(&mut bytes, eocd_offset + 10, u16::MAX);
    set_zip_u32_at(&mut bytes, eocd_offset + 12, u32::MAX);
    set_zip_u32_at(&mut bytes, eocd_offset + 16, u32::MAX);
    set_zip_u64_at(&mut bytes, locator_offset + 8, u64::MAX);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"].as_str().is_some_and(|detail| {
                detail.contains("zip64 end-of-central-directory")
                    && (detail.contains("overflowed")
                        || detail.contains("too large")
                        || detail.contains("outside"))
            })
    }));
}

#[test]
fn package_audit_reads_zip64_end_of_central_directory() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    append_zip64_eocd(&mut bytes);
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u16_at(&mut bytes, eocd_offset + 8, u16::MAX);
    set_zip_u16_at(&mut bytes, eocd_offset + 10, u16::MAX);
    set_zip_u32_at(&mut bytes, eocd_offset + 12, u32::MAX);
    set_zip_u32_at(&mut bytes, eocd_offset + 16, u32::MAX);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["archives_checked"], 1);
    assert!(
        !issues
            .iter()
            .any(|issue| issue["code"] == "archive_member_listing_failed"),
        "{report:#}"
    );
}

#[test]
fn package_audit_fails_closed_on_encrypted_zip_entry_flag() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, b"MIT").with_flags(ZIP_FLAG_ENCRYPTED)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_entry_uninspectable"
            && issue["member"] == member
            && issue["flags"]
                .as_array()
                .unwrap()
                .iter()
                .any(|flag| flag == "encrypted")
    }));
}

#[test]
fn package_audit_fails_closed_on_unsupported_zip_executable_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, b"opaque").with_method(12)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_regular_file_uninspectable"
            && issue["member"] == member
            && issue["compression_method"] == 12
    }));
}

#[test]
fn package_audit_fails_closed_on_unsupported_zip_native_addon_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/node_modules/addon/build/Release/addon.node";
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, b"opaque").with_method(12)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_regular_file_uninspectable"
            && issue["member"] == member
            && issue["compression_method"] == 12
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_executable_payload_crc_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let payload = b"#!/bin/sh\nexec tokenzero-runtime \"$@\"\n";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, payload)]);

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    let wrong_crc = zip_crc32(payload) ^ u32::MAX;
    set_zip_u32_at(&mut bytes, 14, wrong_crc);
    set_zip_u32_at(&mut bytes, central_directory_offset + 16, wrong_crc);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_regular_file_uninspectable"
            && issue["member"] == member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("CRC mismatch"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_regular_member_crc_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    let payload = b"MIT";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, payload)]);

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    let wrong_crc = zip_crc32(payload) ^ u32::MAX;
    set_zip_u32_at(&mut bytes, 14, wrong_crc);
    set_zip_u32_at(&mut bytes, central_directory_offset + 16, wrong_crc);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_entry_payload_integrity_mismatch"
            && issue["member"] == member
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("CRC mismatch"))
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_aggregate_payload_budget() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let first = "tokenzero-v0.1.1/share/nested-one.zip";
    let second = "tokenzero-v0.1.1/share/nested-two.zip";
    let payload = deflate_bytes(b"tiny");
    write_test_zip(
        &artifact,
        &[
            ZipTestEntry::file(first, &payload).with_method(8),
            ZipTestEntry::file(second, &payload).with_method(8),
        ],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let advertised_size = u32::try_from(MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES / 2 + 1).unwrap();
    set_test_zip_entry_uncompressed_sizes(&mut bytes, &[advertised_size, advertised_size]);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_total_payload_size_exceeded"
            && issue["member"] == second
            && issue["limit"].as_u64() == Some(MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES as u64)
    }));
}

#[test]
fn package_audit_fails_closed_on_oversized_top_level_archives() {
    let dir = tempdir().unwrap();
    let artifacts: Vec<PathBuf> = [
        "release.zip",
        "release.tar",
        "release.tar.gz",
        "release.tgz",
        "tokenzero.crate",
    ]
    .iter()
    .map(|name| dir.path().join(name))
    .collect();

    for artifact in &artifacts {
        fs::File::create(artifact)
            .unwrap()
            .set_len(MAX_TOP_LEVEL_ARCHIVE_BYTES + 1)
            .unwrap();
    }

    let report = package_audit(dir.path(), &artifacts);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(
        !issues
            .iter()
            .any(|issue| issue["code"] == "archive_member_listing_failed"),
        "{report:#}"
    );
    for artifact in &artifacts {
        let artifact_path = artifact.display().to_string();
        assert!(
            issues.iter().any(|issue| {
                issue["code"] == "archive_file_too_large"
                    && issue["path"] == artifact_path
                    && issue["limit"].as_u64() == Some(MAX_TOP_LEVEL_ARCHIVE_BYTES)
            }),
            "missing archive_file_too_large for {artifact_path}: {report:#}"
        );
    }
}

#[test]
fn package_audit_fails_closed_on_zip_directory_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/docs/";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, b"hidden")]);

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_directory_payload_present" && issue["member"] == member
    }));
}

#[test]
fn package_audit_fails_closed_on_tar_directory_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    let member = "tokenzero-v0.1.1/docs/";
    write_test_tar_entries(&artifact, &[TarTestEntry::new(member, b'5', b"hidden")]);

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "tar_directory_payload_present" && issue["member"] == member
    }));
}

#[test]
fn package_audit_accepts_deflated_zip_executable_payload() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let payload = deflate_bytes(b"#!/bin/sh\nexec tokenzero-runtime \"$@\"\n");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, &payload).with_method(8)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["archives_checked"], 1);
    assert!(
        !issues
            .iter()
            .any(|issue| issue["code"] == "zip_regular_file_uninspectable"),
        "{report:#}"
    );
}

#[test]
fn package_audit_fails_closed_on_zip_entry_comment() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, b"MIT").with_comment(b"/tmp/example/release")],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_entry_comment_present"
            && issue["member"] == member
            && issue["comment_bytes"] == 20
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_archive_comment() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let archive_comment = b"/tmp/example/release";
    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u16_at(&mut bytes, eocd_offset + 20, archive_comment.len() as u16);
    bytes.extend_from_slice(archive_comment);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_archive_comment_present"
            && issue["comment_bytes"] == archive_comment.len()
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_extra_fields() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let central_member = "tokenzero-v0.1.1/LICENSE";
    let local_member = "tokenzero-v0.1.1/NOTICE";
    let central_extra = zip_extra_field_bytes(0x5455, b"\x01\x00\x00\x00\x00");
    let local_extra = zip_extra_field_bytes(0x7875, b"\x01\x01\xed\x01\x14");

    write_test_zip(
        &artifact,
        &[
            ZipTestEntry::file(central_member, b"MIT").with_central_extra(central_extra),
            ZipTestEntry::file(local_member, b"notice").with_local_extra(local_extra),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_extra_field_present"
            && issue["member"] == central_member
            && issue["field_location"] == "central"
            && issue["tag"] == "0x5455"
            && issue["size"] == 5
    }));
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_extra_field_present"
            && issue["member"] == local_member
            && issue["field_location"] == "local"
            && issue["tag"] == "0x7875"
            && issue["size"] == 5
    }));
}

#[test]
fn package_audit_fails_closed_on_unneeded_zip64_extra_field() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    let zip64_without_sentinel = zip64_extended_info_extra_bytes(&[3, 3, 0]);

    write_test_zip(
        &artifact,
        &[ZipTestEntry::file(member, b"MIT").with_central_extra(zip64_without_sentinel)],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_extra_field_present"
            && issue["member"] == member
            && issue["field_location"] == "central"
            && issue["tag"] == "0x0001"
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_leading_unclaimed_bytes() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, b"MIT")]);

    let preamble = b"#!/bin/sh\nexec /tmp/hidden\n";
    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    bytes.splice(0..0, preamble.iter().copied());
    let new_central_directory_offset = central_directory_offset + preamble.len();
    let new_eocd_offset = eocd_offset + preamble.len();
    set_zip_u32_at(
        &mut bytes,
        new_eocd_offset + 16,
        new_central_directory_offset as u32,
    );
    set_zip_u32_at(
        &mut bytes,
        new_central_directory_offset + 42,
        preamble.len() as u32,
    );
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_unclaimed_local_bytes"
            && issue["start"] == 0
            && issue["end"] == preamble.len()
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_gap_before_central_directory() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
    );

    let gap = b"raw_traces/local_only";
    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
    bytes.splice(
        central_directory_offset..central_directory_offset,
        gap.iter().copied(),
    );
    let new_central_directory_offset = central_directory_offset + gap.len();
    let new_eocd_offset = eocd_offset + gap.len();
    set_zip_u32_at(
        &mut bytes,
        new_eocd_offset + 16,
        new_central_directory_offset as u32,
    );
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_unclaimed_local_bytes"
            && issue["start"] == central_directory_offset
            && issue["end"] == new_central_directory_offset
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_local_header_method_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(&artifact, &[ZipTestEntry::file(member, b"MIT")]);

    let mut bytes = fs::read(&artifact).unwrap();
    set_zip_u16_at(&mut bytes, 8, 8);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_local_header_metadata_mismatch"
            && issue["member"] == member
            && issue["field"] == "compression_method"
            && issue["central"] == 0
            && issue["local"] == 8
    }));
}

#[test]
fn package_audit_fails_closed_on_zip_central_directory_count_mismatch() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(
        &artifact,
        &[
            ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT"),
            ZipTestEntry::file("tokenzero-v0.1.1/.tokenzero/config.json", b"{}"),
        ],
    );

    let mut bytes = fs::read(&artifact).unwrap();
    let eocd_offset = find_zip_eocd(&bytes).unwrap();
    set_zip_u16_at(&mut bytes, eocd_offset + 8, 1);
    set_zip_u16_at(&mut bytes, eocd_offset + 10, 1);
    fs::write(&artifact, bytes).unwrap();

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "archive_member_listing_failed"
            && issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("unparsed bytes"))
    }));
}

#[test]
fn package_audit_fails_closed_on_duplicate_zip_member_names() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_zip(
        &artifact,
        &[
            ZipTestEntry::file(member, b"first"),
            ZipTestEntry::file(member, b"second"),
        ],
    );

    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap();

    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|issue| {
        issue["code"] == "zip_duplicate_member_name" && issue["member"] == member
    }));
}

#[test]
fn package_audit_malformed_zip_corpus_has_stable_listing_failures() {
    struct MalformedZipCase {
        name: &'static str,
        build: fn(&Path),
        detail_contains: &'static str,
    }

    fn write_missing_eocd(path: &Path) {
        write_test_zip(
            path,
            &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
        );
        let mut bytes = fs::read(path).unwrap();
        let eocd_offset = find_zip_eocd(&bytes).unwrap();
        bytes.truncate(eocd_offset);
        fs::write(path, bytes).unwrap();
    }

    fn write_invalid_central_signature(path: &Path) {
        write_test_zip(
            path,
            &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
        );
        let mut bytes = fs::read(path).unwrap();
        let eocd_offset = find_zip_eocd(&bytes).unwrap();
        let central_directory_offset = zip_u32_at(&bytes, eocd_offset + 16).unwrap() as usize;
        set_zip_u32_at(&mut bytes, central_directory_offset, 0);
        fs::write(path, bytes).unwrap();
    }

    fn write_invalid_local_signature(path: &Path) {
        write_test_zip(
            path,
            &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")],
        );
        let mut bytes = fs::read(path).unwrap();
        set_zip_u32_at(&mut bytes, 0, 0);
        fs::write(path, bytes).unwrap();
    }

    fn write_truncated_extra_field(path: &Path) {
        write_test_zip(
            path,
            &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")
                .with_central_extra(vec![0x55, 0x54, 0x01])],
        );
    }

    let dir = tempdir().unwrap();
    let cases = [
        MalformedZipCase {
            name: "missing-eocd.zip",
            build: write_missing_eocd,
            detail_contains: "end-of-central-directory record was not found",
        },
        MalformedZipCase {
            name: "invalid-central-signature.zip",
            build: write_invalid_central_signature,
            detail_contains: "central directory entry has an invalid signature",
        },
        MalformedZipCase {
            name: "invalid-local-signature.zip",
            build: write_invalid_local_signature,
            detail_contains: "local header has an invalid signature",
        },
        MalformedZipCase {
            name: "truncated-extra-field.zip",
            build: write_truncated_extra_field,
            detail_contains: "extra field header is truncated",
        },
    ];

    for case in cases {
        let artifact = dir.path().join(case.name);
        (case.build)(&artifact);

        let report = package_audit(dir.path(), &[artifact]);
        let issues = report["issues"].as_array().unwrap();

        assert_eq!(report["ok"], false, "case {}: {report:#}", case.name);
        assert!(
            issues.iter().any(|issue| {
                issue["code"] == "archive_member_listing_failed"
                    && issue["detail"]
                        .as_str()
                        .is_some_and(|detail| detail.contains(case.detail_contains))
            }),
            "case {} missing stable listing failure {:#}",
            case.name,
            report
        );
    }
}

struct TarTestEntry<'a> {
    name: &'a str,
    typeflag: u8,
    data: &'a [u8],
    link_target: Option<&'a str>,
}

impl<'a> TarTestEntry<'a> {
    fn new(name: &'a str, typeflag: u8, data: &'a [u8]) -> Self {
        Self {
            name,
            typeflag,
            data,
            link_target: None,
        }
    }

    fn with_link_target(mut self, link_target: &'a str) -> Self {
        self.link_target = Some(link_target);
        self
    }
}

fn write_test_tar(path: &Path, names: &[&str]) {
    let entries: Vec<_> = names
        .iter()
        .map(|name| TarTestEntry::new(name, b'0', b""))
        .collect();
    write_test_tar_entries(path, &entries);
}

fn write_test_tar_entries(path: &Path, entries: &[TarTestEntry<'_>]) {
    let mut file = fs::File::create(path).unwrap();
    for entry in entries {
        file.write_all(&test_tar_entry_bytes_with_type(
            entry.name,
            entry.typeflag,
            entry.data,
            entry.link_target,
        ))
        .unwrap();
    }
    file.write_all(&[0u8; 1024]).unwrap();
}

fn test_tar_entry_bytes(name: &str, data: &[u8]) -> Vec<u8> {
    test_tar_entry_bytes_with_type(name, b'0', data, None)
}

fn test_tar_entry_bytes_with_type(
    name: &str,
    typeflag: u8,
    data: &[u8],
    link_target: Option<&str>,
) -> Vec<u8> {
    let header = test_tar_header(name, typeflag, data.len() as u64, link_target);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(data);
    let padding = (512 - (data.len() % 512)) % 512;
    if padding > 0 {
        bytes.extend_from_slice(&vec![0u8; padding]);
    }
    bytes
}

fn test_tar_header(name: &str, typeflag: u8, size: u64, link_target: Option<&str>) -> [u8; 512] {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    assert!(name_bytes.len() < 100);
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    if let Some(link_target) = link_target {
        let link_bytes = link_target.as_bytes();
        assert!(link_bytes.len() < 100);
        header[157..157 + link_bytes.len()].copy_from_slice(link_bytes);
    }
    write_tar_octal(&mut header[100..108], 0o644);
    write_tar_octal(&mut header[108..116], 0);
    write_tar_octal(&mut header[116..124], 0);
    write_tar_octal(&mut header[124..136], size);
    write_tar_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = typeflag;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    write_test_tar_checksum(&mut header);
    header
}

fn write_test_tar_checksum(header: &mut [u8; 512]) {
    write_test_tar_checksum_bytes(header);
}

fn write_test_tar_checksum_bytes(header: &mut [u8]) {
    assert_eq!(header.len(), 512);
    header[148..156].fill(b' ');
    let checksum: u32 = header.iter().map(|byte| *byte as u32).sum();
    let checksum_text = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_text.as_bytes());
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let body = format!("{key}={value}\n");
    let mut length = body.len() + 2;
    loop {
        let record = format!("{length} {body}");
        if record.len() == length {
            return record.into_bytes();
        }
        length = record.len();
    }
}

fn write_tar_octal(field: &mut [u8], value: u64) {
    field.fill(0);
    let text = format!("{value:0width$o}", width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

fn write_tar_base256(field: &mut [u8], value: u128) {
    field.fill(0);
    let mut remaining = value;
    for byte in field.iter_mut().rev() {
        *byte = remaining as u8;
        remaining >>= 8;
    }
    assert_eq!(remaining, 0, "test tar base-256 value does not fit");
    field[0] |= 0x80;
}

struct ZipTestEntry<'a> {
    name: &'a str,
    data: &'a [u8],
    method: u16,
    version_made_by: u16,
    external_attrs: u32,
    local_name: Option<&'a str>,
    flags: u16,
    data_descriptor: bool,
    data_descriptor_signature: bool,
    force_zip64: bool,
    local_extra: Vec<u8>,
    central_extra: Vec<u8>,
    comment: Vec<u8>,
}

impl<'a> ZipTestEntry<'a> {
    fn file(name: &'a str, data: &'a [u8]) -> Self {
        Self {
            name,
            data,
            method: 0,
            version_made_by: (3 << 8) | 20,
            external_attrs: 0o100644 << 16,
            local_name: None,
            flags: 0,
            data_descriptor: false,
            data_descriptor_signature: true,
            force_zip64: false,
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            comment: Vec::new(),
        }
    }

    fn symlink(name: &'a str, target: &'a [u8]) -> Self {
        Self {
            name,
            data: target,
            method: 0,
            version_made_by: (3 << 8) | 20,
            external_attrs: 0o120777 << 16,
            local_name: None,
            flags: 0,
            data_descriptor: false,
            data_descriptor_signature: true,
            force_zip64: false,
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            comment: Vec::new(),
        }
    }

    fn with_method(mut self, method: u16) -> Self {
        self.method = method;
        self
    }

    fn with_flags(mut self, flags: u16) -> Self {
        self.flags |= flags;
        self
    }

    fn with_local_name(mut self, local_name: &'a str) -> Self {
        self.local_name = Some(local_name);
        self
    }

    fn with_data_descriptor(mut self) -> Self {
        self.flags |= ZIP_FLAG_DATA_DESCRIPTOR;
        self.data_descriptor = true;
        self
    }

    fn with_unsigned_data_descriptor(mut self) -> Self {
        self.flags |= ZIP_FLAG_DATA_DESCRIPTOR;
        self.data_descriptor = true;
        self.data_descriptor_signature = false;
        self
    }

    fn with_zip64_extra_fields(mut self) -> Self {
        self.force_zip64 = true;
        self
    }

    fn with_central_unicode_path(mut self, unicode_name: &str) -> Self {
        self.central_extra = zip_unicode_path_extra_bytes(self.name.as_bytes(), unicode_name);
        self
    }

    fn with_local_unicode_path(mut self, unicode_name: &str) -> Self {
        self.local_extra = zip_unicode_path_extra_bytes(self.local_name().as_bytes(), unicode_name);
        self
    }

    fn with_central_extra(mut self, extra: Vec<u8>) -> Self {
        self.central_extra = extra;
        self
    }

    fn with_local_extra(mut self, extra: Vec<u8>) -> Self {
        self.local_extra = extra;
        self
    }

    fn with_comment(mut self, comment: &[u8]) -> Self {
        self.comment = comment.to_vec();
        self
    }

    fn local_name(&self) -> &'a str {
        self.local_name.unwrap_or(self.name)
    }
}

fn write_test_zip(path: &Path, entries: &[ZipTestEntry<'_>]) {
    assert!(entries.len() <= u16::MAX as usize);
    let mut file_bytes = Vec::new();
    let mut central_directory = Vec::new();

    for entry in entries {
        let local_header_offset = file_bytes.len() as u32;
        write_zip_local_header(&mut file_bytes, entry);
        write_zip_central_header(&mut central_directory, entry, local_header_offset);
    }

    let central_directory_offset = file_bytes.len() as u32;
    let central_directory_size = central_directory.len() as u32;
    file_bytes.extend_from_slice(&central_directory);
    push_zip_u32(&mut file_bytes, 0x0605_4b50);
    push_zip_u16(&mut file_bytes, 0);
    push_zip_u16(&mut file_bytes, 0);
    push_zip_u16(&mut file_bytes, entries.len() as u16);
    push_zip_u16(&mut file_bytes, entries.len() as u16);
    push_zip_u32(&mut file_bytes, central_directory_size);
    push_zip_u32(&mut file_bytes, central_directory_offset);
    push_zip_u16(&mut file_bytes, 0);

    fs::write(path, file_bytes).unwrap();
}

fn append_zip64_eocd(bytes: &mut Vec<u8>) {
    let eocd_offset = find_zip_eocd(bytes).unwrap();
    let entry_count = zip_u16_at(bytes, eocd_offset + 10).unwrap() as u64;
    let central_directory_size = zip_u32_at(bytes, eocd_offset + 12).unwrap() as u64;
    let central_directory_offset = zip_u32_at(bytes, eocd_offset + 16).unwrap() as u64;
    let eocd = bytes.split_off(eocd_offset);
    let zip64_eocd_offset = bytes.len() as u64;

    push_zip_u32(bytes, ZIP64_EOCD_RECORD_SIGNATURE);
    push_zip_u64(bytes, 44);
    push_zip_u16(bytes, 45);
    push_zip_u16(bytes, 45);
    push_zip_u32(bytes, 0);
    push_zip_u32(bytes, 0);
    push_zip_u64(bytes, entry_count);
    push_zip_u64(bytes, entry_count);
    push_zip_u64(bytes, central_directory_size);
    push_zip_u64(bytes, central_directory_offset);

    push_zip_u32(bytes, ZIP64_EOCD_LOCATOR_SIGNATURE);
    push_zip_u32(bytes, 0);
    push_zip_u64(bytes, zip64_eocd_offset);
    push_zip_u32(bytes, 1);

    bytes.extend_from_slice(&eocd);
}

fn write_zip_local_header(out: &mut Vec<u8>, entry: &ZipTestEntry<'_>) {
    let local_extra = zip_local_extra(entry);
    let compressed_size = zip_test_entry_compressed_size(entry);
    let uncompressed_size = zip_test_entry_uncompressed_size(entry);
    push_zip_u32(out, 0x0403_4b50);
    push_zip_u16(out, 20);
    push_zip_u16(out, entry.flags);
    push_zip_u16(out, entry.method);
    push_zip_u16(out, 0);
    push_zip_u16(out, 0);
    push_zip_u32(out, zip_test_entry_crc32(entry));
    let local_compressed_size = if entry.data_descriptor {
        0
    } else if entry.force_zip64 {
        u32::MAX
    } else {
        zip_test_u32(compressed_size, "compressed size")
    };
    let local_uncompressed_size = if entry.data_descriptor {
        0
    } else if entry.force_zip64 {
        u32::MAX
    } else {
        zip_test_u32(uncompressed_size, "uncompressed size")
    };
    push_zip_u32(out, local_compressed_size);
    push_zip_u32(out, local_uncompressed_size);
    push_zip_u16(out, entry.local_name().len() as u16);
    push_zip_u16(out, local_extra.len() as u16);
    out.extend_from_slice(entry.local_name().as_bytes());
    out.extend_from_slice(&local_extra);
    out.extend_from_slice(entry.data);
    if entry.data_descriptor {
        if entry.data_descriptor_signature {
            push_zip_u32(out, ZIP_DATA_DESCRIPTOR_SIGNATURE);
        }
        push_zip_u32(out, zip_test_entry_crc32(entry));
        if entry.force_zip64 {
            push_zip_u64(out, compressed_size as u64);
            push_zip_u64(out, uncompressed_size as u64);
        } else {
            push_zip_u32(out, zip_test_u32(compressed_size, "compressed size"));
            push_zip_u32(out, zip_test_u32(uncompressed_size, "uncompressed size"));
        }
    }
}

fn write_zip_central_header(out: &mut Vec<u8>, entry: &ZipTestEntry<'_>, local_header_offset: u32) {
    let central_extra = zip_central_extra(entry, local_header_offset);
    let compressed_size = zip_test_entry_compressed_size(entry);
    let uncompressed_size = zip_test_entry_uncompressed_size(entry);
    push_zip_u32(out, 0x0201_4b50);
    push_zip_u16(out, entry.version_made_by);
    push_zip_u16(out, 20);
    push_zip_u16(out, entry.flags);
    push_zip_u16(out, entry.method);
    push_zip_u16(out, 0);
    push_zip_u16(out, 0);
    push_zip_u32(out, zip_test_entry_crc32(entry));
    if entry.force_zip64 {
        push_zip_u32(out, u32::MAX);
        push_zip_u32(out, u32::MAX);
    } else {
        push_zip_u32(out, zip_test_u32(compressed_size, "compressed size"));
        push_zip_u32(out, zip_test_u32(uncompressed_size, "uncompressed size"));
    }
    push_zip_u16(out, entry.name.len() as u16);
    push_zip_u16(out, central_extra.len() as u16);
    push_zip_u16(out, entry.comment.len() as u16);
    push_zip_u16(out, 0);
    push_zip_u16(out, 0);
    push_zip_u32(out, entry.external_attrs);
    if entry.force_zip64 {
        push_zip_u32(out, u32::MAX);
    } else {
        push_zip_u32(out, local_header_offset);
    }
    out.extend_from_slice(entry.name.as_bytes());
    out.extend_from_slice(&central_extra);
    out.extend_from_slice(&entry.comment);
}

fn zip_test_entry_crc32(entry: &ZipTestEntry<'_>) -> u32 {
    if entry.method == 8
        && let Ok(decompressed) = deflate_decompress_bytes(entry.data)
    {
        return zip_crc32(&decompressed);
    }
    zip_crc32(entry.data)
}

fn zip_test_entry_compressed_size(entry: &ZipTestEntry<'_>) -> usize {
    entry.data.len()
}

fn zip_test_entry_uncompressed_size(entry: &ZipTestEntry<'_>) -> usize {
    if entry.method == 8
        && let Ok(decompressed) = deflate_decompress_bytes(entry.data)
    {
        return decompressed.len();
    }
    entry.data.len()
}

fn zip_test_u32(value: usize, field: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| panic!("test zip {field} does not fit in u32"))
}

fn zip_local_extra(entry: &ZipTestEntry<'_>) -> Vec<u8> {
    let mut extra = entry.local_extra.clone();
    if entry.force_zip64 {
        extra.extend_from_slice(&zip64_extended_info_extra_bytes(&[
            zip_test_entry_uncompressed_size(entry) as u64,
            zip_test_entry_compressed_size(entry) as u64,
        ]));
    }
    extra
}

fn zip_central_extra(entry: &ZipTestEntry<'_>, local_header_offset: u32) -> Vec<u8> {
    let mut extra = entry.central_extra.clone();
    if entry.force_zip64 {
        extra.extend_from_slice(&zip64_extended_info_extra_bytes(&[
            zip_test_entry_uncompressed_size(entry) as u64,
            zip_test_entry_compressed_size(entry) as u64,
            local_header_offset as u64,
        ]));
    }
    extra
}

fn zip64_extended_info_extra_bytes(fields: &[u64]) -> Vec<u8> {
    let mut payload = Vec::new();
    for field in fields {
        push_zip_u64(&mut payload, *field);
    }
    zip_extra_field_bytes(ZIP64_EXTENDED_INFORMATION_EXTRA, &payload)
}

fn zip_extra_field_bytes(tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut extra = Vec::new();
    push_zip_u16(&mut extra, tag);
    push_zip_u16(&mut extra, payload.len() as u16);
    extra.extend_from_slice(payload);
    extra
}

fn push_zip_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_zip_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_zip_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn set_zip_u16_at(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn set_zip_u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn set_zip_u64_at(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn set_test_zip_entry_uncompressed_sizes(bytes: &mut [u8], sizes: &[u32]) {
    let eocd_offset = find_zip_eocd(bytes).unwrap();
    let mut central_offset = zip_u32_at(bytes, eocd_offset + 16).unwrap() as usize;
    for size in sizes {
        let local_header_offset = zip_u32_at(bytes, central_offset + 42).unwrap() as usize;
        set_zip_u32_at(bytes, local_header_offset + 22, *size);
        set_zip_u32_at(bytes, central_offset + 24, *size);
        let name_len = zip_u16_at(bytes, central_offset + 28).unwrap() as usize;
        let extra_len = zip_u16_at(bytes, central_offset + 30).unwrap() as usize;
        let comment_len = zip_u16_at(bytes, central_offset + 32).unwrap() as usize;
        central_offset += 46 + name_len + extra_len + comment_len;
    }
}

fn zip_unicode_path_extra_bytes(header_name: &[u8], unicode_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(1);
    push_zip_u32(&mut payload, zip_crc32(header_name));
    payload.extend_from_slice(unicode_name.as_bytes());

    let mut extra = Vec::new();
    push_zip_u16(&mut extra, 0x7075);
    push_zip_u16(&mut extra, payload.len() as u16);
    extra.extend_from_slice(&payload);
    extra
}

fn gzip_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn deflate_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
