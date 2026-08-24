//! SPEC-TZ-TOK-002: Pulse tokenizer_id is estimator:<slug>, tiktoken:<encoding>,
//! or provider/model@hex. Never treat the default or tiktoken: as
//! ExactTokenizerIdentity / Q99.

use tokenzero_pulse::{PulseEvent, record_event, report_for_path};

#[test]
fn default_tool_call_is_labelled_estimator_not_exact() {
    let event = PulseEvent::tool_call("read", "auto", 1, 1, 0, 0, 0, None);
    assert_eq!(event.tokenizer_id, "estimator:tokenzero-core");
    assert!(event.tokenizer_id.starts_with("estimator:"));
}

#[test]
fn tokenizer_id_grammar_accepts_estimator_and_digest_rejects_q99() {
    let event = PulseEvent::tool_call("read", "auto", 1, 1, 0, 0, 0, None);
    event
        .clone()
        .with_tokenizer_id("estimator:bytes-ceil-div4")
        .expect("labelled estimator");
    let digest = "a".repeat(64);
    event
        .clone()
        .with_tokenizer_id(&format!("openai/gpt-4@{digest}"))
        .expect("provider/model@hex");
    event
        .clone()
        .with_tokenizer_id("tiktoken:o200k_base")
        .expect("labelled bundled BPE");
    event
        .clone()
        .with_tokenizer_id("tiktoken:cl100k_base")
        .expect("cl100k");
    assert!(event.clone().with_tokenizer_id("tiktoken:").is_err());
    assert!(event.clone().with_tokenizer_id("Q99").is_err());
    assert!(event.clone().with_tokenizer_id("exact").is_err());
    assert!(event.clone().with_tokenizer_id("gpt-4o").is_err());
    assert!(
        event
            .with_tokenizer_id("EngineIdentity::TokenZero")
            .is_err()
    );
}

#[test]
fn tiktoken_label_is_not_exact_tokenizer_identity() {
    let id = "tiktoken:o200k_base";
    PulseEvent::tool_call("measure", "auto", 1, 1, 0, 0, 0, None)
        .with_tokenizer_id(id)
        .expect("Pulse must accept the kernel tiktoken: class");
    assert!(id.starts_with("tiktoken:"));
    assert!(!id.contains('@'), "tiktoken: is not provider/model@hex");
    assert!(!id.starts_with("estimator:"));
}

#[test]
fn pulse_report_spent_above_raw_is_negative_savings_not_a_clamped_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("events.jsonl");
    let event = PulseEvent::tool_call("compress", "auto", 10, 15, 0, 0, 0, None);
    assert_eq!(event.tokenizer_id, "estimator:tokenzero-core");
    record_event(&path, &event).expect("record");
    let report = report_for_path(&path).expect("report");
    assert!(
        report.visible_savings < 0.0,
        "spent>raw must not clamp to a 0% save, got {}",
        report.visible_savings
    );
    assert!((report.visible_savings - (-0.5)).abs() < 1e-12);
    assert!(report.recovery_adjusted_savings < 0.0);
}
