use std::fs;

use tempfile::tempdir;

use super::*;
use tokenzero_core::model_artifacts::{AppendOnlyCapsuleSlots, ModelArtifactError, ModelCapsule};

fn stored_for(text: &str) -> StoredPayload {
    let digest = sha256_hex(text);
    StoredPayload {
        blob_ref: format!("tz://blob/{digest}"),
        file_ref: format!("tz://file/f{}", &digest[..16]),
        unit_refs: Vec::new(),
        raw_tokens: text.split_whitespace().count(),
        source_start_line: None,
        source_end_line: None,
    }
}

#[test]
fn engine_read_forms_capsule_with_matching_receipt_and_rejects_rewrites() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("note.txt");
    fs::write(&path, "alpha").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    config.capsule_exact_ref_threshold_bytes = 4096;
    let engine = TokenZeroEngine::new(config);

    // Full engine path: the read response carries the formed capsule ref.
    let response = engine.read(&[path.clone()], Mode::Auto, None, None, false, 1, 4000);
    assert!(response.error.is_none(), "{:?}", response.error);
    let capsule_ref = response
        .refs
        .iter()
        .find(|record| record.kind == "capsule")
        .expect("read response must carry the formed capsule ref");
    assert!(capsule_ref.ref_id.starts_with("tz://capsule/"));
    assert_eq!(capsule_ref.ref_id.len(), "tz://capsule/".len() + 64);
    assert_eq!(capsule_ref.bytes, "alpha".len());

    // Direct formation through the engine path: the receipt binds the exact
    // payload the capsule carries.
    let text = "alpha";
    let stored = stored_for(text);
    let plain = tokenzero_core::make_capsule(text, Mode::Auto, 4000, None).unwrap();
    let formed = TokenZeroEngine::form_read_model_capsule(
        &path,
        None,
        None,
        text.lines().count(),
        &stored,
        &plain.text,
        plain.visible_tokens,
    )
    .unwrap();
    let receipt = formed.formation_receipt();
    assert_eq!(
        receipt.constructor_identity,
        "tokenzero-engine.read-one-path.v1"
    );
    assert_eq!(receipt.epoch, 0);
    assert_eq!(
        receipt.payload_root,
        ModelCapsule::payload_digest(plain.text.as_bytes())
    );
    assert!(receipt.verify_payload(ModelCapsule::payload_digest(formed.render().as_slice())));
    assert_eq!(formed.render(), plain.text.as_bytes());
    assert_eq!(
        formed.causal_key().as_str(),
        format!("{}:1..{}", path.display(), text.lines().count())
    );

    // Append-never-rewrite: same causal key, different bytes -> fail loud.
    let mut slots = AppendOnlyCapsuleSlots::new();
    slots.record(&formed).unwrap();
    slots.record(&formed).unwrap(); // identical re-record is idempotent
    let text_b = "beta";
    let stored_b = stored_for(text_b);
    let plain_b = tokenzero_core::make_capsule(text_b, Mode::Auto, 4000, None).unwrap();
    let rewritten = TokenZeroEngine::form_read_model_capsule(
        &path,
        None,
        None,
        text_b.lines().count(),
        &stored_b,
        &plain_b.text,
        plain_b.visible_tokens,
    )
    .unwrap();
    assert!(matches!(
        slots.record(&rewritten),
        Err(ModelArtifactError::CapsuleCausalKeyRewrite { .. })
    ));
}
