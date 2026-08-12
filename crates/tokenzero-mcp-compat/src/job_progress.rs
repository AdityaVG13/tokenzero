//! Capability-aware MCP progress / logging for background jobs (tokenzero-1c5y).
//!
//! Clients that send a progress token or advertise logging get a bounded start
//! event, optional short progress, and exactly one terminal event. Everyone
//! else keeps `zero.token.job` long-poll. Notifications never include raw job
//! logs.

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

const MAX_MESSAGE_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientFamily {
    Amp,
    Pi,
    ClaudeCode,
    Codex,
    Grok,
    OpenCode,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyMode {
    /// `notifications/progress` with the client-supplied token.
    Progress,
    /// `notifications/message` (MCP logging).
    Logging,
    /// No push; client must long-poll `zero.token.job`.
    PollOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Progress/Completed are observed from tests and later wiring.
pub enum JobEvent {
    Started { job_id: String },
    Progress { job_id: String, cursor: u64 },
    Completed { job_id: String, status: String },
}

#[derive(Debug, Default)]
struct Session {
    family: ClientFamily,
    logging_enabled: bool,
    progress_token: Option<String>,
    terminals: HashSet<String>,
    pending: Vec<Value>,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_sessions() -> std::sync::MutexGuard<'static, HashMap<String, Session>> {
    SESSIONS.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn classify_client(name: &str) -> ClientFamily {
    let lower = name.to_ascii_lowercase();
    if lower.contains("opencode") {
        ClientFamily::OpenCode
    } else if lower.contains("claude") {
        ClientFamily::ClaudeCode
    } else if lower.contains("codex") {
        ClientFamily::Codex
    } else if lower.contains("grok") {
        ClientFamily::Grok
    } else if lower == "amp" || lower.starts_with("amp-") || lower.starts_with("amp ") {
        ClientFamily::Amp
    } else if lower == "pi" || lower.starts_with("pi-") || lower.starts_with("pi ") {
        ClientFamily::Pi
    } else {
        ClientFamily::Other
    }
}

pub fn notify_mode(
    family: ClientFamily,
    logging_enabled: bool,
    progress_token: Option<&str>,
) -> NotifyMode {
    if progress_token.is_some() {
        return NotifyMode::Progress;
    }
    match family {
        ClientFamily::Amp | ClientFamily::Pi | ClientFamily::ClaudeCode | ClientFamily::Grok => {
            NotifyMode::Logging
        }
        ClientFamily::Codex | ClientFamily::OpenCode | ClientFamily::Other => {
            if logging_enabled {
                NotifyMode::Logging
            } else {
                NotifyMode::PollOnly
            }
        }
    }
}

fn bound_message(text: &str) -> String {
    let mut out: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
    if text.chars().count() > MAX_MESSAGE_CHARS {
        out.push('…');
    }
    out
}

fn progress_notification(token: &str, progress: u64, total: u64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": {
            "progressToken": token,
            "progress": progress,
            "total": total,
            "message": bound_message(message),
        }
    })
}

fn logging_notification(message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/message",
        "params": {
            "level": "info",
            "logger": "tokenzero.job",
            "data": bound_message(message),
        }
    })
}

/// Plan the JSON-RPC notification for one job event. `None` means poll-only
/// or a duplicate terminal.
pub fn plan_notification(
    mode: NotifyMode,
    token: Option<&str>,
    event: &JobEvent,
    already_terminal: bool,
) -> Option<Value> {
    match event {
        JobEvent::Completed { .. } if already_terminal => return None,
        _ => {}
    }
    match (mode, event) {
        (NotifyMode::PollOnly, _) => None,
        (NotifyMode::Progress, JobEvent::Started { job_id }) => Some(progress_notification(
            token.unwrap_or(job_id),
            0,
            1,
            &format!("job {job_id} started"),
        )),
        (NotifyMode::Progress, JobEvent::Progress { job_id, cursor }) => {
            Some(progress_notification(
                token.unwrap_or(job_id),
                0,
                1,
                &format!("job {job_id} bytes={cursor}"),
            ))
        }
        (NotifyMode::Progress, JobEvent::Completed { job_id, status }) => {
            Some(progress_notification(
                token.unwrap_or(job_id),
                1,
                1,
                &format!("job {job_id} {status}"),
            ))
        }
        (NotifyMode::Logging, JobEvent::Started { job_id }) => {
            Some(logging_notification(&format!("job {job_id} started")))
        }
        (NotifyMode::Logging, JobEvent::Progress { .. }) => None,
        (NotifyMode::Logging, JobEvent::Completed { job_id, status }) => {
            Some(logging_notification(&format!("job {job_id} {status}")))
        }
    }
}

pub fn remember_client(session_id: &str, client_name: &str, _capabilities: &Value) {
    let mut sessions = lock_sessions();
    let session = sessions.entry(session_id.to_string()).or_default();
    session.family = classify_client(client_name);
}

pub fn remember_logging_enabled(session_id: &str) {
    lock_sessions()
        .entry(session_id.to_string())
        .or_default()
        .logging_enabled = true;
}

pub fn remember_progress_token(session_id: &str, token: Option<String>) {
    if let Some(token) = token {
        lock_sessions()
            .entry(session_id.to_string())
            .or_default()
            .progress_token = Some(token);
    }
}

pub fn observe(session_id: &str, event: JobEvent) {
    let mut sessions = lock_sessions();
    let session = sessions.entry(session_id.to_string()).or_default();
    let job_id = match &event {
        JobEvent::Started { job_id }
        | JobEvent::Progress { job_id, .. }
        | JobEvent::Completed { job_id, .. } => job_id.clone(),
    };
    let already = session.terminals.contains(&job_id);
    let mode = notify_mode(
        session.family,
        session.logging_enabled,
        session.progress_token.as_deref(),
    );
    if let Some(note) = plan_notification(mode, session.progress_token.as_deref(), &event, already)
    {
        session.pending.push(note);
    }
    if matches!(event, JobEvent::Completed { .. }) {
        session.terminals.insert(job_id);
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn take_notifications(session_id: &str) -> Vec<Value> {
    lock_sessions()
        .get_mut(session_id)
        .map(|session| std::mem::take(&mut session.pending))
        .unwrap_or_default()
}

pub fn progress_token_from_params(params: &Value) -> Option<String> {
    let meta = params.get("_meta")?;
    let token = meta.get("progressToken")?;
    token
        .as_str()
        .map(str::to_string)
        .or_else(|| token.as_i64().map(|n| n.to_string()))
        .or_else(|| token.as_u64().map(|n| n.to_string()))
}

pub fn job_id_from_tool_result(result: &Value) -> Option<String> {
    result
        .pointer("/structuredContent/job")
        .or_else(|| result.pointer("/structuredContent/cli/telemetry/job"))
        .or_else(|| result.get("job"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
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
}
