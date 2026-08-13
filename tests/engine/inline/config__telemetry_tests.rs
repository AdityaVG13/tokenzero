use super::*;

#[test]
fn telemetry_env_is_strict_and_default_off() {
    for value in [
        None,
        Some(""),
        Some("0"),
        Some("false"),
        Some("off"),
        Some("no"),
        Some("invalid"),
    ] {
        assert!(!telemetry_env_enabled(value));
    }
    for value in ["1", "ON", " true ", "Yes"] {
        assert!(telemetry_env_enabled(Some(value)));
    }
}

#[test]
fn telemetry_precedence_is_explicit_and_deterministic() {
    assert!(resolve_telemetry(true, false, Some(false), Some("off")));
    assert!(!resolve_telemetry(true, true, Some(true), Some("yes")));
    assert!(resolve_telemetry(false, false, Some(true), Some("off")));
    assert!(!resolve_telemetry(false, false, Some(false), Some("yes")));
    assert!(resolve_telemetry(false, false, None, Some("yes")));
    assert!(!resolve_telemetry(false, false, None, None));
}

#[test]
fn capsule_exact_ref_threshold_is_configurable_and_rejects_zero() {
    assert_eq!(DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES, 40_960);
    assert_eq!(
        capsule_exact_ref_threshold(None),
        DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES
    );
    assert_eq!(capsule_exact_ref_threshold(Some("1024")), 1024);
    assert_eq!(
        capsule_exact_ref_threshold(Some("0")),
        DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES
    );
    assert_eq!(
        capsule_exact_ref_threshold(Some("invalid")),
        DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES
    );
}

/// A sub-second deadline must stay sub-second. Routing milliseconds through
/// the seconds path floored 300ms to 0 and clamped it back to 1s, handing
/// the caller a bound 3x larger than the one they asked for.
#[test]
fn millis_timeout_preserves_sub_second_deadlines() {
    assert_eq!(
        shell_timeout_from_millis(Some(300)),
        Duration::from_millis(300)
    );
    assert_eq!(shell_timeout_from_millis(Some(1)), Duration::from_millis(1));
    assert_eq!(
        shell_timeout_from_millis(Some(1_500)),
        Duration::from_millis(1_500)
    );
}

#[test]
fn millis_and_secs_agree_on_equivalent_requests() {
    assert_eq!(
        shell_timeout_from_millis(Some(2_000)),
        shell_timeout_from_secs(Some(2))
    );
}

/// Zero is not "no timeout"; it is an unusable request. It must not silently
/// disable the deadline.
#[test]
fn millis_timeout_rejects_disabling_values() {
    assert_eq!(shell_timeout_from_millis(Some(0)), Duration::from_millis(1));
}

#[test]
fn millis_timeout_clamps_to_the_same_ceiling_as_secs() {
    let absurd = MAX_SHELL_TIMEOUT_SECS * 1_000 * 10;
    assert_eq!(
        shell_timeout_from_millis(Some(absurd)),
        Duration::from_secs(MAX_SHELL_TIMEOUT_SECS)
    );
}

#[test]
fn millis_timeout_defaults_when_absent() {
    assert_eq!(
        shell_timeout_from_millis(None),
        Duration::from_secs(DEFAULT_SHELL_TIMEOUT_SECS)
    );
}
