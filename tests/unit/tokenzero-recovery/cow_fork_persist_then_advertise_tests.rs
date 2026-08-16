    use super::*;
    use crate::context_view::{AsOf, ContextProjection};
    use tempfile::tempdir;

    fn breakpoint_projection(rendered: &str) -> ContextProjection {
        ContextProjection {
            rendered: rendered.into(),
            stable_prefix: rendered.into(),
            stable_prefix_sha256: sha256_hex(rendered),
            stable_prefix_tokens: 1,
            input_tokens: 1,
            working_set_tokens: 0,
            working_set_ids: Vec::new(),
            hot_tail_ids: Vec::new(),
            evicted_ids: Vec::new(),
            as_of: Some(AsOf::Turn(1)),
            cache_breakpoint: true,
        }
    }

    #[test]
    fn append_persists_novelty_ref_before_returning_it() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery.json");
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let mut session =
            CowSession::from_breakpoint("root", &breakpoint_projection("SYSTEM\n")).unwrap();
        let recovery_ref = session.append(&mut store, "user: alpha\n").unwrap();
        drop(store);

        let mut restarted = RecoveryStore::new(Some(cache));
        let expanded = restarted.expand(&recovery_ref, Some("raw"), None, None, None, None);
        assert!(
            expanded.found,
            "append must persist before advertising: {}",
            expanded.reason
        );
        assert_eq!(expanded.content, "user: alpha\n");
    }

