use tokenzero_engine::{
    AmplificationRecord, ExecutionPath, OperationClass, TA_REGISTRY,
    record_operation_amplification, replay_ta_table,
};

#[test]
fn registry_covers_every_operation_class() {
    let classes = [
        OperationClass::Read, OperationClass::Search, OperationClass::Mutate,
        OperationClass::Shell, OperationClass::Expand, OperationClass::Compact,
        OperationClass::Plan, OperationClass::Other,
    ];
    assert_eq!(TA_REGISTRY.len(), classes.len());
    for class in classes {
        assert!(TA_REGISTRY.iter().any(|(registered, bound)| *registered == class && *bound > 0));
    }
}

#[test]
fn replay_corpus_emits_bounded_per_class_table() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/token-amplification-replay.json"
    )).unwrap();
    let records: Vec<AmplificationRecord> = serde_json::from_value(value["cases"].clone()).unwrap();
    let table = replay_ta_table(&records);
    assert_eq!(table.len(), 3);
    assert!(table.iter().all(|row| row.samples > 0 && row.within_bound));
}

#[test]
fn replay_keeps_billed_and_cached_counts_for_both_directions() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/token-amplification-replay.json"
    )).unwrap();
    let records: Vec<AmplificationRecord> = serde_json::from_value(value["cases"].clone()).unwrap();
    assert_eq!(records[0].input.billed, 8);
    assert_eq!(records[2].output.cached, 4);
    assert_eq!(records[2].floor_tokens, 24);
    assert_eq!(records[2].amplification_milli, 1250);
}
