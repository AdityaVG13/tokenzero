use super::fixtures::*;
use super::*;
use std::path::Path;
fn issue_has_code(issues: &[serde_json::Value], code: &str) -> bool {
    issues.iter().any(|i| i["code"] == code)
}
fn issue_has_code_member(issues: &[serde_json::Value], code: &str, member: &str) -> bool {
    issues.iter().any(|i| i["code"] == code && i["member"] == member)
}
fn text_contains(haystack: &str, needle: &str) -> bool { haystack.contains(needle) }
fn find_issue<'a>(issues: &'a [serde_json::Value], code: &str, member: &str, report: &serde_json::Value) -> &'a serde_json::Value {
    issues.iter().find(|i| i["code"] == code && i["member"] == member)
        .unwrap_or_else(|| panic!("missing {code} issue for {member}: {report:#}"))
}
pub(crate) fn assert_audit_rejected(report: &serde_json::Value) { assert_eq!(report["ok"], false); }
pub(crate) fn assert_issue(issues: &[serde_json::Value], fields: &[(&str, &str)]) {
    assert!(issues.iter().any(|issue| fields.iter().all(|(k, v)| issue[*k] == *v)),
        "expected issue matching {fields:?}\n  in: {issues:#?}");
}
pub(crate) fn assert_no_issue(issues: &[serde_json::Value], code: &str) {
    assert_eq!(issue_has_code(issues, code), false, "unexpected issue with code={code} in {issues:#?}");
}
pub(crate) fn assert_issue_detail(issues: &[serde_json::Value], code: &str, member: &str, s: &str) {
    assert!(issues.iter().any(|i| i["code"] == code && i["member"] == member
        && i["detail"].as_str().is_some_and(|d| d.contains(s))),
        "expected {code} for {member} with detail containing '{s}'\n  in: {issues:#?}");
}
pub(crate) fn assert_issue_fields(issues: &[serde_json::Value], code: &str, member: &str, expected: &[&str], report: &serde_json::Value) {
    let issue = find_issue(issues, code, member, report);
    let fields = issue["fields"].as_array().unwrap();
    for field in expected {
        assert!(fields.iter().any(|v| v == field), "missing {field} field in {issue:#}");
    }
}
pub(crate) fn assert_issue_no_secret(issues: &[serde_json::Value], code: &str, member: &str, secret: &str, report: &serde_json::Value) {
    let issue = find_issue(issues, code, member, report);
    let serialized = serde_json::to_string(issue).unwrap();
    assert_eq!(text_contains(&serialized, secret), false, "issue must not expose '{secret}': {issue:#}");
}
pub(crate) fn assert_no_issue_code_member(issues: &[serde_json::Value], code: &str, member: &str) {
    assert_eq!(issue_has_code_member(issues, code, member), false, "unexpected {code} issue for {member} in {issues:#?}");
}
pub(crate) fn assert_listing_failure(issues: &[serde_json::Value], detail_contains: &str) {
    assert!(issues.iter().any(|i| i["code"] == "archive_member_listing_failed"
        && i["detail"].as_str().is_some_and(|d| d.contains(detail_contains))),
        "expected archive_member_listing_failed with detail containing '{detail_contains}' in {issues:#?}");
}
pub(crate) fn assert_symlink_escape_issues(issues: &[serde_json::Value], symlink_member: &str) {
    assert!(issues.iter().any(|i| i["code"] == "archive_link_target_escape" && i["member"] == symlink_member
        && i["link_kind"] == "symlink" && i["reason"] == "parent_directory"));
    assert!(issues.iter().any(|i| i["code"] == "sensitive_link_target" && i["member"] == symlink_member
        && i["link_target"] == "../.env"));
}
fn audit_artifact(path: &Path) -> (serde_json::Value, Vec<serde_json::Value>) {
    let report = package_audit(path.parent().unwrap(), &[path.to_path_buf()]);
    (report.clone(), report["issues"].as_array().unwrap().clone())
}
pub(crate) fn run_tar_audit(entries: &[TarTestEntry<'_>]) -> (serde_json::Value, Vec<serde_json::Value>) {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    write_test_tar_entries(&artifact, entries);
    audit_artifact(&artifact)
}
pub(crate) fn run_tar_audit_from_names(names: &[&str]) -> (serde_json::Value, Vec<serde_json::Value>) {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.tar");
    write_test_tar(&artifact, names);
    audit_artifact(&artifact)
}
pub(crate) fn run_zip_audit(entries: &[ZipTestEntry<'_>]) -> (serde_json::Value, Vec<serde_json::Value>) {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(&artifact, entries);
    audit_artifact(&artifact)
}
pub(crate) fn read_zip_with_offsets(path: &Path) -> (Vec<u8>, usize, usize) {
    let bytes = fs::read(path).unwrap();
    let eocd = find_zip_eocd(&bytes).unwrap();
    let cd = zip_u32_at(&bytes, eocd + 16).unwrap() as usize;
    (bytes, eocd, cd)
}
pub(crate) struct ContractCase<'a> {
    pub label: &'a str,
    pub expect_codes: &'a [&'a str],
    pub expect_fields: &'a [(&'a str, &'a str)],
}
pub(crate) fn assert_contract(report: &serde_json::Value, issues: &[serde_json::Value], case: &ContractCase<'_>) {
    assert_audit_rejected(report);
    for code in case.expect_codes {
        assert!(issue_has_code(issues, code), "{}: missing code {code} in {issues:#?}", case.label);
    }
    if case.expect_fields.is_empty() == false {
        assert_issue(issues, case.expect_fields);
    }
}
pub(crate) fn run_tar_contract(entries: &[TarTestEntry<'_>], case: &ContractCase<'_>) {
    let (report, issues) = run_tar_audit(entries);
    assert_contract(&report, &issues, case);
}
pub(crate) fn run_zip_contract(entries: &[ZipTestEntry<'_>], case: &ContractCase<'_>) {
    let (report, issues) = run_zip_audit(entries);
    assert_contract(&report, &issues, case);
}
pub(crate) fn nested_private_in_zip(_outer_member: &str, nested_member: &str, build: impl FnOnce(&Path, &[u8]),) -> (serde_json::Value, Vec<serde_json::Value>) {
    let dir = tempdir().unwrap();
    let inner = dir.path().join("inner.tar");
    let artifact = dir.path().join("release.zip");
    write_test_tar(&inner, &[nested_member]);
    let inner_bytes = fs::read(&inner).unwrap();
    build(&artifact, &inner_bytes);
    audit_artifact(&artifact)
}
pub(crate) fn assert_nested_private(issues: &[serde_json::Value], outer_member: &str, nested_member: &str) {
    assert!(issues.iter().any(|i| i["code"] == "private_tool_state_member"
        && i["path"].as_str().is_some_and(|p| p.contains("release.zip!") && p.contains(outer_member))
        && i["member"] == nested_member));
}
fn write_named_and_audit(name: &str, build: impl FnOnce(&Path)) -> (serde_json::Value, Vec<serde_json::Value>) {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join(name);
    build(&artifact);
    audit_artifact(&artifact)
}
pub(crate) fn write_zip_and_audit(build: impl FnOnce(&Path)) -> (serde_json::Value, Vec<serde_json::Value>) {
    write_named_and_audit("release.zip", build)
}
pub(crate) fn write_tar_and_audit(build: impl FnOnce(&Path)) -> (serde_json::Value, Vec<serde_json::Value>) {
    write_named_and_audit("release.tar", build)
}
pub(crate) fn tamper_zip_data_descriptor(tamper: impl FnOnce(&mut Vec<u8>, &Path)) -> serde_json::Value {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("release.zip");
    write_test_zip(&artifact, &[ZipTestEntry::symlink("tokenzero-v0.1.1/bin/tokenzero-link", b"bin/tokenzero").with_data_descriptor()]);
    let mut bytes = fs::read(&artifact).unwrap();
    tamper(&mut bytes, &artifact);
    fs::write(&artifact, &bytes).unwrap();
    package_audit(dir.path(), &[artifact])
}
