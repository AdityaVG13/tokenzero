    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_unlinks_tmp_when_rename_fails() {
        let dir = tempdir().unwrap();
        let record = EntityNoveltyRecord::empty("global", "tokenzero").unwrap();
        let path = entity_novelty_path(dir.path(), "global");
        fs::create_dir_all(&path).unwrap();
        write_entity_novelty(dir.path(), &record).expect_err("rename onto directory");
        let tmp = path.with_extension("json.tmp");
        assert!(
            !tmp.exists(),
            "failed novelty write must unlink tmp {}",
            tmp.display()
        );
    }

