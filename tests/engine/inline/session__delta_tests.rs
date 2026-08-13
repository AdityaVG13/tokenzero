use super::*;
#[test]
fn telemetry_reports_watermark_and_wire_bytes() {
    let mut summary = SessionSummary::default();
    summary.note_wire_bytes(240, 32);
    summary.set_watermark(7, 8);
    let telemetry = summary.telemetry().expect("delta telemetry");
    assert_eq!(telemetry["session_delta"]["from_hwm"], 7);
    assert_eq!(telemetry["session_delta"]["to_hwm"], 8);
    assert_eq!(telemetry["session_delta"]["full_bytes"], 240);
    assert_eq!(telemetry["session_delta"]["delta_bytes"], 32);
    assert_eq!(telemetry["session_delta"]["saved_bytes"], 208);
}
#[test]
fn watermark_is_monotonic() {
    let mut memory = SessionMemory::default();
    assert_eq!(memory.advance_hwm(), (0, 1));
    assert_eq!(memory.advance_hwm(), (1, 2));
    assert_eq!(memory.session_hwm(), 2);
}
