use super::fixtures::*;
use super::*;

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
