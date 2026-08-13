use super::*;
use tempfile::tempdir;

#[test]
fn refuses_second_entity_namespace() {
    assert!(matches!(
        parse_entity_ref(
            "tz://entity/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ),
        Err(NoveltyError::ForbiddenScheme(_))
    ));
    assert!(matches!(
        parse_entity_ref(
            "fz://entity/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ),
        Err(NoveltyError::ForbiddenScheme(_))
    ));
    let id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(parse_entity_ref(&format!("gz://entity/{id}")).unwrap(), id);
    assert_eq!(
        EntityNoveltyRecord::entity_ref(id).unwrap(),
        format!("gz://entity/{id}")
    );
}

#[test]
fn merge_round_trip_on_shared_store_path() {
    let dir = tempdir().unwrap();
    let id_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
    let id_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
    let scope = "session:fusion";
    let written = merge_entity_novelty(
        dir.path(),
        scope,
        std::slice::from_ref(&id_a),
        "graphzero",
        Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"),
    )
    .unwrap();
    assert!(written.knows(&id_a));
    assert_eq!(
        written.cas_digest.as_deref(),
        Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
    );

    let merged = merge_entity_novelty(
        dir.path(),
        scope,
        std::slice::from_ref(&id_b),
        "tokenzero",
        None,
    )
    .unwrap();
    assert!(merged.knows(&id_a));
    assert!(merged.knows(&id_b));
    assert_eq!(merged.producing_engine, "tokenzero");

    let loaded = read_entity_novelty(dir.path(), scope).unwrap();
    assert_eq!(loaded.entity_ids, vec![id_a, id_b]);
}

#[test]
fn rejects_scheme_prefix_in_entity_ids() {
    let mut record = EntityNoveltyRecord::empty("global", "tokenzero").unwrap();
    let err = record
        .merge_ids(
            ["tz://entity/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"],
            "tokenzero",
        )
        .unwrap_err();
    assert!(matches!(err, NoveltyError::EntityId(_)));
}
