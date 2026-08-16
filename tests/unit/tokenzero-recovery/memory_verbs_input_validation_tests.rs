    use super::*;
    use crate::RecoveryStore;
    use crate::working_set::WorkingSet;

    #[test]
    fn store_refuses_empty_payload_and_label() {
        let mut store = RecoveryStore::new(None);
        let mut set = WorkingSet::new(8192);
        let mut req = MemoryVerbRequest {
            verb: MemoryVerb::Store,
            ref_ids: Vec::new(),
            payload: Some(String::new()),
            label: Some("src/a.rs".into()),
        };
        match apply_memory_verb(&mut set, &mut store, &req) {
            Err(MemoryVerbError::NotApplied { reason, .. }) => {
                assert!(reason.contains("non-empty"), "{reason}");
            }
            other => panic!("expected NotApplied, got {other:?}"),
        }
        req.payload = Some("body".into());
        req.label = Some(String::new());
        match apply_memory_verb(&mut set, &mut store, &req) {
            Err(MemoryVerbError::NotApplied { reason, .. }) => {
                assert!(reason.contains("non-empty"), "{reason}");
            }
            other => panic!("expected NotApplied, got {other:?}"),
        }
    }

    #[test]
    fn link_refs_refuses_empty_ids() {
        let mut store = RecoveryStore::new(None);
        let mut set = WorkingSet::new(8192);
        let req = MemoryVerbRequest {
            verb: MemoryVerb::LinkRefs,
            ref_ids: vec![String::new(), "tz://blob/alias".into()],
            payload: None,
            label: None,
        };
        match apply_memory_verb(&mut set, &mut store, &req) {
            Err(MemoryVerbError::NotApplied { reason, .. }) => {
                assert!(reason.contains("non-empty"), "{reason}");
            }
            other => panic!("expected NotApplied, got {other:?}"),
        }
    }

