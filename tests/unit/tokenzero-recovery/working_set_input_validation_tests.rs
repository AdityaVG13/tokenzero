    use super::*;
    use crate::RecoveryStore;
    use tempfile::tempdir;

    #[test]
    fn empty_bodies_are_zero_lines_not_a_phantom_line() {
        assert_eq!(
            delta_between("", ""),
            WorkingSetDelta {
                start_line: 1,
                removed: Vec::new(),
                inserted: Vec::new(),
            },
            "empty-to-empty must not start at line 2 from split_inclusive's remainder"
        );
        assert_eq!(
            integrate_delta("", &delta_between("", "")).as_deref(),
            Some("")
        );
        assert_eq!(
            integrate_delta("", &delta_between("", "a\n")).as_deref(),
            Some("a\n")
        );
        assert_eq!(
            integrate_delta("a", &delta_between("a", "")).as_deref(),
            Some("")
        );
        assert_eq!(
            split_exact_lines(""),
            Vec::<String>::new(),
            "0-byte working-set body has 0 lines"
        );
        assert_eq!(split_exact_lines("a"), vec!["a".to_string()]);
        assert_eq!(split_exact_lines("a\n"), vec!["a\n".to_string()]);
        assert_eq!(
            integrate_delta("a\nb", &delta_between("a\nb", "a\nc")).as_deref(),
            Some("a\nc"),
            "last line without terminator is an inclusive 1-based line"
        );
        assert_eq!(
            integrate_delta(
                "a",
                &WorkingSetDelta {
                    start_line: 0,
                    removed: Vec::new(),
                    inserted: Vec::new(),
                }
            ),
            None,
            "line 0 is not a valid inclusive window"
        );
    }

    #[test]
    fn admit_refuses_zero_start_line() {
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut set = WorkingSet::new(8192);
        let err = set
            .admit(
                &mut store,
                "body".into(),
                SpanAnchor {
                    path: PathBuf::from("src/a.rs"),
                    symbol: None,
                    start_line: 0,
                    end_line: 1,
                },
            )
            .expect_err("start_line 0");
        assert!(
            err.to_string().contains("invalid span line window"),
            "{err}"
        );
    }

    #[test]
    fn prefetch_hint_queue_is_bounded() {
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut set = WorkingSet::new(8192);
        set.register_prefetch_hook(Box::new(SameFileNeighborPrefetch));
        let mut ids = Vec::new();
        for i in 0..6 {
            let admission = set
                .admit(
                    &mut store,
                    format!("span-body-{i} more tokens for eviction\n"),
                    SpanAnchor {
                        path: PathBuf::from("same.rs"),
                        symbol: None,
                        start_line: i * 2 + 1,
                        end_line: i * 2 + 1,
                    },
                )
                .unwrap();
            ids.push(admission.id);
        }
        for id in &ids[1..] {
            set.evict(&mut store, *id)
                .unwrap()
                .expect("resident span must page out");
        }
        let refs: Vec<String> = set.evicted_refs().keys().cloned().collect();
        assert!(
            !refs.is_empty(),
            "explicit evict must populate prefetch candidates"
        );
        for _ in 0..40 {
            for id in &ids[1..] {
                let _ = set.evict(&mut store, *id);
            }
            let live_refs: Vec<String> = set.evicted_refs().keys().cloned().collect();
            for ref_id in &live_refs {
                let _ = set.rehydrate_ref(&mut store, ref_id, None, None);
            }
        }
        assert!(
            set.take_prefetch_hints().len() <= MAX_QUEUED_PREFETCH_HINTS,
            "undrained prefetch hints must not grow without bound"
        );
    }

    #[test]
    fn link_refs_survives_store_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("recovery-cache.json");
        let mut store = RecoveryStore::new(Some(path.clone()));
        let mut set = WorkingSet::new(8192);
        let body = "link-persist-body\nsecond line\n";
        let admission = set
            .admit(
                &mut store,
                body.into(),
                SpanAnchor {
                    path: PathBuf::from("src/link.rs"),
                    symbol: None,
                    start_line: 1,
                    end_line: 2,
                },
            )
            .unwrap();
        let source = set
            .evict(&mut store, admission.id)
            .unwrap()
            .expect("page out")
            .ref_id;
        let alias = "tz://s/0123456789abcdef";
        assert!(set.link_refs(&mut store, &source, alias).unwrap());

        let mut reopened = RecoveryStore::new(Some(path));
        let expanded = reopened.expand(alias, Some("raw"), None, None, None, None);
        assert!(expanded.found, "{}", expanded.reason);
        assert_eq!(expanded.content, body);
    }

    #[test]
    fn rehydrate_owned_ref_fails_loud_when_expand_misses() {
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut set = WorkingSet::new(8192);
        let admission = set
            .admit(
                &mut store,
                "only one line\n".into(),
                SpanAnchor {
                    path: PathBuf::from("src/one.rs"),
                    symbol: None,
                    start_line: 1,
                    end_line: 1,
                },
            )
            .unwrap();
        let source = set
            .evict(&mut store, admission.id)
            .unwrap()
            .expect("page out")
            .ref_id;
        let err = set
            .rehydrate_ref(&mut store, &source, Some(99), Some(99))
            .expect_err("owned-ref expand miss must not look like an unknown ref");
        assert!(
            err.to_string().contains("window-out-of-range")
                || err.to_string().contains("rehydrate of owned ref"),
            "{err}"
        );
    }

    #[test]
    fn rehydrate_fragment_window_wins_over_start_line_args() {
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut set = WorkingSet::new(8192);
        let body = "line-one\nline-two\nline-three\n";
        let admission = set
            .admit(
                &mut store,
                body.into(),
                SpanAnchor {
                    path: PathBuf::from("src/win.rs"),
                    symbol: None,
                    start_line: 10,
                    end_line: 12,
                },
            )
            .unwrap();
        let source = set
            .evict(&mut store, admission.id)
            .unwrap()
            .expect("page out")
            .ref_id;
        let fault = set
            .rehydrate_ref(&mut store, &format!("{source}#L1-1"), Some(2), Some(2))
            .unwrap()
            .expect("fragment window must rehydrate");
        assert!(fault.partial);
        assert_eq!(fault.anchor.start_line, 10);
        assert_eq!(fault.anchor.end_line, 10);
        assert_eq!(set.visible_lines().last().copied(), Some("line-one\n"));
    }

