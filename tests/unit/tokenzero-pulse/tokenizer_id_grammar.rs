//! SPEC-TZ-TOK-002: Pulse tokenizer_id is estimator:<slug> or provider/model@hex.
//! Never treat the default as ExactTokenizerIdentity / Q99.

use tokenzero_pulse::PulseEvent;

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
    assert!(event.clone().with_tokenizer_id("Q99").is_err());
    assert!(event.clone().with_tokenizer_id("exact").is_err());
    assert!(event
        .with_tokenizer_id("EngineIdentity::TokenZero")
        .is_err());
}
