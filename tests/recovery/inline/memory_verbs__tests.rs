use super::*;

#[test]
fn tzfmeo_six_verbs_name_a_substrate_and_do_not_apply() {
    let names: Vec<_> = MemoryVerb::ALL.iter().map(|v| v.as_str()).collect();
    assert_eq!(
        names,
        [
            "store",
            "commit_session",
            "update_capsule",
            "forget_visible",
            "promote_anchor",
            "link_refs"
        ]
    );
    for verb in MemoryVerb::ALL {
        assert!(!verb.substrate_target().is_empty(), "{verb:?}");
        let effect = describe_memory_verb(&MemoryVerbRequest {
            verb,
            ref_ids: vec!["tz://blob/deadbeef".into()],
            payload: None,
            label: None,
        });
        assert!(!effect.applied, "{verb:?} stub must not apply");
        assert_eq!(effect.substrate, verb.substrate_target());
    }
}
