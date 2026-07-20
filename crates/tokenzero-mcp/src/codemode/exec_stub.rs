//! CodeMode JS sandbox stub when `surface-codemode` is not compiled (tokenzero-mcp-only).
//!
//! Recipe/JSON discovery still compiles; pure JS plan execution fails closed with a
//! precise diagnostic instead of linking rquickjs.

use super::result::{CodeModeOptions, CodeModeResult};
use serde_json::Value;

pub fn execute_codemode(plan: &str) -> CodeModeResult {
    execute_codemode_with_options(plan, CodeModeOptions::default())
}

pub fn execute_codemode_with_options(plan: &str, _options: CodeModeOptions) -> CodeModeResult {
    let _ = plan;
    CodeModeResult::error_with_kind(
        "unavailable",
        "CodeMode JavaScript sandbox was not compiled into this artifact \
(missing feature surface-codemode / rquickjs). Install tokenzero-codemode, or \
use the CodeMode catalog surface package. tokenzero-mcp never embeds the JS runtime.",
        0,
        false,
    )
}

/// No-op telemetry helper when JS sandbox is absent.
pub(crate) fn record_exact_expand_payload(_text: &str) {}

/// Expand-value detection is JS-runtime-specific; stub always returns false.
pub(crate) fn is_exact_expand_value(_value: &Value) -> bool {
    false
}
