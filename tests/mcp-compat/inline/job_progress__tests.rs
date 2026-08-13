use super::*;

fn family_name(family: ClientFamily) -> &'static str {
    match family {
        ClientFamily::Amp => "amp",
        ClientFamily::Pi => "pi",
        ClientFamily::ClaudeCode => "claude-code",
        ClientFamily::Codex => "codex",
        ClientFamily::Grok => "grok",
        ClientFamily::OpenCode => "opencode",
        ClientFamily::Other => "conformance-client",
    }
}

#[test]
fn tz1c5y_client_families_and_capability_paths() {
    assert_eq!(classify_client("Amp"), ClientFamily::Amp);
    assert_eq!(classify_client("pi"), ClientFamily::Pi);
    assert_eq!(classify_client("claude-code"), ClientFamily::ClaudeCode);
    assert_eq!(classify_client("Codex"), ClientFamily::Codex);
    assert_eq!(classify_client("grok-code"), ClientFamily::Grok);
    assert_eq!(classify_client("OpenCode"), ClientFamily::OpenCode);

    let token = Some("tok-1");
    for family in [
        ClientFamily::Amp,
        ClientFamily::Pi,
        ClientFamily::ClaudeCode,
        ClientFamily::Codex,
        ClientFamily::Grok,
        ClientFamily::OpenCode,
    ] {
        assert_eq!(
            notify_mode(family, false, token),
            NotifyMode::Progress,
            "{family:?} with a progress token must use progress"
        );
    }

    assert_eq!(
        notify_mode(ClientFamily::Amp, false, None),
        NotifyMode::Logging
    );
    assert_eq!(
        notify_mode(ClientFamily::Pi, false, None),
        NotifyMode::Logging
    );
    assert_eq!(
        notify_mode(ClientFamily::ClaudeCode, false, None),
        NotifyMode::Logging
    );
    assert_eq!(
        notify_mode(ClientFamily::Grok, false, None),
        NotifyMode::Logging
    );
    assert_eq!(
        notify_mode(ClientFamily::Codex, false, None),
        NotifyMode::PollOnly
    );
    assert_eq!(
        notify_mode(ClientFamily::OpenCode, false, None),
        NotifyMode::PollOnly
    );
    assert_eq!(
        notify_mode(ClientFamily::Codex, true, None),
        NotifyMode::Logging
    );
}

#[test]
fn tz1c5y_exactly_one_terminal_and_no_raw_log_flood() {
    for family in [
        ClientFamily::Amp,
        ClientFamily::Pi,
        ClientFamily::ClaudeCode,
        ClientFamily::Codex,
        ClientFamily::Grok,
        ClientFamily::OpenCode,
    ] {
        let session = format!("tz1c5y-{}", family_name(family));
        remember_client(&session, family_name(family), &json!({}));
        remember_progress_token(&session, Some(format!("pt-{}", family_name(family))));
        observe(
            &session,
            JobEvent::Started {
                job_id: "job-1".into(),
            },
        );
        observe(
            &session,
            JobEvent::Progress {
                job_id: "job-1".into(),
                cursor: 32,
            },
        );
        observe(
            &session,
            JobEvent::Completed {
                job_id: "job-1".into(),
                status: "exited".into(),
            },
        );
        observe(
            &session,
            JobEvent::Completed {
                job_id: "job-1".into(),
                status: "exited".into(),
            },
        );
        let notes = take_notifications(&session);
        let terminals: Vec<_> = notes
            .iter()
            .filter(|n| {
                n["params"]["message"]
                    .as_str()
                    .or_else(|| n["params"]["data"].as_str())
                    .is_some_and(|text| text.contains("exited"))
            })
            .collect();
        assert_eq!(
            terminals.len(),
            1,
            "{family:?} must emit exactly one terminal: {notes:?}"
        );
        for note in &notes {
            let blob = note.to_string();
            assert!(
                !blob.contains("\nCloning") && blob.len() < 400,
                "notification must stay bounded: {blob}"
            );
        }
    }
}

#[test]
fn tz1c5y_poll_only_clients_keep_long_poll_fallback() {
    let session = "tz1c5y-opencode-poll";
    remember_client(session, "opencode", &json!({}));
    observe(
        session,
        JobEvent::Started {
            job_id: "job-2".into(),
        },
    );
    observe(
        session,
        JobEvent::Completed {
            job_id: "job-2".into(),
            status: "exited".into(),
        },
    );
    assert!(
        take_notifications(session).is_empty(),
        "OpenCode without a progress token keeps zero.token.job polling"
    );
}

#[test]
fn tz1c5y_tools_call_emits_progress_frame_from_handle_jsonrpc() {
    let dir = tempfile::tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    engine.mark_lifecycle_ready_for_tests();
    let command = if cfg!(windows) {
        "echo tz1c5y-live"
    } else {
        "printf tz1c5y-live"
    };
    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "tz_shell",
            "arguments": { "command": command, "background": true },
            "_meta": { "progressToken": "pt-live" }
        }
    });
    let raw = crate::handle_jsonrpc(&engine, &request.to_string())
        .expect("tools/call must return a JSON-RPC exchange");
    let frames: Vec<Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|err| panic!("{err}: {line}")))
        .collect();
    let progress: Vec<&Value> = frames
        .iter()
        .filter(|frame| frame["method"] == "notifications/progress")
        .collect();
    assert!(
        !progress.is_empty(),
        "handle_jsonrpc must emit a notifications/progress frame: {raw}"
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame["id"] == 7 && frame.get("result").is_some()),
        "tools/call result must still leave handle_jsonrpc: {raw}"
    );
    assert!(
        progress.iter().any(|frame| {
            frame["params"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("exited") || text.contains("failed"))
        }),
        "real job lifecycle must emit a terminal progress frame: {raw}"
    );
}
