use super::*;
use tempfile::tempdir;

#[test]
fn same_length_spill_mutation_degrades_without_refs() {
    let dir = tempdir().unwrap();
    let spill = dir.path().join("stdout.log");
    let observed = "original";
    fs::write(&spill, observed).unwrap();
    let capture = StreamCapture {
        bytes_seen: observed.len(),
        captured_bytes: 3,
        truncated: true,
        captured_utf8_lossless: true,
        full_stream_sha256: Some(sha256_hex(observed)),
        spill_path: Some(spill.display().to_string()),
        spill_bytes: observed.len(),
    };
    fs::write(&spill, "mutation").unwrap();

    let error = exact_shell_stream_text("ori", &capture, "stdout", 1024).unwrap_err();
    assert!(error.contains("digest changed"), "{error}");
    let response = degraded_shell_response("probe", Mode::Auto, "preview", error);
    assert!(response.refs.is_empty());
    assert_eq!(
        response.telemetry.as_ref().unwrap()["transport_status"],
        "degraded"
    );
    assert!(response.safety.is_none());
}

#[test]
fn absent_observer_digest_never_authorizes_exact_recovery() {
    let capture = StreamCapture {
        bytes_seen: 3,
        captured_bytes: 3,
        truncated: false,
        captured_utf8_lossless: true,
        full_stream_sha256: None,
        spill_path: None,
        spill_bytes: 0,
    };
    let error = exact_shell_stream_text("abc", &capture, "stdout", 1024).unwrap_err();
    assert!(
        error.contains("omitted its observer-time digest"),
        "{error}"
    );
}

/// [SPEC-TZ-SH-001] Combined witness is a canonical stdout+stderr reconstruction.
/// It must not claim the process's temporal stdout/stderr interleaving.
#[cfg(not(windows))]
#[test]
fn combined_witness_does_not_claim_temporal_interleaving() {
    let dir = tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    let response = engine.shell(
        "printf 'out\\n'; printf 'err\\n' >&2",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok", "{response:?}");
    let safety = response
        .safety
        .as_ref()
        .expect("successful shell emits combined-witness safety");
    assert_eq!(
        safety["combined_witness_temporal_interleaving"],
        "not_claimed"
    );
    let capture_ref = response.telemetry.as_ref().unwrap()["capture_ref"]
        .as_str()
        .expect("successful shell emits capture_ref");
    let expanded = engine.expand(capture_ref, Some("raw"), None, None, None, None);
    let capture: serde_json::Value =
        serde_json::from_str(&expanded.visible.unwrap().text).expect("capture JSON");
    assert_eq!(
        capture["combined"]["kind"],
        "canonical_stdout_stderr_witness"
    );
    assert_eq!(capture["combined"]["temporal_interleaving_claimed"], false);
}
