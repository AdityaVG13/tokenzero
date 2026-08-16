    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_artifacts_publishes_complete_json_without_tmp_siblings() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("report.json");
        let report = object!({"ok": true, "schema_version": "tokenzero.write-atomic.v1"});
        write_artifacts(&dest, None, &report, "Atomic").unwrap();
        let parsed: Json = serde_json::from_slice(&fs::read(&dest).unwrap()).unwrap();
        assert_eq!(parsed["ok"], true);
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "report.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "atomic publish must rename the temp file away: {leftovers:?}"
        );
    }

    #[test]
    fn write_artifacts_second_publish_replaces_with_complete_json() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("report.json");
        write_artifacts(
            &dest,
            None,
            &object!({"ok": false, "n": 1}),
            "Atomic",
        )
        .unwrap();
        write_artifacts(&dest, None, &object!({"ok": true, "n": 2}), "Atomic").unwrap();
        let parsed: Json = serde_json::from_slice(&fs::read(&dest).unwrap()).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["n"], 2);
    }

