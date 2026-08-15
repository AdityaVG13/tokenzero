use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use tempfile::tempdir;
use tokenzero_core::{
    Accounting, ChannelSeparation, ContentType, Diagnostic, Mode, RecoveryReceipt, RefRecord,
    ToolResponse,
};
use tokenzero_recovery::{ExpansionResult, RecoveryStore};

use super::{
    EngineConfig, LocalPayloadPolicy, RecoveryStoreLease, TokenZeroEngine, expansion_response,
    local_payload_policy, path_not_allowed, preview, render_text, render_text_with_complete_read,
    rewrite_full_refs_if_strictly_cheaper,
};

struct ColdStorePause {
    entered: Barrier,
    release: Barrier,
}

static COLD_STORE_PAUSE: Mutex<Option<Arc<ColdStorePause>>> = Mutex::new(None);

pub(super) fn pause_during_cold_store() {
    let pause = COLD_STORE_PAUSE
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(pause) = pause {
        pause.entered.wait();
        pause.release.wait();
    }
}

#[test]
fn recovery_store_drops_slot_before_cold_construct() {
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = dir.path().join("recovery.json");
    let engine = Arc::new(TokenZeroEngine::new(config));
    let _busy = engine.recovery_store();
    let pause = Arc::new(ColdStorePause {
        entered: Barrier::new(2),
        release: Barrier::new(2),
    });
    *COLD_STORE_PAUSE.lock().unwrap() = Some(Arc::clone(&pause));

    let worker_engine = Arc::clone(&engine);
    let worker = thread::spawn(move || {
        let _lease = worker_engine.recovery_store();
    });
    pause.entered.wait();
    let slot_free = engine
        .recovery_store
        .as_ref()
        .expect("long-lived engine owns a store slot")
        .try_lock()
        .is_ok();
    pause.release.wait();
    worker.join().unwrap();
    assert!(
        slot_free,
        "recovery_store occupancy mutex must not stay held across RecoveryStore::new I/O"
    );
}

#[test]
fn shared_recovery_store_lease_returns_its_store_without_optional_state() {
    let slot = Mutex::new(None);
    {
        let mut lease = RecoveryStoreLease::Shared {
            store: RecoveryStore::new(None),
            slot: &slot,
        };
        lease.recovery_count = 7;
    }
    let available = slot.lock().unwrap();
    assert_eq!(
        available.as_ref().map(|store| store.recovery_count),
        Some(7)
    );
}

#[test]
fn capsule_payload_policy_respects_threshold_boundaries_and_modes() {
    let threshold = 1024;
    let fixtures = [
        (threshold - 1, Mode::Auto, true, LocalPayloadPolicy::Inline),
        (threshold, Mode::Auto, true, LocalPayloadPolicy::Inline),
        (
            threshold + 1,
            Mode::Auto,
            true,
            LocalPayloadPolicy::ExactRef,
        ),
        (threshold + 1, Mode::Auto, false, LocalPayloadPolicy::Inline),
        (
            threshold + 1,
            Mode::Passthrough,
            true,
            LocalPayloadPolicy::Inline,
        ),
        (threshold + 1, Mode::Exact, true, LocalPayloadPolicy::Inline),
    ];
    for (bytes, mode, exact_ref_available, expected) in fixtures {
        assert_eq!(
            local_payload_policy(bytes, threshold, mode, exact_ref_available),
            expected,
            "bytes={bytes} mode={mode:?} exact_ref_available={exact_ref_available}"
        );
    }
}

#[test]
fn expansion_response_reports_clamped_window_metadata() {
    let mut result = ExpansionResult::ok(
        "tz://blob/test#L1-L200".to_string(),
        Some("raw".to_string()),
        "a
b
"
        .to_string(),
    );
    result.clamped = true;
    result.returned_start_line = Some(1);
    result.returned_end_line = Some(2);
    result.line_count = Some(2);

    let response = expansion_response(result, 0);
    let window = &response.telemetry.as_ref().unwrap()["window"];
    assert_eq!(window["clamped"], true);
    assert_eq!(window["start_line"], 1);
    assert_eq!(window["end_line"], 2);
    assert_eq!(window["line_count"], 2);
}

#[test]
fn expansion_response_maps_fragment_failures_to_typed_codes() {
    let cases = [
        ("fragment-malformed", "fragment_malformed"),
        ("fragment-reversed", "fragment_reversed"),
        (
            "fragment-out-of-range; start=0 end=99 len=4",
            "fragment_out_of_range",
        ),
        (
            "fragment-not-utf8-boundary; start=1 end=3 len=4",
            "fragment_not_utf8_boundary",
        ),
        ("non_utf8_line_fragment", "fragment_not_utf8_boundary"),
        ("fragment-unknown-kind", "fragment_unknown_kind"),
        ("fragment-duplicate", "fragment_duplicate"),
    ];
    for (reason, code) in cases {
        let result = ExpansionResult::missing(
            "tz://blob/test#B0+99".to_string(),
            Some("raw".to_string()),
            reason,
        );
        let response = expansion_response(result, 0);
        let error = response.error.as_ref().expect(reason);
        assert_eq!(error.code, code, "reason {reason}");
        assert!(
            error.message.contains(reason.split(';').next().unwrap()),
            "message keeps the typed reason detail: {}",
            error.message
        );
        assert!(
            error
                .repair
                .as_deref()
                .is_some_and(|repair| repair.contains("fragment")),
            "fragment repair hint: {:?}",
            error.repair
        );
    }
}

#[test]
fn expansion_response_preserves_typed_misses_for_ledger() {
    for (reason, code, dangling) in [
        ("dangling-ref", "dangling_ref", 1),
        ("stale-ref", "ref_stale", 0),
        ("ref-not-found", "ref_not_found", 0),
    ] {
        let result =
            ExpansionResult::missing("tz://o/7/23".to_owned(), Some("raw".to_owned()), reason);
        let response = expansion_response(result, 0);
        assert_eq!(response.error.as_ref().unwrap().code, code);
        let expand = &response.telemetry.as_ref().unwrap()["expand"];
        assert_eq!(expand["fail_count"], 1);
        assert_eq!(expand["dangling_ref_count"], dangling);
        assert_eq!(expand["miss_kind"], code);
    }
}

#[test]
fn session_alias_rewrites_every_ref_field_and_survives_restart() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery.json");
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache.clone();
    let engine = TokenZeroEngine::new(config);
    let full_ref = engine
        .recovery_store()
        .store_blob("exact payload", ContentType::Unknown)
        .unwrap();
    let mut response = ToolResponse::ok(
        "read",
        Mode::Auto,
        format!("visible {full_ref}"),
        vec![RefRecord {
            kind: "blob".into(),
            ref_id: full_ref.clone(),
            bytes: 13,
            live: true,
        }],
        Accounting {
            raw_tokens: 3,
            visible_tokens: 3,
            recovery_tokens: 0,
            billed_tokens: 3,
            cached_tokens: 0,
            exact_ref_tokens: None,
        },
    );
    response.telemetry = Some(serde_json::json!({"ref": full_ref}));
    response.safety = Some(serde_json::json!({"anchor": full_ref}));
    response.channels = Some(ChannelSeparation {
        action: "read".into(),
        status_line: format!("ref={full_ref}"),
        user_message: Some(format!("expand {full_ref}")),
    });

    // The default lexical gauge counts an ordinal (8 tokens) as costlier
    // than a full blob ref (6 tokens), so the accepted path exercises the
    // same engine flow with an injected char-count meter (74-char blob
    // ref vs ~11-char ordinal) through the real persist/all-fields flow.
    engine.apply_session_visible_ref_aliases_with_meter(&mut response, |text| text.chars().count());
    let alias = response.refs[0].ref_id.clone();
    assert!(alias.starts_with("tz://o/"), "{alias}");
    assert_eq!(response.detail_ref.as_deref(), Some(alias.as_str()));
    assert!(
        !serde_json::to_string(&response)
            .unwrap()
            .contains(&full_ref),
        "every response field must use the same alias: {response:?}"
    );
    // The gate is whole-serialization, not lexical: equal/larger rewrites
    // are rejected, strictly smaller ones win (vocabulary-free meter).
    let meter = |text: &str| text.chars().count();
    assert_eq!(
        rewrite_full_refs_if_strictly_cheaper("aa", &[("aa".into(), "bb".into())], meter),
        None
    );
    assert_eq!(
        rewrite_full_refs_if_strictly_cheaper("aa", &[("aa".into(), "bbbb".into())], meter),
        None
    );
    assert_eq!(
        rewrite_full_refs_if_strictly_cheaper(
            "full-aaaa full-aaaa",
            &[("full-aaaa".into(), "bb".into())],
            meter
        ),
        Some("bb bb".to_owned())
    );
    drop(engine);

    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&alias, Some("raw"), None, None, None, None);
    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content, "exact payload");
}

#[test]
fn session_alias_rewrite_is_rejected_when_the_complete_response_is_not_cheaper() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery.json");
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache.clone();
    let engine = TokenZeroEngine::new(config);
    let full_ref = engine
        .recovery_store()
        .store_blob("exact payload", ContentType::Unknown)
        .unwrap();
    let mut response = ToolResponse::ok(
        "read",
        Mode::Auto,
        format!("visible {full_ref}"),
        vec![RefRecord {
            kind: "blob".into(),
            ref_id: full_ref.clone(),
            bytes: 13,
            live: true,
        }],
        Accounting {
            raw_tokens: 3,
            visible_tokens: 3,
            recovery_tokens: 0,
            billed_tokens: 3,
            cached_tokens: 0,
            exact_ref_tokens: None,
        },
    );
    let before = serde_json::to_string(&response).unwrap();
    engine.apply_session_visible_ref_aliases(&mut response);
    let after = serde_json::to_string(&response).unwrap();
    // Default lexical gauge: `tz://o/<gen>/<ord>` (8 tokens) costs more
    // than `tz://blob/<64hex>` (6 tokens), so the gated rewrite is
    // rejected and the response keeps its byte/field semantics; the
    // persisted alias must not leak into any field.
    assert_eq!(
        after, before,
        "rejected rewrite must leave the response untouched"
    );
    assert!(
        after.contains(&full_ref),
        "full ref stays the visible identity"
    );
    assert!(
        !after.contains("tz://o/"),
        "no ordinal alias is exposed: {after}"
    );
}

#[test]
fn text_render_elides_only_exact_small_complete_read_footers() {
    let blob_ref = "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let file_ref = "tz://file/f0000000000000000";
    let response = |visible: String, bytes: usize| {
        let mut response = ToolResponse::ok(
            "read",
            Mode::Auto,
            visible,
            vec![
                RefRecord {
                    kind: "blob".into(),
                    ref_id: blob_ref.into(),
                    bytes,
                    live: true,
                },
                RefRecord {
                    kind: "file".into(),
                    ref_id: file_ref.into(),
                    bytes,
                    live: true,
                },
            ],
            Accounting {
                raw_tokens: 3,
                visible_tokens: 3,
                recovery_tokens: 0,
                billed_tokens: 3,
                cached_tokens: 0,
                exact_ref_tokens: None,
            },
        );
        response.telemetry = Some(serde_json::json!({"output_strategy": "full"}));
        response
    };

    let complete = response("alpha\nBETA\ngamma".into(), 16);
    assert!(render_text(&complete).contains(blob_ref));
    assert_eq!(
        render_text_with_complete_read(&complete),
        "alpha\nBETA\ngamma"
    );

    let partial = response("BETA".into(), 4);
    let partial_text = render_text(&partial);
    assert!(partial_text.contains(blob_ref), "{partial_text}");

    for trimmed_source in ["trailing space", "trailing tab", "trailing newline"] {
        let trimmed = response("abc".into(), 4);
        let text = render_text_with_complete_read(&trimmed);
        assert!(text.contains(blob_ref), "{trimmed_source}: {text}");
    }

    let large = response("x".repeat(257), 257);
    let large_text = render_text_with_complete_read(&large);
    assert!(large_text.contains(blob_ref), "{large_text}");

    let binary_like = response("a\0b".into(), 3);
    assert!(render_text_with_complete_read(&binary_like).contains(blob_ref));

    let mut lossy = response("alpha\nBETA\ngamma".into(), 16);
    lossy.telemetry = Some(serde_json::json!({"output_strategy": "seen_set_dedup"}));
    assert!(render_text_with_complete_read(&lossy).contains(blob_ref));

    let mut diagnosed = response("alpha\nBETA\ngamma".into(), 16);
    diagnosed.diagnostic = Some(Diagnostic {
        code: "note".into(),
        message: "review".into(),
        repair: None,
    });
    assert!(render_text_with_complete_read(&diagnosed).contains(blob_ref));

    let mut recovered = response("alpha\nBETA\ngamma".into(), 16);
    recovered.recovery = Some(RecoveryReceipt {
        terminal: true,
        do_not_recompact: true,
        exact_bytes: true,
    });
    assert!(render_text_with_complete_read(&recovered).contains(blob_ref));

    let mut channeled = response("alpha\nBETA\ngamma".into(), 16);
    channeled.channels = Some(ChannelSeparation {
        action: "read".into(),
        status_line: "ok".into(),
        user_message: None,
    });
    assert!(render_text_with_complete_read(&channeled).contains(blob_ref));
}

#[test]
fn text_render_elides_only_redundant_warm_search_refs() {
    let blob_ref = "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut response = ToolResponse::ok(
        "grep",
        Mode::Auto,
        format!("# grep needle — 2 matches; full results: expand {blob_ref}"),
        vec![
            RefRecord {
                kind: "blob".into(),
                ref_id: blob_ref.into(),
                bytes: 42,
                live: true,
            },
            RefRecord {
                kind: "search".into(),
                ref_id: "tz://search/h1111111111111111".into(),
                bytes: 0,
                live: true,
            },
            RefRecord {
                kind: "search".into(),
                ref_id: "tz://search/h2222222222222222".into(),
                bytes: 0,
                live: true,
            },
        ],
        Accounting {
            raw_tokens: 20,
            visible_tokens: 10,
            recovery_tokens: 0,
            billed_tokens: 10,
            cached_tokens: 0,
            exact_ref_tokens: None,
        },
    );
    response.telemetry = Some(serde_json::json!({
        "output_strategy": "seen_set_dedup",
        "transport_status": "ok",
        "exact_refs_available": true,
        "degraded": false,
        "storage_error": null,
        "truncated_by_visit": false,
        "matches": 2
    }));

    let compact = render_text(&response);
    assert!(!compact.contains("search_ref:"), "{compact}");
    assert!(compact.contains(blob_ref), "{compact}");

    let mut cold = response.clone();
    cold.telemetry.as_mut().unwrap()["output_strategy"] = serde_json::json!("full");
    assert!(render_text(&cold).contains("search_ref:"));

    let mut dead = response.clone();
    dead.refs[1].live = false;
    assert!(render_text(&dead).contains("search_ref:"));

    let mut warned = response.clone();
    warned.safety = Some(serde_json::json!({"warning": "review"}));
    assert!(render_text(&warned).contains("search_ref:"));

    let mut mixed = response.clone();
    mixed.refs.push(RefRecord {
        kind: "blob".into(),
        ref_id: "tz://blob/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        bytes: 1,
        live: true,
    });
    assert!(render_text(&mixed).contains("search_ref:"));

    let mut diagnosed = response.clone();
    diagnosed.diagnostic = Some(Diagnostic {
        code: "note".into(),
        message: "review".into(),
        repair: None,
    });
    assert!(render_text(&diagnosed).contains("search_ref:"));

    let mut recovered = response.clone();
    recovered.recovery = Some(RecoveryReceipt {
        terminal: true,
        do_not_recompact: true,
        exact_bytes: true,
    });
    assert!(render_text(&recovered).contains("search_ref:"));

    let mut channeled = response.clone();
    channeled.channels = Some(ChannelSeparation {
        action: "grep".into(),
        status_line: "ok".into(),
        user_message: None,
    });
    assert!(render_text(&channeled).contains("search_ref:"));

    let mut truncated = response.clone();
    truncated.telemetry.as_mut().unwrap()["truncated_by_visit"] = serde_json::json!(true);
    assert!(render_text(&truncated).contains("search_ref:"));

    let mut count_mismatch = response;
    count_mismatch.telemetry.as_mut().unwrap()["matches"] = serde_json::json!(1);
    assert!(render_text(&count_mismatch).contains("search_ref:"));
}

#[test]
fn text_render_quiets_only_verified_exact_edit_success() {
    let mut response = ToolResponse::ok(
        "edit",
        Mode::Auto,
        String::new(),
        vec![
            RefRecord {
                kind: "blob".into(),
                ref_id:
                    "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .into(),
                bytes: 17,
                live: true,
            },
            RefRecord {
                kind: "undo".into(),
                ref_id:
                    "tz://blob/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into(),
                bytes: 17,
                live: true,
            },
        ],
        Accounting {
            raw_tokens: 3,
            visible_tokens: 0,
            recovery_tokens: 0,
            billed_tokens: 0,
            cached_tokens: 0,
            exact_ref_tokens: None,
        },
    );
    response.ack = None;
    response.telemetry = Some(serde_json::json!({
        "transport_status": "ok",
        "exact_refs_available": true,
        "dry_run": false,
        "degraded": false,
        "storage_error": null
    }));
    assert_eq!(render_text(&response), "");

    let mut warned = response.clone();
    warned.safety = Some(serde_json::json!({"warning": "review"}));
    assert!(render_text(&warned).contains("undo_ref:"));

    let mut dead_ref = response.clone();
    dead_ref.refs[1].live = false;
    assert!(render_text(&dead_ref).contains("undo_ref:"));

    let mut diagnosed = response.clone();
    diagnosed.diagnostic = Some(Diagnostic {
        code: "note".into(),
        message: "review".into(),
        repair: None,
    });
    assert!(render_text(&diagnosed).contains("undo_ref:"));

    let mut recovered = response.clone();
    recovered.recovery = Some(RecoveryReceipt {
        terminal: true,
        do_not_recompact: true,
        exact_bytes: true,
    });
    assert!(render_text(&recovered).contains("undo_ref:"));

    let mut channeled = response.clone();
    channeled.channels = Some(ChannelSeparation {
        action: "edit".into(),
        status_line: "ok".into(),
        user_message: None,
    });
    assert!(render_text(&channeled).contains("undo_ref:"));

    let mut dry_run = response.clone();
    dry_run.telemetry.as_mut().unwrap()["dry_run"] = serde_json::json!(true);
    assert!(render_text(&dry_run).contains("undo_ref:"));

    let mut missing_telemetry = response;
    missing_telemetry.telemetry = None;
    assert!(render_text(&missing_telemetry).contains("undo_ref:"));
}

#[test]
fn multiline_preview_is_bounded_and_reports_omitted_lines() {
    let text = (1..=9)
        .map(|line| format!("line {line}: {}", "x".repeat(80)))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = preview(&text);
    assert!(rendered.lines().count() <= 7);
    assert!(rendered.chars().count() <= 320);
    assert!(rendered.ends_with("+3 more lines"), "{rendered}");
    assert!(rendered.contains('\n'));
}

#[test]
fn path_not_allowed_names_active_root_and_relative_repair() {
    let root = PathBuf::from("/workspace/project");
    let rejected = PathBuf::from("/tmp/secret.txt");
    let response = path_not_allowed("read", &rejected, std::slice::from_ref(&root));
    let error = response.error.expect("path_not_allowed must be an error");
    assert_eq!(error.code, "path_not_allowed");
    assert!(
        error.message.contains("outside allowed roots"),
        "classifier substring lost: {}",
        error.message
    );
    assert!(
        error.message.contains("/workspace/project"),
        "must echo the active root: {}",
        error.message
    );
    assert!(
        error.message.contains("re-root") && error.message.contains("relative"),
        "must tell the caller how to self-correct: {}",
        error.message
    );
    let repair = error.repair.expect("repair must be present");
    assert!(
        repair.contains("secret.txt") && repair.contains("re-root"),
        "repair must suggest a relative path or re-root: {repair}"
    );
}
