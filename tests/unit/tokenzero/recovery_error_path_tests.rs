    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn expand_ref_check_row_records_spawn_failure_instead_of_skipping() {
        let cache = tempdir().unwrap();
        let row = expand_ref_check_row(
            Path::new("/definitely/not/a/tokenzero-expand-binary"),
            cache.path(),
            "combined",
            "tz://missing",
        )
        .expect("spawn failure must still produce a check row");
        assert_eq!(row["kind"], "combined");
        assert_eq!(row["ref"], "tz://missing");
        assert_eq!(row["expand_success"], false, "{row}");
        assert_eq!(row["byte_perfect"], false, "{row}");
        assert_eq!(row["bytes"], 0, "{row}");
        assert!(
            row["error"].as_str().is_some_and(|error| !error.is_empty()),
            "spawn error must be preserved: {row}"
        );
    }

    #[test]
    fn exact_expand_check_does_not_drop_unspawnable_refs() {
        let cache = tempdir().unwrap();
        let refs = vec![
            object!({"kind": "stdout", "ref": "tz://a"}),
            object!({"kind": "combined", "ref": "tz://b"}),
        ];
        let checks = exact_expand_check(
            Path::new("/definitely/not/a/tokenzero-expand-binary"),
            cache.path(),
            &refs,
        )
        .expect("failed expands are rows, not omitted");
        assert_eq!(checks.len(), 2, "{checks:?}");
        assert!(
            checks.iter().all(|row| row["byte_perfect"] == false),
            "unrecovered refs must fail the byte-perfect gate: {checks:?}"
        );
    }

