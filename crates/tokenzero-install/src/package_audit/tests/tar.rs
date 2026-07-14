use super::fixtures::*;
use super::harness::*;
use super::*;
#[test]
fn package_audit_rejects_tar_archive_dev_target_launcher_payload() {
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    run_tar_contract(
        &[TarTestEntry::new(member, b'0', b"#!/bin/sh\nexec target/release/tokenzero \"$@\"\n")],
        &ContractCase { label: "dev launcher", expect_codes: &["dev_runtime_launcher"], expect_fields: &[("code", "dev_runtime_launcher"), ("member", member)], },
    );
}
#[test]
fn package_audit_fails_closed_on_archive_link_target_control_characters() {
    let member = "tokenzero-v0.1.1/bin/tokenzero";
    let link_target = "bin/tokenzero\rshim";
    run_tar_contract(
        &[TarTestEntry::new(member, b'2', b"").with_link_target(link_target)],
        &ContractCase { label: "link control", expect_codes: &["archive_link_target_uninspectable"], expect_fields: &[ ("code", "archive_link_target_uninspectable"), ("member", member), ("link_target", link_target), ("reason", "control_character"), ], },
    );
}
#[test]
fn package_audit_rejects_private_gzip_tar_members_in_process() {
    let dir = tempdir().unwrap();
    let tar_path = dir.path().join("release.tar");
    let artifact = dir.path().join("release.tar.gz");
    write_test_tar(&tar_path, &["tokenzero-v0.1.1/._LICENSE", "tokenzero-v0.1.1/.tokenzero/config.json"]);
    fs::write(&artifact, gzip_bytes(&fs::read(&tar_path).unwrap())).unwrap();
    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "appledouble_metadata")]);
    assert_issue(&issues, &[("code", "private_tool_state_member")]);
}
#[test]
fn package_audit_rejects_concatenated_gzip_tar_members() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar.gz");
    let visible = test_tar_entry_bytes("tokenzero-v0.1.1/LICENSE", b"MIT");
    let mut hidden = test_tar_entry_bytes("tokenzero-v0.1.1/.tokenzero/config.json", b"{}");
    hidden.extend_from_slice(&[0u8; 1024]);
    let mut bytes = gzip_bytes(&visible);
    bytes.extend_from_slice(&gzip_bytes(&hidden));
    fs::write(&artifact, bytes).unwrap();
    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "private_tool_state_member"), ("member", "tokenzero-v0.1.1/.tokenzero/config.json")]);
}
#[test]
fn package_audit_fails_closed_on_tar_end_marker_contracts() {
    let (report, issues) = write_tar_and_audit(|a| {
        fs::write(a, test_tar_entry_bytes("tokenzero-v0.1.1/LICENSE", b"MIT")).unwrap();
    });
    assert_audit_rejected(&report);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_metadata_malformed"
        && i["detail"].as_str().is_some_and(|d| d.contains("end-of-archive marker"))));
    let (report, issues) = write_tar_and_audit(|a| {
        write_test_tar(a, &["tokenzero-v0.1.1/LICENSE"]);
        let mut b = fs::read(a).unwrap();
        b.extend_from_slice(&test_tar_entry_bytes("tokenzero-v0.1.1/.tokenzero/config.json", b"{}"));
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert!(issues.iter().any(|i| i["code"] == "archive_trailing_data"
        && i["detail"].as_str().is_some_and(|d| d.contains("end-of-archive marker"))));
}
#[test]
fn package_audit_fails_closed_on_tar_private_owner_metadata() {
    let member = "tokenzero-v0.1.1/LICENSE";
    let (report, issues) = write_tar_and_audit(|a| {
        let mut header = test_tar_header(member, b'0', 0, None);
        write_tar_octal(&mut header[108..116], 501);
        write_tar_octal(&mut header[116..124], 20);
        header[265..271].copy_from_slice(b"aditya");
        header[297..302].copy_from_slice(b"staff");
        write_test_tar_checksum(&mut header);
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(&[0u8; 1024]);
        fs::write(a, bytes).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue_fields(&issues, "archive_private_owner_metadata", member, &["uid", "gid", "uname", "gname"], &report);
}
#[test]
fn package_audit_fails_closed_on_tar_special_member_types() {
    let cases = [
        ("tokenzero-v0.1.1/dev/null", b'3', "character_device", b"".as_slice()),
        ("tokenzero-v0.1.1/run/install.fifo", b'6', "fifo", b""),
        ("tokenzero-v0.1.1/bin/tokenzero", b'S', "sparse_file", b"target/release/tokenzero"),
    ];
    let entries: Vec<_> = cases.iter().map(|(m, f, _, d)| TarTestEntry::new(m, *f, d)).collect();
    let (report, issues) = run_tar_audit(&entries);
    assert_audit_rejected(&report);
    for (member, _, reason, _) in cases {
        assert_issue(&issues, &[("code", "archive_unsupported_member_type"), ("member", member), ("reason", reason)]);
    }
}
#[test]
fn package_audit_rejects_long_and_pax_name_overrides() {
    let long_member = format!("tokenzero-v0.1.1/{}/{}/{}/.env", "a".repeat(90), "b".repeat(90), "c".repeat(90));
    let long_payload = format!("{long_member}\0");
    let (report, issues) = run_tar_audit(&[
        TarTestEntry::new("././@LongLink", b'L', long_payload.as_bytes()),
        TarTestEntry::new("payload.txt", b'0', b""),
    ]);
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "sensitive_member_name"), ("member", long_member.as_str())]);
    let pax_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax = pax_record("path", pax_member);
    run_tar_contract(
        &[TarTestEntry::new("./PaxHeaders.0/config.json", b'x', &pax), TarTestEntry::new("config.json", b'0', b"")],
        &ContractCase { label: "pax path private", expect_codes: &["private_tool_state_member"], expect_fields: &[("code", "private_tool_state_member"), ("member", pax_member)], },
    );
}
#[test]
fn package_audit_accepts_empty_pax_delete_overrides() {
    for (key, header, body) in [
        ("path", "./PaxHeaders.0/LICENSE", ("tokenzero-v0.1.1/LICENSE", b'0', b"MIT".as_slice(), None)),
        ("linkpath", "./PaxHeaders.0/tokenzero-link", ("tokenzero-v0.1.1/bin/tokenzero-link", b'2', b"".as_slice(), Some("bin/tokenzero"))),
    ] {
        let dir = tempfile::Builder::new().prefix("tokenzero-test-").tempdir().unwrap();
        let artifact = dir.path().join("release.tar");
        let pax_payload = pax_record(key, "");
        let mut entry = TarTestEntry::new(body.0, body.1, body.2);
        if let Some(link) = body.3 {
            entry = entry.with_link_target(link);
        }
        write_test_tar_entries(&artifact, &[TarTestEntry::new(header, b'x', &pax_payload), entry]);
        assert_eq!(package_audit(dir.path(), &[artifact])["ok"], true);
    }
}
#[test]
fn package_audit_empty_pax_suppresses_global_pax_overrides() {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    let global_pax = pax_record("path", global_path);
    let delete_pax = pax_record("path", "");
    write_test_tar_entries(&artifact, &[
        TarTestEntry::new("./GlobalHead.0", b'g', &global_pax),
        TarTestEntry::new("./PaxHeaders.0/payload.bin", b'x', &delete_pax),
        TarTestEntry::new("payload.bin", b'0', &inner_bytes),
    ]);
    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "archive_global_pax_override_present"), ("field", "path")]);
    assert_no_issue_code_member(&issues, "private_tool_state_member", nested_member);
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let global_link_target = "../.env";
    let global_pax = pax_record("linkpath", global_link_target);
    let delete_pax = pax_record("linkpath", "");
    let (report, issues) = run_tar_audit(&[
        TarTestEntry::new("./GlobalHead.0", b'g', &global_pax),
        TarTestEntry::new("./PaxHeaders.0/tokenzero-link", b'x', &delete_pax),
        TarTestEntry::new(member, b'2', b"").with_link_target("bin/tokenzero"),
    ]);
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "archive_global_pax_override_present"), ("field", "linkpath")]);
    assert_eq!(
        issues.iter().any(|i| i["code"] == "archive_link_target_escape" && i["member"] == member && i["link_target"] == global_link_target),
        false,
        "global PAX linkpath should be deleted for the symlink member: {report:#}"
    );
}
#[test]
fn package_audit_fails_closed_on_tar_size_and_checksum_matrix() {
    let member = "tokenzero-v0.1.1/LICENSE";
    let (report, issues) = write_tar_and_audit(|a| {
        let mut header = test_tar_header(member, b'0', 0, None);
        header[124..136].copy_from_slice(b"not-octal\0\0\0");
        write_test_tar_checksum(&mut header);
        fs::write(a, header).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "archive_member_size_malformed"), ("member", member)]);
    let sensitive = "tokenzero-v0.1.1/.env";
    let payload = b"license";
    let (report, issues) = write_tar_and_audit(|a| {
        let mut bytes = test_tar_entry_bytes(sensitive, payload);
        write_tar_base256(&mut bytes[124..136], payload.len() as u128);
        write_test_tar_checksum_bytes(&mut bytes[0..512]);
        bytes.extend_from_slice(&[0u8; 1024]);
        fs::write(a, bytes).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "sensitive_member_name"), ("member", sensitive)]);
    assert_no_issue(&issues, "archive_member_size_malformed");
    let (report, issues) = write_tar_and_audit(|a| {
        let mut header = test_tar_header(member, b'0', 0, None);
        header[124..136].fill(0xff);
        write_test_tar_checksum(&mut header);
        fs::write(a, header).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue_detail(&issues, "archive_member_size_malformed", member, "negative base-256");
    let (report, issues) = write_tar_and_audit(|a| {
        let mut header = test_tar_header(member, b'0', 0, None);
        header[124..136].fill(0);
        header[124] = 0x81;
        write_test_tar_checksum(&mut header);
        fs::write(a, header).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue_detail(&issues, "archive_member_size_malformed", member, "too large");
    let (report, issues) = write_tar_and_audit(|a| {
        let mut header = test_tar_header(member, b'0', 0, None);
        header[148..156].copy_from_slice(b"000000\0 ");
        fs::write(a, header).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue_detail(&issues, "archive_member_metadata_malformed", member, "checksum");
    let (report, issues) = write_tar_and_audit(|a| {
        let mut bytes = test_tar_header(member, b'0', 16, None).to_vec();
        bytes.extend_from_slice(b"partial");
        fs::write(a, bytes).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "archive_member_payload_truncated"), ("member", member)]);
}
#[test]
fn package_audit_fails_closed_on_pax_malformed_and_metadata_matrix() {
    let hidden = "tokenzero-v0.1.1/.tokenzero/config.json";
    let malformed = format!("999 path={hidden}\n");
    run_tar_contract(
        &[TarTestEntry::new("./PaxHeaders.0/config.json", b'x', malformed.as_bytes()), TarTestEntry::new("config.json", b'0', b"")],
        &ContractCase { label: "malformed pax", expect_codes: &["archive_member_metadata_malformed"], expect_fields: &[("code", "archive_member_metadata_malformed"), ("member", "./PaxHeaders.0/config.json")], },
    );
    let dir = tempdir().unwrap();
    let path_a = dir.path().join("duplicate-path.tar");
    let link_a = dir.path().join("duplicate-linkpath.tar");
    let mut dup_path = pax_record("path", hidden);
    dup_path.extend_from_slice(&pax_record("path", "tokenzero-v0.1.1/config.json"));
    write_test_tar_entries(&path_a, &[
        TarTestEntry::new("./PaxHeaders.0/config.json", b'x', &dup_path),
        TarTestEntry::new("config.json", b'0', b"{}"),
    ]);
    let mut dup_link = pax_record("linkpath", "../.env");
    dup_link.extend_from_slice(&pax_record("linkpath", "config.json"));
    write_test_tar_entries(&link_a, &[
        TarTestEntry::new("./PaxHeaders.0/config-link", b'x', &dup_link),
        TarTestEntry::new("config-link", b'2', b"").with_link_target("config.json"),
    ]);
    let report = package_audit(dir.path(), &[path_a, link_a]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    for (member, field) in [("./PaxHeaders.0/config.json", "path"), ("./PaxHeaders.0/config-link", "linkpath")] {
        assert_issue_detail(&issues, "archive_member_metadata_malformed", member, field);
    }
    let (report, issues) = write_tar_and_audit(|a| {
        let mut pax = pax_record("uname", "builder");
        pax.extend_from_slice(&pax_record("comment", "/tmp/example/release"));
        write_test_tar_entries(a, &[
            TarTestEntry::new("./PaxHeaders.0/LICENSE", b'x', &pax),
            TarTestEntry::new("tokenzero-v0.1.1/LICENSE", b'0', b"MIT"),
        ]);
    });
    assert_audit_rejected(&report);
    assert_issue_fields(&issues, "archive_pax_metadata_present", "./PaxHeaders.0/LICENSE", &["uname", "comment"], &report);
    assert_issue_no_secret(&issues, "archive_pax_metadata_present", "./PaxHeaders.0/LICENSE", "builder", &report);
    assert_issue_no_secret(&issues, "archive_pax_metadata_present", "./PaxHeaders.0/LICENSE", "/tmp/example", &report);
    let pax = pax_record("SCHILY.xattr.com.apple.quarantine", "local-machine");
    let (report, issues) = run_tar_audit(&[
        TarTestEntry::new("./GlobalHead.0", b'g', &pax),
        TarTestEntry::new("tokenzero-v0.1.1/LICENSE", b'0', b"MIT"),
    ]);
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "archive_pax_metadata_present"), ("member", "./GlobalHead.0")]);
    assert!(issues.iter().any(|i| i["code"] == "archive_pax_metadata_present"
        && i["fields"].as_array().unwrap().iter().any(|f| f == "SCHILY.xattr.*")), "{issues:#?}");
}
#[test]
fn package_audit_global_pax_override_contracts() {
    for (key, secret, body) in [
        ("path", "tokenzero-v0.1.1/LICENSE", ("LICENSE", b'0', b"MIT".as_slice(), None)),
        ("linkpath", "bin/tokenzero", ("tokenzero-v0.1.1/bin/tokenzero-link", b'2', b"".as_slice(), Some("bin/tokenzero"))),
    ] {
        let pax = pax_record(key, secret);
        let mut entry = TarTestEntry::new(body.0, body.1, body.2);
        if let Some(link) = body.3 {
            entry = entry.with_link_target(link);
        }
        let (report, issues) = run_tar_audit(&[TarTestEntry::new("./GlobalHead.0", b'g', &pax), entry]);
        assert_audit_rejected(&report);
        assert_issue_no_secret(&issues, "archive_global_pax_override_present", "./GlobalHead.0", secret, &report);
    }
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.tar");
    let global_path = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested_member = "tokenzero-v0.1.1/.tokenzero/config.json";
    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    let pax = pax_record("path", global_path);
    write_test_tar_entries(&artifact, &[
        TarTestEntry::new("./GlobalHead.0", b'g', &pax),
        TarTestEntry::new("payload.bin", b'0', &inner_bytes),
    ]);
    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    assert!(issues.iter().any(|i| i["code"] == "private_tool_state_member"
        && i["path"].as_str().is_some_and(|p| p.contains("release.tar!") && p.contains(global_path))
        && i["member"] == nested_member));
    let global_path = "tokenzero-v0.1.1/LICENSE";
    let pax = pax_record("path", global_path);
    run_tar_contract(
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax),
            TarTestEntry::new("first.txt", b'0', b"first"),
            TarTestEntry::new("second.txt", b'0', b"second"),
        ],
        &ContractCase { label: "global path duplicate", expect_codes: &["tar_duplicate_member_name"], expect_fields: &[("code", "tar_duplicate_member_name"), ("member", global_path)], },
    );
    let member = "tokenzero-v0.1.1/bin/tokenzero-link";
    let global_link_target = "../.env";
    let pax = pax_record("linkpath", global_link_target);
    run_tar_contract(
        &[
            TarTestEntry::new("./GlobalHead.0", b'g', &pax),
            TarTestEntry::new(member, b'2', b"").with_link_target("bin/tokenzero"),
        ],
        &ContractCase { label: "global linkpath escape", expect_codes: &["archive_link_target_escape"], expect_fields: &[("code", "archive_link_target_escape"), ("member", member), ("link_target", global_link_target)], },
    );
}
#[test]
fn package_audit_fails_closed_on_duplicate_tar_member_names() {
    let dir = tempdir().unwrap();
    let tar_a = dir.path().join("release.tar");
    let gzip_a = dir.path().join("release.tar.gz");
    let member = "tokenzero-v0.1.1/LICENSE";
    write_test_tar_entries(&tar_a, &[TarTestEntry::new(member, b'0', b"first"), TarTestEntry::new(member, b'0', b"second")]);
    fs::write(&gzip_a, gzip_bytes(&fs::read(&tar_a).unwrap())).unwrap();
    let report = package_audit(dir.path(), &[tar_a.clone(), gzip_a.clone()]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    for a in [tar_a, gzip_a] {
        assert_issue(&issues, &[("code", "tar_duplicate_member_name"), ("path", &a.display().to_string()), ("member", member)]);
    }
}
#[test]
fn package_audit_rejects_path_and_link_escape_matrix() {
    let parent = "tokenzero-v0.1.1/../.env";
    let absolute = "/tmp/tokenzero/LICENSE";
    let windows = "C:/Users/example/.ssh/id_ed25519";
    let (report, issues) = run_tar_audit_from_names(&[parent, absolute, windows]);
    assert_audit_rejected(&report);
    for (member, reason) in [(parent, "parent_directory"), (absolute, "absolute_path"), (windows, "windows_drive_path")] {
        assert_issue(&issues, &[("code", "archive_member_path_escape"), ("member", member), ("reason", reason)]);
    }
    let symlink = "tokenzero-v0.1.1/bin/tokenzero";
    let hardlink = "tokenzero-v0.1.1/cache/recovery-cache.json";
    let (report, issues) = run_tar_audit(&[
        TarTestEntry::new(symlink, b'2', b"").with_link_target("../.env"),
        TarTestEntry::new(hardlink, b'1', b"").with_link_target("/home/example/.tokenzero/recovery-cache.json"),
    ]);
    assert_audit_rejected(&report);
    for fields in [
        &[("code", "archive_link_target_escape"), ("member", symlink), ("link_kind", "symlink"), ("reason", "parent_directory")][..],
        &[("code", "sensitive_link_target"), ("member", symlink), ("link_target", "../.env")][..],
        &[("code", "archive_link_target_escape"), ("member", hardlink), ("link_kind", "hardlink"), ("reason", "absolute_path")][..],
        &[("code", "private_tool_state_link_target"), ("member", hardlink), ("link_target", "/home/example/.tokenzero/recovery-cache.json")][..],
    ] {
        assert_issue(&issues, fields);
    }
}
#[test]
fn package_audit_rejects_private_dotdir_members_and_targets() {
    let dir = tempdir().unwrap();
    let tar_a = dir.path().join("release.tar");
    let zip_a = dir.path().join("release.zip");
    let tar_dir = "tokenzero-v0.1.1/.tokenzero";
    let zip_dir = "tokenzero-v0.1.1/.cursor/";
    write_test_tar_entries(&tar_a, &[TarTestEntry::new(tar_dir, b'5', b"")]);
    write_test_zip(&zip_a, &[ZipTestEntry::file(zip_dir, b"")]);
    let report = package_audit(dir.path(), &[tar_a, zip_a]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    for member in [tar_dir, zip_dir] {
        assert_issue(&issues, &[("code", "private_tool_state_member"), ("member", member)]);
    }
    let symlink = "tokenzero-v0.1.1/config-link";
    run_tar_contract(
        &[TarTestEntry::new(symlink, b'2', b"").with_link_target(".tokenzero")],
        &ContractCase { label: "dotdir link", expect_codes: &["private_tool_state_link_target"], expect_fields: &[("code", "private_tool_state_link_target"), ("member", symlink), ("link_target", ".tokenzero")], },
    );
}
#[test]
fn package_audit_rejects_pax_and_gnu_link_and_name_conflicts() {
    let global_path = "tokenzero-v0.1.1/.tokenzero/config.json";
    let global_linkpath = "../.env";
    let mut pax_payload = pax_record("path", global_path);
    pax_payload.extend_from_slice(&pax_record("linkpath", global_linkpath));
    let (report, issues) = run_tar_audit(&[
        TarTestEntry::new("./GlobalHead.0", b'g', &pax_payload),
        TarTestEntry::new("tokenzero-v0.1.1/config.json", b'0', b"{}"),
    ]);
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "private_tool_state_member"), ("member", global_path)]);
    assert_issue(&issues, &[("code", "archive_link_target_escape"), ("member", "./GlobalHead.0"), ("link_target", global_linkpath), ("reason", "parent_directory")]);
    let pax_member = "tokenzero-v0.1.1/config";
    let pax_target = "tokenzero-v0.1.1/.tokenzero/config.json";
    let gnu_member = "tokenzero-v0.1.1/ssh-key";
    let gnu_target = format!("../{}/id_ed25519", "private".repeat(20));
    let gnu_payload = format!("{gnu_target}\0");
    let pax = pax_record("linkpath", pax_target);
    let (report, issues) = run_tar_audit(&[
        TarTestEntry::new("./PaxHeaders.0/config", b'x', &pax),
        TarTestEntry::new(pax_member, b'2', b""),
        TarTestEntry::new("././@LongLink", b'K', gnu_payload.as_bytes()),
        TarTestEntry::new(gnu_member, b'2', b""),
    ]);
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "private_tool_state_link_target"), ("member", pax_member), ("link_target", pax_target)]);
    assert_issue(&issues, &[("code", "archive_link_target_escape"), ("member", gnu_member), ("link_target", gnu_target.as_str()), ("reason", "parent_directory")]);
    assert_issue(&issues, &[("code", "sensitive_link_target"), ("member", gnu_member), ("link_target", gnu_target.as_str())]);
    let private = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax = pax_record("path", private);
    run_tar_contract(
        &[
            TarTestEntry::new("./PaxHeaders.0/config.json", b'x', &pax),
            TarTestEntry::new("././@LongLink", b'L', b"tokenzero-v0.1.1/config.json\0"),
            TarTestEntry::new("config.json", b'0', b""),
        ],
        &ContractCase { label: "name conflict", expect_codes: &["private_tool_state_member"], expect_fields: &[("code", "private_tool_state_member"), ("member", private)], },
    );
    let symlink = "tokenzero-v0.1.1/config-link";
    let private_target = "tokenzero-v0.1.1/.tokenzero/config.json";
    let pax = pax_record("linkpath", private_target);
    run_tar_contract(
        &[
            TarTestEntry::new("./PaxHeaders.0/config-link", b'x', &pax),
            TarTestEntry::new("././@LongLink", b'K', b"tokenzero-v0.1.1/config.json\0"),
            TarTestEntry::new(symlink, b'2', b"").with_link_target("config.json"),
        ],
        &ContractCase { label: "link conflict", expect_codes: &["private_tool_state_link_target"], expect_fields: &[("code", "private_tool_state_link_target"), ("member", symlink), ("link_target", private_target)], },
    );
}
#[test]
fn package_audit_fails_closed_on_tar_directory_payload() {
    let member = "tokenzero-v0.1.1/docs/";
    run_tar_contract(
        &[TarTestEntry::new(member, b'5', b"hidden")],
        &ContractCase { label: "dir payload", expect_codes: &["tar_directory_payload_present"], expect_fields: &[("code", "tar_directory_payload_present"), ("member", member)], },
    );
}
