use std::fs;

use tempfile::tempdir;

#[test]
fn reach_report_keeps_local_only_contract() {
    let root = tempdir().unwrap();
    let report = crate::reach::run_reach(root.path().to_path_buf(), None).unwrap();

    assert_eq!(report["schema_version"], "tokenzero.reach.v1");
    assert_eq!(report["ok"], true);
    assert_eq!(report["daemon_required"], false);
    assert_eq!(report["global_writes"], false);
    assert_eq!(report["installed_wrapper_audit"]["daemon_required"], false);
    assert_eq!(report["installed_wrapper_audit"]["global_writes"], false);
    assert!(
        report["rows"]
            .as_array()
            .is_some_and(|rows| rows.len() == 7)
    );
}

#[test]
fn installed_wrapper_audit_reports_command_surface() {
    let report = crate::reach::installed_tokenzero_command_audit();

    assert_eq!(
        report["schema_version"],
        "tokenzero.installed_wrapper_audit.v1"
    );
    assert_eq!(report["command"], "tokenzero");
    assert!(report["candidate_count"].as_u64().is_some());
    assert!(report["candidate_paths"].as_array().is_some());
    assert_eq!(report["daemon_required"], false);
    assert_eq!(report["global_writes"], false);
}

#[test]
fn same_path_for_audit_matches_identical_existing_path() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("tokenzero");
    fs::write(&file, b"#!/bin/sh\n").unwrap();

    assert!(crate::reach::same_path_for_audit(&file, &file));
}

#[test]
fn reach_does_not_import_cli_monolith() {
    let source = include_str!("../reach.rs");
    let forbidden_imports = [
        format!("use {}::", "super"),
        format!("{}::", "super"),
        format!("use crate::{}", "main"),
        format!("crate::{}::", "main"),
        format!("crate::{}", "Cli"),
        format!("crate::{}", "Commands"),
    ];
    for forbidden in forbidden_imports {
        assert!(
            !source.contains(&forbidden),
            "reach.rs must not back-import the CLI monolith: {forbidden}"
        );
    }
}
