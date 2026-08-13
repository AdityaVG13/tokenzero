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
