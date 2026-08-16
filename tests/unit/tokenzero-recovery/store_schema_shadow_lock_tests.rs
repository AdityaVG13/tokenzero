    use super::*;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn concurrent_shadow_appends_keep_every_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shadow.jsonl");
        let workers = 4;
        let per_worker = 32;
        let mut handles = Vec::new();
        for worker in 0..workers {
            let path = path.clone();
            handles.push(thread::spawn(move || {
                for idx in 0..per_worker {
                    append_shadow_jsonl(&path, &format!("{worker}:{idx}")).unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines = text.lines().filter(|line| !line.is_empty()).count();
        assert_eq!(
            lines,
            workers * per_worker,
            "ring trim under lock must not drop concurrent appends"
        );
    }

    #[test]
    fn write_segment_unlinks_sidecars_when_rename_fails() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("entry.json");
        fs::create_dir_all(&dest).unwrap();
        let err = write_actioncache_segment(&dest, b"{}").expect_err("rename onto directory");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name();
                let name = name.to_string_lossy();
                (name.ends_with(".tmp") || name.ends_with(".commit")).then(|| name.into_owned())
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "failed segment write must unlink tmp/commit ({err}): {leftovers:?}"
        );
    }

