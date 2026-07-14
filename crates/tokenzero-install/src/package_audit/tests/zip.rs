use super::fixtures::*;
use super::harness::*;
use super::*;
use std::path::{Path, PathBuf};
#[test]
fn package_audit_rejects_zip_symlink_target_escape() {
    let m = "tokenzero-v0.1.1/bin/tokenzero";
    let (report, issues) = run_zip_audit(&[ZipTestEntry::symlink(m, b"../.env")]);
    assert_audit_rejected(&report);
    assert_symlink_escape_issues(&issues, m);
}
#[test]
fn package_audit_fails_closed_on_unreadable_zip_symlink_target() {
    let m = "tokenzero-v0.1.1/config-link";
    let (report, issues) = run_zip_audit(&[ZipTestEntry::symlink(m, b"not-deflated").with_method(8)]);
    assert_audit_rejected(&report);
    assert_issue_detail(&issues, "zip_symlink_target_unreadable", m, "deflate");
}
#[test]
fn symlink_escape_encoding_variants() {
    let m = "tokenzero-v0.1.1/bin/tokenzero";
    let compressed = deflate_bytes(b"../.env");
    for entry in [
        ZipTestEntry::symlink(m, &compressed).with_method(8),
        ZipTestEntry::symlink(m, b"../.env").with_data_descriptor(),
        ZipTestEntry::symlink(m, b"../.env").with_unsigned_data_descriptor(),
    ] {
        let (report, issues) = run_zip_audit(&[entry]);
        assert_audit_rejected(&report);
        assert_symlink_escape_issues(&issues, m);
        assert_eq!(issues.iter().any(|i| i["code"] == "zip_data_descriptor_mismatch"), false);
    }
}
#[test]
fn data_descriptor_tamper_matrix() {
    let m = "tokenzero-v0.1.1/bin/tokenzero-link";
    let report = tamper_zip_data_descriptor(|bytes, _| {
        let local = zip_local_header(bytes, 0).unwrap_or_else(|e| panic!("{}", zip_payload_error_detail(e)));
        set_zip_u32_at(bytes, local.data_start + b"bin/tokenzero".len() + 4, zip_crc32(b"bin/tokenzero") ^ u32::MAX);
    });
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    assert_issue_detail(&issues, "zip_data_descriptor_mismatch", m, "CRC");
    let report = tamper_zip_data_descriptor(|bytes, _| {
        let wrong = u32::try_from(b"bin/tokenzero".len() + 1).unwrap();
        set_zip_u32_at(bytes, 18, wrong);
        set_zip_u32_at(bytes, 22, wrong);
    });
    let issues = report["issues"].as_array().unwrap().clone();
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "zip_local_header_metadata_mismatch"), ("member", m), ("field", "data_descriptor_sizes")]);
}
#[test]
fn package_audit_fails_closed_on_zip64_data_descriptor_size_mismatch() {
    let m = "tokenzero-v0.1.1/config-link";
    let target = b"config.json";
    let (report, issues) = write_zip_and_audit(|artifact| {
        write_test_zip(artifact, &[ZipTestEntry::symlink(m, target).with_data_descriptor().with_zip64_extra_fields()]);
        let (mut bytes, _, cd) = read_zip_with_offsets(artifact);
        let name_len = zip_u16_at(&bytes, cd + 28).unwrap() as usize;
        let off = cd + 46 + name_len;
        assert_eq!(zip_u16_at(&bytes, off).unwrap(), ZIP64_EXTENDED_INFORMATION_EXTRA);
        set_zip_u64_at(&mut bytes, off + 4, u32::MAX as u64 + 1 + target.len() as u64);
        fs::write(artifact, bytes).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_data_descriptor_mismatch" && i["member"] == m
        && i["detail"].as_str().is_some_and(|d| d.contains("zip64 descriptor"))));
}
#[test]
fn package_audit_fails_closed_on_zip_size_and_overlap_matrix() {
    let member = "tokenzero-v0.1.1/LICENSE";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(member, b"MIT")]);
        let (mut b, _, cd) = read_zip_with_offsets(a);
        set_zip_u32_at(&mut b, 22, 4);
        set_zip_u32_at(&mut b, cd + 24, 4);
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "zip_entry_size_mismatch"), ("member", member)]);
    let symlink = "tokenzero-v0.1.1/config-link";
    let target = b"config.json";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::symlink(symlink, target)]);
        let (mut b, _, cd) = read_zip_with_offsets(a);
        set_zip_u32_at(&mut b, 22, target.len() as u32 + 1);
        set_zip_u32_at(&mut b, cd + 24, target.len() as u32 + 1);
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue_detail(&issues, "zip_symlink_target_unreadable", symlink, "uncompressed size mismatch");
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(member, b"MIT")]);
        let local = zip_local_header(&fs::read(a).unwrap(), 0).unwrap_or_else(|e| panic!("{}", zip_payload_error_detail(e)));
        let (mut b, _, cd) = read_zip_with_offsets(a);
        let size = (cd - local.data_start + 1) as u32;
        for off in [18usize, 22, cd + 20, cd + 24] { set_zip_u32_at(&mut b, off, size); }
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "zip_local_record_overlap"), ("member", member), ("field", "central_directory")]);
    let first = "tokenzero-v0.1.1/LICENSE";
    let second = "tokenzero-v0.1.1/NOTICE";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(first, b"first"), ZipTestEntry::file(second, b"second")]);
        let first_header = zip_local_header(&fs::read(a).unwrap(), 0).unwrap_or_else(|e| panic!("{}", zip_payload_error_detail(e)));
        let (mut b, _, cd) = read_zip_with_offsets(a);
        let size = (first_header.data_start + b"first".len() - first_header.data_start + 1) as u32;
        for off in [18usize, 22, cd + 20, cd + 24] { set_zip_u32_at(&mut b, off, size); }
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "zip_local_record_overlap"), ("member", first), ("field", "local_record"), ("next_member", second)]);
    let m = "tokenzero-v0.1.1/bin/tokenzero-link";
    let target = b"bin/tokenzero";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::symlink(m, target).with_data_descriptor()]);
        let local = zip_local_header(&fs::read(a).unwrap(), 0).unwrap_or_else(|e| panic!("{}", zip_payload_error_detail(e)));
        let (mut b, _, cd) = read_zip_with_offsets(a);
        let start = local.data_start + target.len();
        assert_eq!(cd - start, 16);
        b.drain(start..cd);
        let eocd = find_zip_eocd(&b).unwrap();
        set_zip_u32_at(&mut b, eocd + 16, start as u32);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_data_descriptor_mismatch" && i["member"] == m
        && i["detail"].as_str().is_some_and(|d| d.contains("before the central directory"))));
}
#[test]
fn package_audit_nested_archive_matrix() {
    let outer = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested = "tokenzero-v0.1.1/.tokenzero/config.json";
    {
        let (report, issues) = nested_private_in_zip(outer, nested, |a, inner| {
            write_test_zip(a, &[ZipTestEntry::file(outer, inner)]);
        });
        assert_eq!(report["ok"], false);
        assert_nested_private(&issues, outer, nested);
    }
    {
        let (report, issues) = nested_private_in_zip(outer, nested, |a, inner| {
            let c = deflate_bytes(inner);
            write_test_zip(a, &[ZipTestEntry::file(outer, &c).with_method(8)]);
        });
        assert_eq!(report["ok"], false);
        assert_nested_private(&issues, outer, nested);
    }
    {
        let (report, issues) = nested_private_in_zip(outer, nested, |a, inner| {
            write_test_zip(a, &[ZipTestEntry::file(outer, inner).with_zip64_extra_fields()]);
        });
        assert_eq!(report["ok"], false);
        assert_nested_private(&issues, outer, nested);
    }
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    write_test_tar(&inner, &["tokenzero-v0.1.1/LICENSE"]);
    let inner_bytes = fs::read(&inner).unwrap();
    write_test_zip(&artifact, &[ZipTestEntry::file(outer, &inner_bytes)]);
    let mut bytes = fs::read(&artifact).unwrap();
    let eocd = find_zip_eocd(&bytes).unwrap();
    let cd = zip_u32_at(&bytes, eocd + 16).unwrap() as usize;
    let wrong = zip_crc32(&inner_bytes) ^ u32::MAX;
    set_zip_u32_at(&mut bytes, 14, wrong);
    set_zip_u32_at(&mut bytes, cd + 16, wrong);
    fs::write(&artifact, bytes).unwrap();
    let report = package_audit(dir.path(), &[artifact]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "nested_archive_member_unreadable" && i["member"] == outer
        && i["detail"].as_str().is_some_and(|d| d.contains("CRC mismatch"))));
}
#[test]
fn package_audit_zip_name_and_unicode_path_matrix() {
    let visible = "tokenzero-v0.1.1/config.json";
    let unicode = "tokenzero-v0.1.1/.tokenzero/config.json";
    let (report, issues) = run_zip_audit(&[ZipTestEntry::file(visible, b"{}").with_local_name(unicode)]);
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_local_header_name_mismatch" && i["member"] == visible && i["local_member"] == unicode));
    assert!(issues.iter().any(|i| i["code"] == "private_tool_state_member" && i["member"] == unicode));
    for entry in [
        ZipTestEntry::file(visible, b"{}").with_central_unicode_path(unicode),
        ZipTestEntry::file(visible, b"{}").with_local_unicode_path(unicode),
    ] {
        let (report, issues) = run_zip_audit(&[entry]);
        assert_eq!(report["ok"], false);
        assert!(issues.iter().any(|i| i["code"] == "private_tool_state_member" && i["member"] == unicode));
    }
    let central_u = "tokenzero-v0.1.1/config-central.json";
    let local_u = "tokenzero-v0.1.1/config-local.json";
    let (report, issues) = run_zip_audit(&[ZipTestEntry::file(visible, b"{}").with_central_unicode_path(central_u).with_local_unicode_path(local_u)]);
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_local_header_metadata_mismatch" && i["member"] == visible
        && i["field"] == "unicode_path" && i["central"] == central_u && i["local"] == local_u));
    let (report, issues) = run_zip_audit(&[ZipTestEntry::file(visible, b"{}").with_central_extra(vec![0x75, 0x70, 0x05, 0x00, 1, 0, 0, 0, 0])]);
    assert_audit_rejected(&report);
    assert_listing_failure(&issues, "unicode path extra field");
    let vis = "tokenzero-v0.1.1/artifacts/payload.bin";
    let uni = "tokenzero-v0.1.1/artifacts/inner.tar";
    let nested = "tokenzero-v0.1.1/.tokenzero/config.json";
    let (report, issues) = nested_private_in_zip(uni, nested, |a, inner| {
        write_test_zip(a, &[ZipTestEntry::file(vis, inner).with_central_unicode_path(uni)]);
    });
    assert_eq!(report["ok"], false);
    assert_nested_private(&issues, uni, nested);
    run_zip_contract(
        &[ZipTestEntry::file("tokenzero-v0.1.1/metadata", b"").with_central_unicode_path("tokenzero-v0.1.1/.idea/")],
        &ContractCase { label: "unicode dotdir", expect_codes: &["non_public_dotdir_member"], expect_fields: &[("code", "non_public_dotdir_member"), ("member", "tokenzero-v0.1.1/.idea/")] },
    );
}
#[test]
fn package_audit_zip_structure_and_eocd_matrix() {
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")]);
        let (mut b, eocd, _) = read_zip_with_offsets(a);
        set_zip_u16_at(&mut b, eocd + 4, 1);
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_listing_failure(&issues, "multi-disk");
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file("tokenzero-v0.1.1/.tokenzero/config.json", b"{}")]);
        let mut b = fs::read(a).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        set_zip_u16_at(&mut b, eocd + 20, 22);
        push_zip_u32(&mut b, 0x0605_4b50);
        for _ in 0..4 { push_zip_u16(&mut b, 0); }
        push_zip_u32(&mut b, 0);
        push_zip_u32(&mut b, 0);
        push_zip_u16(&mut b, 0);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains("plausible end-of-central-directory"))));
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")]);
        let b = fs::read(a).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        let cd = zip_u32_at(&b, eocd + 16).unwrap() as usize;
        let central = b[cd..eocd].to_vec();
        let mut out = b[..cd].to_vec();
        let new_eocd = out.len();
        push_zip_u32(&mut out, 0x0605_4b50);
        push_zip_u16(&mut out, 0); push_zip_u16(&mut out, 0);
        push_zip_u16(&mut out, 1); push_zip_u16(&mut out, 1);
        push_zip_u32(&mut out, central.len() as u32);
        push_zip_u32(&mut out, (new_eocd + 22) as u32);
        push_zip_u16(&mut out, central.len() as u16);
        out.extend_from_slice(&central);
        fs::write(a, out).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains("central directory overlaps or follows"))));
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[
            ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT"),
            ZipTestEntry::file("tokenzero-v0.1.1/.tokenzero/config.json", b"{}"),
        ]);
        let mut b = fs::read(a).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        set_zip_u16_at(&mut b, eocd + 8, 1);
        set_zip_u16_at(&mut b, eocd + 10, 1);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains("unparsed bytes"))));
}
#[test]
fn package_audit_zip64_matrix() {
    let license = "tokenzero-v0.1.1/LICENSE";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT")]);
        let (mut b, _, cd) = read_zip_with_offsets(a);
        set_zip_u32_at(&mut b, cd + 42, u32::MAX);
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_listing_failure(&issues, "zip64 extended information");
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT").with_zip64_extra_fields()]);
    });
    assert_eq!(report["archives_checked"], 1);
    assert_eq!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"), false, "{report:#}");
    let (report, issues) = write_zip_and_audit(|a| {
        let mut dup = zip64_extended_info_extra_bytes(&[3, 3, 0]);
        dup.extend_from_slice(&zip64_extended_info_extra_bytes(&[3, 3, 0]));
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT").with_central_extra(dup)]);
        let mut b = fs::read(a).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        let cd = zip_u32_at(&b, eocd + 16).unwrap() as usize;
        for off in [20usize, 24, 42] { set_zip_u32_at(&mut b, cd + off, u32::MAX); }
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed" && i["detail"].as_str().is_some_and(|d| d.contains("duplicated"))));
    let dir = tempdir().unwrap();
    let central_a = dir.path().join("central-extra-duplicate.zip");
    let local_a = dir.path().join("local-extra-duplicate.zip");
    let mut dup = zip_extra_field_bytes(0x5455, &[1, 0, 0, 0, 0]);
    dup.extend_from_slice(&zip_extra_field_bytes(0x5455, &[1, 0, 0, 0, 0]));
    write_test_zip(&central_a, &[ZipTestEntry::file(license, b"MIT").with_central_extra(dup.clone())]);
    write_test_zip(&local_a, &[ZipTestEntry::file("tokenzero-v0.1.1/NOTICE", b"MIT").with_local_extra(dup)]);
    let report = package_audit(dir.path(), &[central_a, local_a]);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_eq!(report["ok"], false);
    assert_eq!(issues.iter().filter(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains("0x5455") && d.contains("duplicated"))).count(), 2, "{report:#}");
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT").with_central_extra(zip64_extended_info_extra_bytes(&[3, 3, 0, 42]))]);
        let mut b = fs::read(a).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        let cd = zip_u32_at(&b, eocd + 16).unwrap() as usize;
        for off in [20usize, 24, 42] { set_zip_u32_at(&mut b, cd + off, u32::MAX); }
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains("zip64") && d.contains("unclaimed bytes"))));
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT")]);
        let (mut b, eocd, _) = read_zip_with_offsets(a);
        set_zip_u32_at(&mut b, eocd + 16, u32::MAX);
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_listing_failure(&issues, "zip64");
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT")]);
        let mut b = fs::read(a).unwrap();
        append_zip64_eocd(&mut b);
        let eocd = find_zip_eocd(&b).unwrap();
        let loc = eocd - 20;
        set_zip_u16_at(&mut b, eocd + 8, u16::MAX); set_zip_u16_at(&mut b, eocd + 10, u16::MAX);
        set_zip_u32_at(&mut b, eocd + 12, u32::MAX); set_zip_u32_at(&mut b, eocd + 16, u32::MAX);
        set_zip_u64_at(&mut b, loc + 8, u64::MAX);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains("zip64 end-of-central-directory")
            && (d.contains("overflowed") || d.contains("too large") || d.contains("outside")))));
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(license, b"MIT")]);
        let mut b = fs::read(a).unwrap();
        append_zip64_eocd(&mut b);
        let eocd = find_zip_eocd(&b).unwrap();
        set_zip_u16_at(&mut b, eocd + 8, u16::MAX); set_zip_u16_at(&mut b, eocd + 10, u16::MAX);
        set_zip_u32_at(&mut b, eocd + 12, u32::MAX); set_zip_u32_at(&mut b, eocd + 16, u32::MAX);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["archives_checked"], 1);
    assert_eq!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"), false, "{report:#}");
}
#[test]
fn package_audit_zip_payload_policy_matrix() {
    let member = "tokenzero-v0.1.1/LICENSE";
    let (report, issues) = run_zip_audit(&[ZipTestEntry::file(member, b"MIT").with_flags(ZIP_FLAG_ENCRYPTED)]);
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "zip_entry_uninspectable"), ("member", member)]);
    assert!(issues.iter().any(|i| i["flags"].as_array().into_iter().flatten().any(|f| f == "encrypted")));
    for m in [
        "tokenzero-v0.1.1/bin/tokenzero",
        "tokenzero-v0.1.1/node_modules/addon/build/Release/addon.node",
    ] {
        let (report, issues) = run_zip_audit(&[ZipTestEntry::file(m, b"opaque").with_method(12)]);
        assert_audit_rejected(&report);
        assert_eq!(issues.iter().find(|i| i["code"] == "zip_regular_file_uninspectable" && i["member"] == m).unwrap()["compression_method"], 12);
    }
    let first = "tokenzero-v0.1.1/share/nested-one.zip";
    let second = "tokenzero-v0.1.1/share/nested-two.zip";
    let (report, issues) = write_zip_and_audit(|a| {
        let payload = deflate_bytes(b"tiny");
        write_test_zip(a, &[
            ZipTestEntry::file(first, &payload).with_method(8),
            ZipTestEntry::file(second, &payload).with_method(8),
        ]);
        let mut b = fs::read(a).unwrap();
        let size = u32::try_from(MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES / 2 + 1).unwrap();
        set_test_zip_entry_uncompressed_sizes(&mut b, &[size, size]);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_total_payload_size_exceeded" && i["member"] == second
        && i["limit"].as_u64() == Some(MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES as u64)));
    let dir = tempdir().unwrap();
    let artifacts: Vec<PathBuf> = ["release.zip", "release.tar", "release.tar.gz", "release.tgz", "tokenzero.crate"]
        .iter().map(|n| dir.path().join(n)).collect();
    for a in &artifacts {
        fs::File::create(a).unwrap().set_len(MAX_TOP_LEVEL_ARCHIVE_BYTES + 1).unwrap();
    }
    let report = package_audit(dir.path(), &artifacts);
    let issues = report["issues"].as_array().unwrap().clone();
    assert_eq!(report["ok"], false);
    assert_eq!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"), false, "{report:#}");
    for a in &artifacts {
        let path = a.display().to_string();
        assert!(issues.iter().any(|i| i["code"] == "archive_file_too_large" && i["path"] == path
            && i["limit"].as_u64() == Some(MAX_TOP_LEVEL_ARCHIVE_BYTES)), "missing for {path}: {report:#}");
    }
    run_zip_contract(
        &[ZipTestEntry::file("tokenzero-v0.1.1/docs/", b"hidden")],
        &ContractCase { label: "dir payload", expect_codes: &["zip_directory_payload_present"], expect_fields: &[("code", "zip_directory_payload_present"), ("member", "tokenzero-v0.1.1/docs/")] },
    );
    let m = "tokenzero-v0.1.1/bin/tokenzero";
    let (report, issues) = write_zip_and_audit(|a| {
        let payload = deflate_bytes(b"#!/bin/sh\nexec tokenzero-runtime \"$@\"\n");
        write_test_zip(a, &[ZipTestEntry::file(m, &payload).with_method(8)]);
    });
    assert_eq!(report["archives_checked"], 1);
    assert_eq!(issues.iter().any(|i| i["code"] == "zip_regular_file_uninspectable"), false, "{report:#}");
    let m = "tokenzero-v0.1.1/bin/tokenzero.cmd";
    let (report, issues) = write_zip_and_audit(|a| {
        let payload = deflate_bytes(b"@echo off\r\nuv run tokenzero %*\r\n");
        write_test_zip(a, &[ZipTestEntry::file(m, &payload).with_method(8)]);
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "external_runtime_dependency" && i["member"] == m));
}
#[test]
fn package_audit_zip_metadata_layout_matrix() {
    let member = "tokenzero-v0.1.1/LICENSE";
    let (report, issues) = run_zip_audit(&[ZipTestEntry::file(member, b"MIT").with_comment(b"/tmp/example/release")]);
    assert_audit_rejected(&report);
    assert_eq!(issues.iter().find(|i| i["code"] == "zip_entry_comment_present" && i["member"] == member).unwrap()["comment_bytes"], 20);
    let comment = b"/tmp/example/release";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(member, b"MIT")]);
        let mut b = fs::read(a).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        set_zip_u16_at(&mut b, eocd + 20, comment.len() as u16);
        b.extend_from_slice(comment);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_archive_comment_present" && i["comment_bytes"] == comment.len()));
    let central_m = "tokenzero-v0.1.1/LICENSE";
    let local_m = "tokenzero-v0.1.1/NOTICE";
    let (report, issues) = run_zip_audit(&[
        ZipTestEntry::file(central_m, b"MIT").with_central_extra(zip_extra_field_bytes(0x5455, b"\x01\x00\x00\x00\x00")),
        ZipTestEntry::file(local_m, b"notice").with_local_extra(zip_extra_field_bytes(0x7875, b"\x01\x01\xed\x01\x14")),
    ]);
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_extra_field_present" && i["member"] == central_m && i["field_location"] == "central" && i["tag"] == "0x5455" && i["size"] == 5));
    assert!(issues.iter().any(|i| i["code"] == "zip_extra_field_present" && i["member"] == local_m && i["field_location"] == "local" && i["tag"] == "0x7875" && i["size"] == 5));
    let (report, issues) = run_zip_audit(&[ZipTestEntry::file(member, b"MIT").with_central_extra(zip64_extended_info_extra_bytes(&[3, 3, 0]))]);
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_extra_field_present" && i["member"] == member && i["field_location"] == "central" && i["tag"] == "0x0001"));
    let preamble = b"#!/bin/sh\nexec /tmp/hidden\n";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(member, b"MIT")]);
        let (mut b, eocd, cd) = read_zip_with_offsets(a);
        b.splice(0..0, preamble.iter().copied());
        set_zip_u32_at(&mut b, eocd + preamble.len() + 16, (cd + preamble.len()) as u32);
        set_zip_u32_at(&mut b, cd + preamble.len() + 42, preamble.len() as u32);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_unclaimed_local_bytes" && i["start"] == 0 && i["end"] == preamble.len()));
    let gap = b"raw_traces/local_only";
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(member, b"MIT")]);
        let (mut b, eocd, cd) = read_zip_with_offsets(a);
        b.splice(cd..cd, gap.iter().copied());
        set_zip_u32_at(&mut b, eocd + gap.len() + 16, (cd + gap.len()) as u32);
        fs::write(a, b).unwrap();
    });
    assert_eq!(report["ok"], false);
    assert!(issues.iter().any(|i| i["code"] == "zip_unclaimed_local_bytes" && i["byte_count"] == gap.len()));
    let (report, issues) = write_zip_and_audit(|a| {
        write_test_zip(a, &[ZipTestEntry::file(member, b"MIT")]);
        let mut b = fs::read(a).unwrap();
        set_zip_u16_at(&mut b, 8, 8);
        fs::write(a, b).unwrap();
    });
    assert_audit_rejected(&report);
    assert_issue(&issues, &[("code", "zip_local_header_metadata_mismatch"), ("member", member), ("field", "compression_method")]);
    let issue = issues.iter().find(|i| i["code"] == "zip_local_header_metadata_mismatch" && i["member"] == member).unwrap();
    assert_eq!(issue["central"], 0);
    assert_eq!(issue["local"], 8);
    run_zip_contract(
        &[ZipTestEntry::file(member, b"first"), ZipTestEntry::file(member, b"second")],
        &ContractCase { label: "dup", expect_codes: &["zip_duplicate_member_name"], expect_fields: &[("code", "zip_duplicate_member_name"), ("member", member)] },
    );
}
#[test]
fn package_audit_malformed_zip_corpus_has_stable_listing_failures() {
    struct Case { name: &'static str, build: fn(&Path), detail: &'static str }
    fn missing_eocd(p: &Path) {
        write_test_zip(p, &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")]);
        let mut b = fs::read(p).unwrap();
        b.truncate(find_zip_eocd(&b).unwrap());
        fs::write(p, b).unwrap();
    }
    fn bad_central(p: &Path) {
        write_test_zip(p, &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")]);
        let mut b = fs::read(p).unwrap();
        let eocd = find_zip_eocd(&b).unwrap();
        let cd = zip_u32_at(&b, eocd + 16).unwrap() as usize;
        set_zip_u32_at(&mut b, cd, 0);
        fs::write(p, b).unwrap();
    }
    fn bad_local(p: &Path) {
        write_test_zip(p, &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT")]);
        let mut b = fs::read(p).unwrap();
        set_zip_u32_at(&mut b, 0, 0);
        fs::write(p, b).unwrap();
    }
    fn trunc_extra(p: &Path) {
        write_test_zip(p, &[ZipTestEntry::file("tokenzero-v0.1.1/LICENSE", b"MIT").with_central_extra(vec![0x55, 0x54, 0x01])]);
    }
    let dir = tempdir().unwrap();
    for case in [
        Case { name: "missing-eocd.zip", build: missing_eocd, detail: "end-of-central-directory record was not found" },
        Case { name: "invalid-central-signature.zip", build: bad_central, detail: "central directory entry has an invalid signature" },
        Case { name: "invalid-local-signature.zip", build: bad_local, detail: "local header has an invalid signature" },
        Case { name: "truncated-extra-field.zip", build: trunc_extra, detail: "extra field header is truncated" },
    ] {
        let artifact = dir.path().join(case.name);
        (case.build)(&artifact);
        let report = package_audit(dir.path(), &[artifact]);
        let issues = report["issues"].as_array().unwrap().clone();
        assert_eq!(report["ok"], false, "case {}: {report:#}", case.name);
        assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
            && i["detail"].as_str().is_some_and(|d| d.contains(case.detail))), "case {} missing {:#}", case.name, report);
    }
}
