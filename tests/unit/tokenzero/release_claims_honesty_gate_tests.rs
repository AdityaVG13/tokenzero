    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn write_json(dir: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, serde_json::to_vec(value).expect("serialize fixture")).unwrap();
        path
    }

    fn reasons(gate: &Value) -> Vec<String> {
        gate["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|reason| reason.as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn recovery_gate_rejects_green_rows_with_wrong_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_json(
            dir.path(),
            "recovery.json",
            &json!({
                "schema_version": "not.a.real.schema",
                "ok": true,
                "normal_rows": [{"all_refs_recover": true}]
            }),
        );
        let gate = evaluate_recovery_claim_gate(Some(&path)).unwrap();
        assert_eq!(gate["pass"], false, "{gate}");
        assert!(
            reasons(&gate)
                .iter()
                .any(|reason| reason == "recovery artifact schema mismatch"),
            "{gate}"
        );
    }

    #[test]
    fn os_gate_rejects_approved_flag_without_all_release_oses() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_json(
            dir.path(),
            "os.json",
            &json!({
                "schema_version": "tokenzero.os_reach_audit.v1",
                "all_release_oses_run": false,
                "public_os_claim_approved": true
            }),
        );
        let gate = evaluate_os_claim_gate(Some(&path)).unwrap();
        assert_eq!(gate["pass"], false, "{gate}");
        assert!(
            reasons(&gate)
                .iter()
                .any(|reason| reason == "OS artifact set missing required release OS rows"),
            "{gate}"
        );
    }

    #[test]
    fn task_success_gate_rejects_wrong_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_json(
            dir.path(),
            "task.json",
            &json!({
                "schema_version": "tokenzero.wrong.v1",
                "ok": true,
                "critical_miss_rate": 0.0,
                "rows": [{"task_success": true}]
            }),
        );
        let gate = evaluate_task_success_claim_gate(Some(&path)).unwrap();
        assert_eq!(gate["pass"], false, "{gate}");
        assert!(
            reasons(&gate)
                .iter()
                .any(|reason| reason == "task-success artifact schema mismatch"),
            "{gate}"
        );
    }

