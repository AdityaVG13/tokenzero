use super::*;
use std::any::type_name;
use tempfile::tempdir;
use tokenzero_core::output_novelty::{OUTPUT_NOVELTY_SCHEMA, OutputNoveltyReceipt};

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

/// [SPEC-TZ-NOV-002] Recovery entity novelty is not core output novelty.
#[test]
fn entity_novelty_record_is_not_output_novelty_receipt() {
    assert_ne!(
        type_name::<EntityNoveltyRecord>(),
        type_name::<OutputNoveltyReceipt>(),
        "EntityNoveltyRecord must not be an alias of OutputNoveltyReceipt"
    );
    assert_eq!(ENTITY_NOVELTY_SCHEMA_VERSION, "zerostack.entity-novelty");
    assert_eq!(OUTPUT_NOVELTY_SCHEMA, "tokenzero.output-novelty/v1");
    assert_ne!(ENTITY_NOVELTY_SCHEMA_VERSION, OUTPUT_NOVELTY_SCHEMA);
    assert_eq!(ENTITY_NOVELTY_RECORD_TYPE, "entity-novelty");

    let record = EntityNoveltyRecord::empty("global", "tokenzero").unwrap();
    let entity_json = serde_json::to_value(&record).unwrap();
    let entity_obj = entity_json
        .as_object()
        .expect("EntityNoveltyRecord JSON is an object");
    assert_eq!(
        entity_obj["schema_version"].as_str().unwrap(),
        ENTITY_NOVELTY_SCHEMA_VERSION
    );
    assert_eq!(
        entity_obj["record_type"].as_str().unwrap(),
        ENTITY_NOVELTY_RECORD_TYPE
    );
    assert!(entity_obj.contains_key("entity_ids"));
    assert!(entity_obj.contains_key("producing_engine"));
    assert!(entity_obj.contains_key("scope_key"));
    for output_only in [
        "selection_origin",
        "classification_authority_digest",
        "selected_effect_digest",
        "verification_receipt_digest",
        "tokenizer_identity_digest",
        "encoding_digest",
        "total_encoded_bytes",
        "total_encoded_tokens",
        "fields",
        "totals",
    ] {
        assert!(
            !entity_obj.contains_key(output_only),
            "EntityNoveltyRecord JSON must not carry output-novelty field {output_only}"
        );
    }
    let dumped = serde_json::to_string(&entity_json).unwrap();
    assert!(
        !dumped.contains(OUTPUT_NOVELTY_SCHEMA),
        "entity novelty JSON must not carry the output-novelty schema id"
    );
}
