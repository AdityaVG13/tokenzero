//! Progressive disclosure conformance (tokenzero-oyid).
//!
//! Search/list/read surfaces must return bounded snippets plus a tz/fz/gz ref.
//! Full bytes are allowed only on expand. Any registered domain op that returns
//! unbounded visible content without a recovery ref fails this test.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tempfile::tempdir;
use tokenzero_engine::{
    DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES, DispatchSurface, EngineConfig, TokenZeroEngine,
    all_domain_operations, dispatch_operation,
};

const MARKER: &str = "TZOYID-MARKER";
const LINE_COUNT: usize = 900;

fn unique_payload() -> String {
    let mut out = String::with_capacity(LINE_COUNT * 100);
    for i in 0..LINE_COUNT {
        let mix = (i as u128)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0xC0FF_EE);
        out.push_str(&format!("{MARKER} {i:04} {mix:080x}\n"));
    }
    out
}

fn engine_for(root: &Path) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.session_dedup = false;
    config.diff_reads = false;
    config.fetch_enabled = false;
    TokenZeroEngine::new(config)
}

fn has_recovery_ref(refs: &[String]) -> bool {
    refs.iter()
        .any(|r| r.starts_with("tz://") || r.starts_with("fz://") || r.starts_with("gz://"))
}

fn visible_text(value: &Value, response_text: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(text) = response_text {
        parts.push(text.to_string());
    }
    if let Some(text) = value.get("visible").and_then(Value::as_str) {
        parts.push(text.to_string());
    }
    parts.join("\n")
}

fn collected_refs(outcome: &tokenzero_engine::DispatchOutcome) -> Vec<String> {
    let mut refs = outcome.result.refs.clone();
    if let Some(response) = outcome.tool_response.as_ref() {
        refs.extend(response.refs.iter().map(|r| r.ref_id.clone()));
        if let Some(detail) = response.detail_ref.as_ref() {
            refs.push(detail.clone());
        }
    }
    refs
}

fn fixture_args(
    op: &str,
    root: &Path,
    large_path: &Path,
    payload: &str,
    expand_ref: &str,
) -> Value {
    let root_s = root.display().to_string();
    let path_s = large_path.display().to_string();
    match op {
        "tz_read" => json!({"path": path_s}),
        "tz_find" | "tz_grep" => json!({"query": MARKER, "path": root_s}),
        "tz_recall" => json!({"query": MARKER}),
        "tz_glob" => json!({"pattern": "*.txt", "path": root_s}),
        "tz_tree" => json!({"path": root_s, "depth": 2}),
        "tz_edit" => json!({
            "path": path_s,
            "edits": [{"find": format!("{MARKER} 0000"), "replace": format!("{MARKER} 0xxx")}],
            "dry_run": true
        }),
        "tz_shell" => {
            json!({"command": format!("/bin/cat {}", large_path.display()), "cwd": root_s})
        }
        "tz_ingest" => json!({"text": payload}),
        "tz_expand" => json!({"ref": expand_ref}),
        "tz_mem" => json!({}),
        "tz_cache_pack" => json!({"scope": "agent"}),
        "tz_rewrite" => json!({"command": "echo hi"}),
        "tz_discover" => json!({}),
        "tz_report_tool_issue" => json!({
            "tool": "zero_execute",
            "summary": "oyid progressive-disclosure probe"
        }),
        "tz_batch" => json!({"ops": [{"tool": "tz_read", "args": {"path": path_s}}]}),
        "tz_fetch" => json!({"url": "https://example.invalid/"}),
        _ => json!({}),
    }
}

fn is_expand_family(name: &str) -> bool {
    name == "tz_expand" || name.ends_with(".expand") || name.ends_with("multiExpand")
}

fn is_material_content_op(name: &str) -> bool {
    matches!(
        name,
        "tz_read"
            | "tz_find"
            | "tz_grep"
            | "tz_recall"
            | "tz_glob"
            | "tz_tree"
            | "tz_ingest"
            | "tz_shell"
            | "tz_batch"
            | "tz_edit"
    )
}

/// Every registered domain op that can emit file content must stay bounded
/// and carry a recovery ref; expand is the only full-byte path.
#[test]
fn tzoyid_registered_ops_do_not_return_unbounded_content_without_a_ref() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let payload = unique_payload();
    assert!(
        payload.len() > DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES,
        "fixture must exceed the exact-ref threshold ({} bytes), got {}",
        DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES,
        payload.len()
    );
    let large_path = root.join("oyid-corpus.txt");
    fs::write(&large_path, &payload).unwrap();
    fs::write(root.join("note.txt"), "oyid-sidecar\n").unwrap();

    let engine = engine_for(root);
    let seed = dispatch_operation(
        &engine,
        DispatchSurface::RawWorker,
        "tz_ingest",
        &json!({"text": payload}),
    );
    assert!(seed.is_ok(), "seed ingest must succeed: {seed:?}");
    let expand_ref = collected_refs(&seed)
        .into_iter()
        .find(|r| r.starts_with("tz://") || r.starts_with("fz://") || r.starts_with("gz://"))
        .expect("seed ingest must mint a recovery ref");

    let ops = all_domain_operations();
    assert!(
        !ops.is_empty(),
        "registry must expose at least one domain operation"
    );

    let mut driven = 0usize;
    for op in &ops {
        let args = fixture_args(op.name, root, &large_path, &payload, &expand_ref);
        let outcome = dispatch_operation(&engine, DispatchSurface::RawWorker, op.name, &args);
        driven += 1;

        if !outcome.is_ok() {
            // Fetch (disabled) and dummy control probes may fail. Discovery
            // and read surfaces that touch the fixture must succeed.
            let tool_err = outcome
                .tool_response
                .as_ref()
                .and_then(|r| r.error.as_ref())
                .map(|e| format!("{}: {}", e.code, e.message));
            assert!(
                !matches!(
                    op.name,
                    "tz_read" | "tz_find" | "tz_grep" | "tz_glob" | "tz_tree" | "tz_ingest"
                ),
                "{} failed: domain={:?} tool={tool_err:?}",
                op.name,
                outcome.domain_error.as_ref().map(|e| &e.message)
            );
            continue;
        }

        let response_text = outcome
            .tool_response
            .as_ref()
            .and_then(|r| r.visible.as_ref())
            .map(|v| v.text.as_str());
        let visible = visible_text(&outcome.result.value, response_text);
        let refs = collected_refs(&outcome);
        let recovered = has_recovery_ref(&refs) || has_recovery_ref(&[visible.clone()]);

        if is_expand_family(op.name) {
            assert!(
                visible.contains(payload.trim_end()) || visible.contains(&payload),
                "{} is the full-byte path and must recover the stored payload (visible {} bytes)",
                op.name,
                visible.len()
            );
            continue;
        }

        assert!(
            !visible.contains(payload.trim_end()),
            "{} inlined the full {}-byte fixture; full bytes are expand-only",
            op.name,
            payload.len()
        );
        assert!(
            visible.len() <= DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES,
            "{} returned {} visible bytes without staying under the {}-byte disclosure bound",
            op.name,
            visible.len(),
            DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES
        );

        if is_material_content_op(op.name) {
            assert!(
                recovered,
                "{} returned material content without a tz/fz/gz ref (visible {} bytes, refs={:?})",
                op.name,
                visible.len(),
                refs
            );
        }
    }

    assert_eq!(
        driven,
        ops.len(),
        "conformance must drive every registered domain op"
    );
}

/// Expand of a read/ingest ref is the only path that returns the fixture bytes.
#[test]
fn tzoyid_expand_is_the_full_byte_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let payload = unique_payload();
    fs::write(root.join("oyid-corpus.txt"), &payload).unwrap();
    let engine = engine_for(root);

    let read = dispatch_operation(
        &engine,
        DispatchSurface::RawWorker,
        "tz_read",
        &json!({"path": root.join("oyid-corpus.txt").display().to_string()}),
    );
    assert!(read.is_ok(), "read must succeed: {read:?}");
    let read_visible = read
        .tool_response
        .as_ref()
        .and_then(|r| r.visible.as_ref())
        .map(|v| v.text.as_str())
        .unwrap_or("");
    assert!(
        !read_visible.contains(payload.trim_end()),
        "read must not inline the full fixture"
    );
    let recovery = collected_refs(&read)
        .into_iter()
        .find(|r| r.starts_with("tz://") || r.starts_with("fz://") || r.starts_with("gz://"))
        .expect("read of a large file must mint a recovery ref");

    let expanded = dispatch_operation(
        &engine,
        DispatchSurface::RawWorker,
        "tz_expand",
        &json!({"ref": recovery}),
    );
    assert!(expanded.is_ok(), "expand must succeed: {expanded:?}");
    let expanded_visible = expanded
        .tool_response
        .as_ref()
        .and_then(|r| r.visible.as_ref())
        .map(|v| v.text.as_str())
        .unwrap_or("");
    assert!(
        expanded_visible.contains(payload.trim_end()),
        "expand must return the stored fixture bytes (got {} bytes)",
        expanded_visible.len()
    );
    let receipt = expanded
        .tool_response
        .as_ref()
        .and_then(|r| r.recovery.as_ref())
        .expect("expand must carry a terminal recovery receipt");
    assert!(receipt.terminal);
    assert!(receipt.do_not_recompact);
}
