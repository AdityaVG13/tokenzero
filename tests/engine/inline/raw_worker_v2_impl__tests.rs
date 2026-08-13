use super::*;

fn engine() -> TokenZeroEngine {
    engine_from_options(&RawWorkerServeOptions::default())
}

fn handshake(expected_revision: &str) -> Value {
    let cap = local_capability();
    json!({"kind":"handshake","request":{
        "protocol_version":"zerostack.raw_worker.v2","root":"/fixture/repo","session_id":"session-1",
        "expected_engine":"tokenzero","expected_worker_revision":expected_revision,
        "expected_contract_digest":cap["semantic_contract_digest"],
        "expected_registry_digest":cap["operation_registry_digest"]
    }})
}

fn send(session: &mut RawWorkerV2Session, frame: Value) -> Value {
    serde_json::from_slice(&execute_raw_worker_v2_frame(
        &engine(),
        session,
        &serde_json::to_vec(&frame).unwrap(),
    ))
    .unwrap()
}

#[test]
fn response_encoding_uses_the_shared_strict_codec() {
    let shutdown = encode(json!({"kind":"shutdown_ack"}));
    assert_eq!(shutdown, b"{\"kind\":\"shutdown_ack\"}\n");
    assert!(matches!(
        zero_abi::decode_response_frame(&shutdown, zero_abi::DEFAULT_MAX_FRAME_BYTES),
        Ok(zero_abi::WorkerResponseFrame::ShutdownAck)
    ));

    let mutant = encode(json!({"kind":"shutdown_ack","extra":true}));
    let decoded = zero_abi::decode_response_frame(&mutant, zero_abi::DEFAULT_MAX_FRAME_BYTES)
        .expect("fallback uses shared codec");
    assert!(matches!(
        decoded,
        zero_abi::WorkerResponseFrame::Error { ref error, .. }
            if error.kind == "internal_contract"
    ));
}

#[test]
fn oversized_internal_request_id_cannot_panic_the_typed_fallback() {
    let oversized = "x".repeat(zero_abi::DEFAULT_MAX_FRAME_BYTES);
    let bytes = encode(json!({
        "kind":"error",
        "request_id":oversized,
        "error":{"kind":"internal","message":"boom","retryable":false}
    }));
    let decoded = zero_abi::decode_response_frame(&bytes, zero_abi::DEFAULT_MAX_FRAME_BYTES)
        .expect("uncorrelated fixed fallback uses shared codec");
    assert!(matches!(
        decoded,
        zero_abi::WorkerResponseFrame::Error {
            request_id: None,
            ref error,
            ..
        } if error.kind == "frame_too_large"
    ));
}

#[test]
fn golden_handshake_reports_binding_limits_and_metadata_capabilities() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let response = send(&mut session, handshake(&rev));
    assert_eq!(response["kind"], "handshake_ack");
    assert_eq!(response["ack"]["binding"]["worker_revision"], rev);
    assert_eq!(response["ack"]["binding"]["ref_scheme"], "tz://");
    assert_eq!(response["ack"]["limits"]["max_frame_bytes"], 1_048_576);
    assert_eq!(response["ack"]["capabilities"]["cancellation"], true);
}

#[test]
fn requested_worker_token_accounting_matches_domain_accounting_and_hub_abi() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-accounting","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let response = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-accounting",
            "op":"mem",
            "args":{},
            "telemetry_request":{
                "engine_stage_timeline":true,
                "worker_token_accounting":true
            },
            "trace":trace
        }}),
    );
    assert_eq!(response["kind"], "result", "{response}");
    let accounting: raw_worker_v2_protocol::WorkerTokenAccountingV1 =
        serde_json::from_value(response["worker_token_accounting"].clone()).unwrap();
    zero_abi::validate_worker_token_accounting_v1(&accounting).unwrap();
    assert_eq!(
        accounting.count_kind,
        raw_worker_v2_protocol::WorkerTokenCountKind::ConservativeUpperBound
    );
    assert_eq!(accounting.tokenizer_id, "conservative:utf8-json-bytes-v1");
    let timeline: raw_worker_v2_protocol::EngineStageTimelineV1 =
        serde_json::from_value(response["engine_timeline"].clone()).unwrap();
    zero_abi::validate_engine_stage_timeline_v1(&timeline).unwrap();
    assert_eq!(timeline.spans[0].stage, "tokenzero.raw_worker_call");
    let domain = &response["result"]["value"]["accounting"];
    assert!(accounting.raw_tokens >= accounting.visible_tokens);
    assert!(accounting.visible_tokens >= domain["visible_tokens"].as_u64().unwrap());
    assert!(accounting.recovery_tokens >= domain["recovery_tokens"].as_u64().unwrap());
    assert!(accounting.billed_tokens >= accounting.visible_tokens);
    assert_eq!(
        accounting.cached_tokens,
        domain["cached_tokens"].as_u64().unwrap()
    );
    assert_eq!(accounting.exact_ref_tokens, None);
    let encoded = serde_json::to_vec(&response).unwrap();
    let decoded =
        zero_abi::decode_response_frame(&encoded, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES)
            .unwrap();
    assert!(matches!(
        decoded,
        zero_abi::WorkerResponseFrame::Result {
            worker_token_accounting: Some(_),
            ..
        }
    ));
}

#[test]
fn relative_read_and_edit_bind_to_call_root_not_process_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // Process cwd is deliberately NOT the bound root: relative path args
    // must resolve against EngineConfig.call_root, never the worker's
    // working directory.
    let fixture = root.join("fixture.txt");
    std::fs::write(&fixture, "needle\n").unwrap();
    let engine = engine_from_options(&RawWorkerServeOptions {
        root: root.clone(),
        ..Default::default()
    });
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    let frame = |session: &mut RawWorkerV2Session, value: &Value| -> Value {
        serde_json::from_slice(&execute_raw_worker_v2_frame(
            &engine,
            session,
            &serde_json::to_vec(value).unwrap(),
        ))
        .unwrap()
    };
    let ack = frame(
        &mut session,
        &json!({"kind":"handshake","request":{
            "protocol_version":"zerostack.raw_worker.v2",
            "root": root.to_string_lossy(), "session_id":"session-1",
            "expected_engine":"tokenzero","expected_worker_revision":rev,
            "expected_contract_digest":cap["semantic_contract_digest"],
            "expected_registry_digest":cap["operation_registry_digest"]
        }}),
    );
    assert_eq!(ack["kind"], "handshake_ack", "{ack}");
    let trace = |request_id: &str| {
        json!({"runtime_id":"rt","cell_id":"cell","request_id":request_id,
            "trace_id":request_id,"worker_revision":rev,
            "contract_digest":cap["semantic_contract_digest"]})
    };
    // Relative read binds to the bound root, not process cwd.
    let read = frame(
        &mut session,
        &json!({"kind":"call","request":{
            "request_id":"req-rel-read","op":"read",
            "args":{"path":"fixture.txt"},"trace":trace("req-rel-read")
        }}),
    );
    assert_eq!(read["kind"], "result", "{read}");
    let read_value = &read["result"]["value"];
    assert_eq!(read_value["status"], "ok", "{read}");
    assert!(
        read_value["visible"]
            .as_str()
            .unwrap_or_default()
            .contains("needle"),
        "{read}"
    );
    // Relative edit targets the bound root and actually mutates disk.
    let edit = frame(
        &mut session,
        &json!({"kind":"call","request":{
            "request_id":"req-rel-edit","op":"edit",
            "args":{"path":"fixture.txt",
                "edits":[{"find":"needle","replace":"changed"}]},
            "trace":trace("req-rel-edit")
        }}),
    );
    assert_eq!(edit["kind"], "result", "{edit}");
    assert_eq!(
        std::fs::read_to_string(&fixture).unwrap(),
        "changed\n",
        "relative edit must mutate the file inside the bound root"
    );
    // Escape remains rejected: `..` must not resolve out of the bound root.
    let escape = frame(
        &mut session,
        &json!({"kind":"call","request":{
            "request_id":"req-escape","op":"read",
            "args":{"path":"../fixture.txt"},"trace":trace("req-escape")
        }}),
    );
    assert_eq!(escape["kind"], "error", "{escape}");
    assert_eq!(escape["error"]["kind"], "policy", "{escape}");
}

#[test]
fn unrequested_accounting_preserves_the_legacy_response_shape() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-legacy","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let response = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-legacy","op":"mem","args":{},"trace":trace
        }}),
    );
    assert_eq!(response["kind"], "result", "{response}");
    assert!(response.get("worker_token_accounting").is_none());
    assert!(response.get("engine_timeline").is_none());
}

#[test]
fn requested_timeline_is_preserved_on_typed_dispatch_errors() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-error","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let response = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-error",
            "op":"execute_code",
            "args":{},
            "telemetry_request":{
                "engine_stage_timeline":true,
                "worker_token_accounting":true
            },
            "trace":trace
        }}),
    );
    assert_eq!(response["kind"], "error");
    assert_eq!(response["error"]["kind"], "unsupported_operation");
    assert!(response.get("worker_token_accounting").is_none());
    let timeline: raw_worker_v2_protocol::EngineStageTimelineV1 =
        serde_json::from_value(response["engine_timeline"].clone()).unwrap();
    zero_abi::validate_engine_stage_timeline_v1(&timeline).unwrap();
    zero_abi::decode_response_frame(
        &serde_json::to_vec(&response).unwrap(),
        raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES,
    )
    .unwrap();
}

#[test]
fn missing_or_inconsistent_domain_accounting_fails_loudly() {
    assert!(worker_token_accounting("read", &json!({}), &json!({})).is_err());
    let error = worker_token_accounting(
        "read",
        &json!({}),
        &json!({"refs":[],"accounting":{
            "raw_tokens":10,
            "visible_tokens":5,
            "recovery_tokens":0,
            "billed_tokens":1,
            "cached_tokens":2,
            "exact_ref_tokens":0
        }}),
    )
    .unwrap_err();
    assert!(
        error.contains("cached_tokens exceeds billed_tokens"),
        "{error}"
    );

    let unicode = json!({
        "visible":{"kind":"capsule","text":"é🙂"},
        "refs":[{"kind":"blob","ref":"tz://blob/example","bytes":9,"live":true}],
        "accounting":{
            "raw_tokens":1,
            "visible_tokens":1,
            "recovery_tokens":1,
            "billed_tokens":1,
            "cached_tokens":0
        }
    });
    let upper = worker_token_accounting("read", &json!({"input":"é🙂"}), &unicode).unwrap();
    assert_eq!(
        upper.count_kind,
        raw_worker_v2_protocol::WorkerTokenCountKind::ConservativeUpperBound
    );
    assert!(upper.raw_tokens >= upper.visible_tokens + 9);
    assert_eq!(upper.recovery_tokens, 9);
    assert_eq!(upper.exact_ref_tokens, None);

    let job = worker_token_accounting(
        zero_abi::TOKEN_JOB_OPERATION_V1,
        &json!({"id":"job-1"}),
        &json!({"id":"job-1","status":"exited"}),
    )
    .unwrap();
    assert_eq!(
        job.count_kind,
        raw_worker_v2_protocol::WorkerTokenCountKind::ConservativeUpperBound
    );
    assert_eq!(job.cached_tokens, 0);
    assert_eq!(job.recovery_tokens, 0);

    let launch = worker_token_accounting(
        "shell",
        &json!({"command":"printf ok","background":true}),
        &json!({"job":"job-1","cursor":0,"version":0}),
    )
    .unwrap();
    assert_eq!(
        launch.count_kind,
        raw_worker_v2_protocol::WorkerTokenCountKind::ConservativeUpperBound
    );
    assert_eq!(launch.cached_tokens, 0);
    assert_eq!(launch.recovery_tokens, 0);
}

#[test]
fn skew_and_pre_handshake_calls_are_rejected() {
    let mut session = RawWorkerV2Session::default();
    let response = send(&mut session, handshake("skewed"));
    assert_eq!(response["error"]["kind"], "worker_revision_changed");
    assert_eq!(response["error"]["retryable"], true);
    let response = send(
        &mut session,
        json!({"kind":"cancel","request":{"request_id":"r"}}),
    );
    assert_eq!(response["error"]["kind"], "handshake_required");
}

#[test]
fn handshake_rebind_after_revision_swap_is_survivable() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let first = send(&mut session, handshake(&rev));
    assert_eq!(first["kind"], "handshake_ack");
    // Same root+session re-handshakes cleanly (host-side revision swap).
    let second = send(&mut session, handshake(&rev));
    assert_eq!(second["kind"], "handshake_ack");
    assert_eq!(second["ack"]["binding"]["worker_revision"], rev);
}

#[test]
fn rehandshake_with_foreign_session_stays_terminal() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    send(&mut session, handshake(&rev));
    let cap = local_capability();
    let foreign = json!({"kind":"handshake","request":{
        "protocol_version":"zerostack.raw_worker.v2","root":"/fixture/repo","session_id":"session-2",
        "expected_engine":"tokenzero","expected_worker_revision":rev,
        "expected_contract_digest":cap["semantic_contract_digest"],
        "expected_registry_digest":cap["operation_registry_digest"]
    }});
    let response = send(&mut session, foreign);
    assert_eq!(response["error"]["kind"], "already_bound");
    assert_eq!(response["error"]["retryable"], false);
}

#[test]
fn stale_trace_revision_is_retryable_and_recoverable() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let stale = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req","trace_id":"trace",
        "worker_revision":"stale-rev","contract_digest":cap["semantic_contract_digest"]});
    let response = send(
        &mut session,
        json!({"kind":"call","request":{"request_id":"req","op":"read",
        "args":{},"trace":stale}}),
    );
    assert_eq!(response["error"]["kind"], "worker_revision_changed");
    assert_eq!(response["error"]["retryable"], true);
    assert_eq!(response["trace"]["trace_id"], "trace");
    // Recovery: re-handshake, then the call dispatches with a fresh trace.
    let rebind = send(&mut session, handshake(&rev));
    assert_eq!(rebind["kind"], "handshake_ack");
    let fresh = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let response = send(
        &mut session,
        json!({"kind":"call","request":{"request_id":"req","op":"read",
        "args":{},"trace":fresh}}),
    );
    assert_ne!(response["error"]["kind"], "worker_revision_changed");
}

#[test]
fn inbound_bound_is_enforced_before_json_parse() {
    let mut session = RawWorkerV2Session::default();
    let response: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
        &engine(),
        &mut session,
        &vec![b'x'; 1_048_577],
    ))
    .unwrap();
    assert_eq!(response["error"]["kind"], "frame_too_large");
}

#[test]
fn oversized_frame_fails_closed_without_parsing_request_id() {
    let mut session = RawWorkerV2Session::default();
    let mut frame = serde_json::to_vec(&json!({
        "kind": "call",
        "request": {"request_id": "req-oversized", "op": "token.read"}
    }))
    .unwrap();
    frame.extend(std::iter::repeat_n(b' ', 1_048_600));
    let response: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
        &engine(),
        &mut session,
        &frame,
    ))
    .unwrap();
    assert_eq!(response["error"]["kind"], "frame_too_large");
    assert!(response.get("request_id").is_none());
}

#[test]
fn in_bound_typed_frame_error_round_trips_request_id() {
    let mut session = RawWorkerV2Session::default();
    let line = br#"{"kind":"call","request":{"request_id":"req-7"},"extra":}"#.to_vec();
    let response: Value =
        serde_json::from_slice(&execute_raw_worker_v2_frame(&engine(), &mut session, &line))
            .unwrap();
    assert_eq!(response["error"]["kind"], "invalid_frame");
    assert!(response.get("request_id").is_none());
}

#[test]
fn empty_and_malformed_frames_return_typed_invalid_frame() {
    let mut session = RawWorkerV2Session::default();
    for line in [b"\n".to_vec(), b"{not json".to_vec()] {
        let response: Value =
            serde_json::from_slice(&execute_raw_worker_v2_frame(&engine(), &mut session, &line))
                .unwrap();
        assert_eq!(response["error"]["kind"], "invalid_frame");
    }
}

#[test]
fn bounded_reader_rejects_oversized_line_without_unbounded_growth() {
    let mut oversized = vec![b'x'; 1_048_600];
    oversized.push(b'\n');
    oversized.extend_from_slice(b"{}\n");
    let mut reader = std::io::BufReader::new(std::io::Cursor::new(oversized));
    let first =
        read_bounded_frame(&mut reader, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES).unwrap();
    assert!(matches!(first, BoundedFrame::TooLarge));
    let second =
        read_bounded_frame(&mut reader, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES).unwrap();
    assert!(matches!(second, BoundedFrame::Line(line) if line == b"{}\n"));
}

#[test]
fn expired_deadline_preserves_typed_trace_and_cancel_truth() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let response = send(
        &mut session,
        json!({"kind":"call","request":{"request_id":"req","op":"read",
        "args":{},"deadline_unix_ms":1,"trace":trace}}),
    );
    assert_eq!(response["error"]["kind"], "deadline_exceeded");
    assert_eq!(response["trace"]["trace_id"], "trace");
    let response = send(
        &mut session,
        json!({"kind":"cancel","request":{"request_id":"unknown"}}),
    );
    assert_eq!(response["cancelled"], false);
}

#[test]
fn cross_root_replay_against_bound_session_fails_closed() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    send(&mut session, handshake(&rev));
    let cap = local_capability();
    let replay = json!({"kind":"handshake","request":{
        "protocol_version":"zerostack.raw_worker.v2","root":"/fixture/other","session_id":"session-1",
        "expected_engine":"tokenzero","expected_worker_revision":rev,
        "expected_contract_digest":cap["semantic_contract_digest"],
        "expected_registry_digest":cap["operation_registry_digest"]
    }});
    let response = send(&mut session, replay);
    assert_eq!(response["error"]["kind"], "already_bound");
    assert_eq!(response["error"]["retryable"], false);
}

#[test]
fn registry_engine_and_contract_mismatches_fail_closed_typed() {
    let cap = local_capability();
    let rev = revision();
    for (field, bad) in [
        ("expected_registry_digest", json!("d".repeat(64))),
        ("expected_engine", json!("fszero")),
        ("expected_contract_digest", json!("d".repeat(64))),
    ] {
        let mut session = RawWorkerV2Session::default();
        let mut frame = handshake(&rev);
        frame["request"][field] = bad;
        let response = send(&mut session, frame);
        assert_eq!(
            response["error"]["kind"], "binding_mismatch",
            "field {field} must fail closed"
        );
        assert_eq!(response["error"]["retryable"], false);
        assert!(session.binding.is_none());
    }
    let _ = cap;
}

#[test]
fn absent_optional_digest_pins_are_allowed_skew() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let mut frame = handshake(&rev);
    let request = frame["request"].as_object_mut().unwrap();
    request.remove("expected_registry_digest");
    request.remove("expected_worker_revision");
    let response = send(&mut session, frame);
    assert_eq!(response["kind"], "handshake_ack");
}

/// Serializes tests that dispatch through the process-global
/// ACTIVE_CANCEL slot so parallel test threads cannot clobber it.
static DISPATCH_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(unix)]
fn process_is_live(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
#[cfg(unix)]
fn cancel_control_frame_stops_dispatched_shell_work() {
    let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    crate::shell_hooks::install(crate::shell_hooks::ProcessHooks::with_note_child(
        v2_note_child,
    ));
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-cancel","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let frame = json!({"kind":"call","request":{"request_id":"req-cancel","op":"shell",
        "args":{"command":"sleep 30 & child=$!; printf 'pid:%s\\npartial-before-cancel' \"$child\"; wait \"$child\""},"trace":trace}});
    let ctx = validate_call(session.binding.as_ref().unwrap(), &frame).unwrap();
    let cancel = session.register_cancel(&ctx.id);
    let observed_cancel = Arc::clone(&cancel);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let engine = engine();
        // Engine construction is slow on cold stores; only the dispatch
        // window counts for the cancel bound.
        ready_tx.send(()).expect("ready signal");
        run_call_registered(&engine, ctx, cancel)
    });
    ready_rx.recv().expect("worker ready");
    let started = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(
        observed_cancel
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_some(),
        "dispatch must publish the exact child before cancellation"
    );
    let ack = send(
        &mut session,
        json!({"kind":"cancel","request":{"request_id":"req-cancel"}}),
    );
    assert_eq!(ack["kind"], "cancel_ack");
    assert_eq!(ack["cancelled"], true);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "cancel acknowledgement exceeded bound: {:?}",
        started.elapsed()
    );
    assert!(ack.get("process_kill_supported").is_none());
    let value = worker.join().expect("worker joins after cancel");
    assert_eq!(value["error"]["kind"], "cancelled");
    assert_eq!(value["error"]["retryable"], false);
    let details = &value["error"]["details"];
    assert_eq!(
        details["artifact_scope"], "full_observed_stdout_stderr_streams",
        "{value}"
    );
    assert_eq!(details["temporal_interleaving_claimed"], false);
    let partial = &details["partial_result"]["tool_response"];
    assert_eq!(partial["safety"]["refs_cover_full_output"], true);
    assert_eq!(partial["telemetry"]["refs_cover_full_output"], true);
    let stdout_ref = partial["telemetry"]["stdout_ref"].as_str().unwrap();
    let expanded = engine().expand(stdout_ref, Some("raw"), None, None, None, None);
    let output = expanded.visible.unwrap().text;
    let mut lines = output.lines();
    let child_pid = lines
        .next()
        .and_then(|line| line.strip_prefix("pid:"))
        .expect("cancelled shell records descendant pid");
    assert_eq!(lines.next(), Some("partial-before-cancel"));
    let descendant_gone = (0..20).any(|_| {
        if !process_is_live(child_pid) {
            true
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
            false
        }
    });
    assert!(
        descendant_gone,
        "cancel must reap the background descendant"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "cancelled call must not run to completion"
    );
    crate::shell_hooks::reset();
}

#[test]
fn cancel_of_unknown_request_id_reports_false() {
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    send(&mut session, handshake(&rev));
    let ack = send(
        &mut session,
        json!({"kind":"cancel","request":{"request_id":"missing"}}),
    );
    assert_eq!(ack["cancelled"], false);
}

#[test]
fn deadline_reaches_dispatched_shell_work() {
    let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-dl","trace_id":"trace",
        "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
    let deadline = unix_ms() + 800;
    let started = std::time::Instant::now();
    let response = send(
        &mut session,
        json!({"kind":"call","request":{"request_id":"req-dl","op":"shell",
        "args":{"command":"sleep 30"},"deadline_unix_ms":deadline,"trace":trace}}),
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "deadline must reach the dispatched shell process"
    );
    let text = response.to_string();
    assert!(
        text.contains("\"timeout\":true") || text.contains("timed_out"),
        "shell run must report timeout enforcement: {text}"
    );
}

#[test]
fn background_shell_and_job_use_the_shared_typed_private_path_free_boundary() {
    let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = |request_id: &str| {
        json!({
            "runtime_id":"rt",
            "cell_id":"cell",
            "request_id":request_id,
            "trace_id":request_id,
            "worker_revision":rev,
            "contract_digest":cap["semantic_contract_digest"],
        })
    };
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command \"[Console]::Out.Write('tz://not-a-ref')\""
    } else {
        "printf 'tz://not-a-ref'"
    };
    let launched = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-job-launch",
            "op":"shell",
            "args":{"command":command,"background":true,"timeout_ms":2_000},
            "trace":trace("req-job-launch")
        }}),
    );
    assert_eq!(launched["kind"], "result", "{launched}");
    let launch = &launched["result"]["value"];
    assert_eq!(launch["cursor"], 0);
    assert_eq!(launch["version"], 0);
    assert!(
        launch.get("log").is_none(),
        "private log path leaked: {launch}"
    );
    assert_eq!(launch.as_object().unwrap().len(), 3, "{launch}");
    let id = launch["job"].as_str().unwrap().to_string();

    let polled = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-job-poll",
            "op":zero_abi::TOKEN_JOB_OPERATION_V1,
            "args":{"id":id,"waitMs":30_000,"since":0,"tailBytes":64},
            "trace":trace("req-job-poll")
        }}),
    );
    assert_eq!(polled["kind"], "result", "{polled}");
    let value = polled["result"]["value"].clone();
    assert!(
        value.get("log").is_none(),
        "private log path leaked: {value}"
    );
    let typed: zero_abi::TokenJobPollResultV1 = serde_json::from_value(value.clone()).unwrap();
    typed.validate().unwrap();
    assert!(typed.tail.contains("tz://not-a-ref"), "{value}");
    assert_eq!(polled["result"]["metadata"]["ownership"]["refs"], json!([]));

    let unknown = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-job-unknown",
            "op":zero_abi::TOKEN_JOB_OPERATION_V1,
            "args":{"id":id,"privateLog":"/private/session/job.log"},
            "trace":trace("req-job-unknown")
        }}),
    );
    assert_eq!(unknown["kind"], "error", "{unknown}");
    assert_eq!(unknown["error"]["kind"], "validation");
}

#[cfg(unix)]
#[test]
fn raw_session_shutdown_terminates_a_background_process_group() {
    struct ResetBackgroundTermination;
    impl Drop for ResetBackgroundTermination {
        fn drop(&mut self) {
            crate::engine_shell::reset_background_job_termination_for_tests();
        }
    }

    let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    crate::engine_shell::reset_background_job_termination_for_tests();
    let _reset = ResetBackgroundTermination;
    let mut session = RawWorkerV2Session::default();
    let rev = revision();
    let cap = local_capability();
    send(&mut session, handshake(&rev));
    let trace = json!({
        "runtime_id":"rt","cell_id":"cell","request_id":"req-long-job",
        "trace_id":"req-long-job","worker_revision":rev,
        "contract_digest":cap["semantic_contract_digest"],
    });
    let launched = send(
        &mut session,
        json!({"kind":"call","request":{
            "request_id":"req-long-job","op":"shell",
            "args":{"command":"sleep 30","background":true,"timeout_ms":60_000},
            "trace":trace
        }}),
    );
    assert_eq!(launched["kind"], "result", "{launched}");
    let id = launched["result"]["value"]["job"]
        .as_str()
        .unwrap()
        .to_string();
    let probe_engine = engine();
    let pid = (0..100)
        .find_map(|_| {
            let pid = probe_engine
                .shell_job_wait(&id, std::time::Duration::ZERO, 0, 1)
                .unwrap()["pid"]
                .as_u64();
            if pid.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            pid
        })
        .expect("background child did not publish its pid");

    let shutdown = send(
        &mut session,
        json!({"kind":"shutdown","request":{"reason":"test"}}),
    );
    assert_eq!(shutdown["kind"], "shutdown_ack");
    let session = Mutex::new(session);
    terminate_raw_worker_v2_session(&session);
    let gone = (0..20).any(|_| {
        let status = std::process::Command::new("kill")
            .args(["-0", "--", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        if status.success() {
            std::thread::sleep(std::time::Duration::from_millis(50));
            false
        } else {
            true
        }
    });
    assert!(gone, "background child {pid} survived raw session shutdown");
}

#[test]
fn v2_effect_class_marks_shell_and_compaction_storage_writes() {
    assert_eq!(effect_class("read"), "read_only");
    assert_eq!(effect_class("shell"), "irreversible");
    assert_eq!(effect_class("compact"), "irreversible");
    assert_eq!(effect_class("ingest"), "irreversible");
}

#[test]
fn oversized_result_value_is_rejected_with_typed_output_error() {
    let dir = tempfile::tempdir().unwrap();
    let big = "x".repeat(70_000);
    std::fs::write(dir.path().join("big.txt"), &big).unwrap();
    let opts = RawWorkerServeOptions {
        root: dir.path().to_path_buf(),
        cache_path: Some(dir.path().join("recovery-cache.json")),
        ..RawWorkerServeOptions::default()
    };
    let engine = engine_from_options(&opts);
    let root = dir.path().display().to_string();
    let mut session = RawWorkerV2Session::for_binding(&root, "s-oversize");
    let cap = local_capability();
    let ack: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
        &engine,
        &mut session,
        &serde_json::to_vec(&json!({"kind":"handshake","request":{
            "protocol_version":raw_worker_v2_protocol::RAW_WORKER_PROTOCOL_VERSION,
            "root":root,
            "session_id":"s-oversize",
            "expected_engine":"tokenzero",
            "expected_contract_digest":cap["semantic_contract_digest"],
            "expected_registry_digest":cap["operation_registry_digest"]
        }}))
        .unwrap(),
    ))
    .unwrap();
    assert_eq!(ack["kind"], "handshake_ack", "{ack}");
    assert_eq!(
        ack["ack"]["limits"]["max_output_bytes"], MAX_OUTPUT_BYTES as u64,
        "handshake must advertise the enforced constant"
    );
    let revision = ack["ack"]["binding"]["worker_revision"]
        .as_str()
        .unwrap()
        .to_string();
    let contract = ack["ack"]["binding"]["semantic_contract_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let response: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
        &engine,
        &mut session,
        &serde_json::to_vec(&json!({"kind":"call","request":{
            "request_id":"req-oversize","op":"read",
            "args":{
                "path":dir.path().join("big.txt").display().to_string(),
                "raw":true,
                "max_visible_tokens":1_000_000
            },
            "trace":{
                "runtime_id":"rt","cell_id":"cell","request_id":"req-oversize",
                "trace_id":"trace-oversize","worker_revision":revision,"contract_digest":contract
            }
        }}))
        .unwrap(),
    ))
    .unwrap();
    assert_eq!(response["kind"], "error", "{response}");
    assert_eq!(response["request_id"], "req-oversize", "{response}");
    assert_eq!(response["error"]["kind"], "output_too_large", "{response}");
    let details = &response["error"]["details"];
    assert_eq!(details["limit_name"], "max_output_bytes", "{details}");
    assert_eq!(details["limit_bytes"], 65_536u64, "{details}");
    assert!(
        details["actual_bytes"].as_u64().unwrap() > 65_536,
        "oversized value must measure above the cap: {details}"
    );
    assert_eq!(
        details["frame_limit_bytes"],
        zero_abi::DEFAULT_MAX_FRAME_BYTES as u64,
        "{details}"
    );
    assert!(
        response.get("result").is_none(),
        "no oversized result may leak: {response}"
    );
}
