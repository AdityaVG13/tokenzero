//! Crash-transparent supervisor for the stdio MCP server.
//!
//! The supervisor process owns the client-facing stdio pipes for the entire
//! session and proxies JSON-RPC messages to an inner server child process.
//! If the child ever dies — panic-abort, OOM kill, anything the in-process
//! hardening cannot absorb — the supervisor respawns it with backoff, replays
//! the cached MCP `initialize` handshake so the new child is immediately
//! usable, answers the requests that were in flight with a retryable error,
//! and keeps proxying. The client never observes a disconnect.

use crate::stdio::{StdioEvent, read_stdio_events_from_reader, write_framed_jsonrpc};
use crate::{JsonRpcErrorData, jsonrpc_error};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufReader, Error, ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Consecutive immediate child deaths tolerated before the supervisor gives
/// up and lets the client see EOF (so the client can relaunch from scratch).
const MAX_CONSECUTIVE_SPAWN_FAILURES: u32 = 10;
/// A child that lives at least this long resets the failure backoff.
const STABLE_CHILD_LIFETIME: Duration = Duration::from_secs(2);
#[cfg(not(test))]
const BASE_RESPAWN_BACKOFF: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const MAX_RESPAWN_BACKOFF: Duration = Duration::from_secs(2);
#[cfg(test)]
const BASE_RESPAWN_BACKOFF: Duration = Duration::from_millis(1);
#[cfg(test)]
const MAX_RESPAWN_BACKOFF: Duration = Duration::from_millis(5);
/// After the client closes stdin, in-flight child responses are still
/// forwarded for up to this long before the supervisor exits.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

pub fn run_supervised_stdio(program: OsString, child_args: Vec<OsString>) -> i32 {
    let (event_tx, event_rx) = mpsc::channel();
    let client_tx = event_tx.clone();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let (raw_tx, raw_rx) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(event) = raw_rx.recv() {
                if client_tx.send(SupervisorEvent::FromClient(event)).is_err() {
                    break;
                }
            }
        });
        read_stdio_events_from_reader(&mut reader, raw_tx);
    });
    let spawner = move || spawn_server_child(&program, &child_args);
    run_supervisor_loop(spawner, event_tx, event_rx, std::io::stdout())
}

pub(crate) enum SupervisorEvent {
    FromClient(StdioEvent),
    FromChild { generation: u64, text: String },
    ChildExited { generation: u64 },
}

/// Handles for one running inner-server child. Generic so tests can supply
/// in-memory children; production wraps `std::process::Child`.
pub(crate) struct ChildHandles {
    pub(crate) stdin: Box<dyn Write + Send>,
    pub(crate) stdout: Box<dyn Read + Send>,
    pub(crate) terminate: Box<dyn FnMut() + Send>,
}

fn spawn_server_child(program: &OsString, args: &[OsString]) -> std::io::Result<ChildHandles> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("supervised MCP child has no stdin pipe"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("supervised MCP child has no stdout pipe"))?;
    Ok(ChildHandles {
        stdin: Box::new(stdin),
        stdout: Box::new(stdout),
        terminate: Box::new(move || reap_child(&mut child)),
    })
}

fn reap_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

struct ActiveChild {
    /// `None` once the client has hung up and the child's stdin was closed
    /// to let it finish in-flight work and exit.
    stdin: Option<Box<dyn Write + Send>>,
    terminate: Box<dyn FnMut() + Send>,
    spawned_at: Instant,
    generation: u64,
}

fn write_child_line(child: &mut ActiveChild, line: &str) -> std::io::Result<()> {
    let Some(stdin) = child.stdin.as_mut() else {
        return Err(Error::from(ErrorKind::BrokenPipe));
    };
    writeln!(stdin, "{line}")?;
    stdin.flush()
}

pub(crate) fn run_supervisor_loop<W: Write>(
    mut spawn: impl FnMut() -> std::io::Result<ChildHandles>,
    event_tx: mpsc::Sender<SupervisorEvent>,
    event_rx: mpsc::Receiver<SupervisorEvent>,
    mut client_out: W,
) -> i32 {
    let mut generation = 0u64;
    let mut consecutive_failures = 0u32;
    let mut cached_initialize: Option<String> = None;
    let mut cached_initialized_notification: Option<String> = None;
    // id-key -> client framing for every request awaiting a child response.
    let mut outstanding: HashMap<String, bool> = HashMap::new();
    // id-key of a replayed initialize whose duplicate response must not
    // reach the client.
    let mut swallow_response_id: Option<String> = None;
    // A message that failed to write because the child died mid-send; it
    // never reached a child, so re-sending after respawn cannot double-run it.
    let mut pending_resend: Option<String> = None;

    let mut child = match start_child(
        &mut spawn,
        &event_tx,
        &mut generation,
        &mut consecutive_failures,
        cached_initialize.as_deref(),
        cached_initialized_notification.as_deref(),
        &mut swallow_response_id,
    ) {
        Some(child) => child,
        None => return 1,
    };

    let exit_code = loop {
        let Ok(event) = event_rx.recv() else { break 0 };
        match event {
            SupervisorEvent::FromClient(StdioEvent::Message { framed, text }) => {
                let line = text.trim_end_matches(['\r', '\n']).to_string();
                if line.is_empty() {
                    continue;
                }
                track_client_message(
                    &line,
                    framed,
                    &mut cached_initialize,
                    &mut cached_initialized_notification,
                    &mut outstanding,
                );
                if write_child_line(&mut child, &line).is_err() {
                    // The child is gone and this message never reached it.
                    pending_resend = Some(line);
                    let respawned = recover_child(
                        &mut spawn,
                        &event_tx,
                        &mut generation,
                        &mut consecutive_failures,
                        &mut child,
                        cached_initialize.as_deref(),
                        cached_initialized_notification.as_deref(),
                        &mut swallow_response_id,
                        &mut outstanding,
                        &mut client_out,
                        &mut pending_resend,
                    );
                    if !respawned {
                        break 1;
                    }
                }
            }
            SupervisorEvent::FromClient(StdioEvent::ParseError {
                framed,
                error,
                recoverable,
            }) => {
                let response = jsonrpc_error(
                    Value::Null,
                    -32700,
                    "Parse error",
                    JsonRpcErrorData::parse_error(error),
                )
                .to_string();
                if write_client_response(&mut client_out, framed, &response).is_err() {
                    break 0;
                }
                if !recoverable {
                    break 0;
                }
            }
            SupervisorEvent::FromClient(StdioEvent::Eof) => {
                break drain_child_after_client_eof(
                    &mut child,
                    &event_rx,
                    &mut swallow_response_id,
                    &mut outstanding,
                    &mut client_out,
                );
            }
            SupervisorEvent::FromChild { generation, text } => {
                if generation != child.generation {
                    // A response raced the teardown of a replaced child; its
                    // requests were already answered with retryable errors.
                    continue;
                }
                let line = text.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    continue;
                }
                if let Some(swallow_id) = swallow_response_id.as_deref() {
                    if response_id_key(line).as_deref() == Some(swallow_id) {
                        swallow_response_id = None;
                        continue;
                    }
                }
                let framed = response_framing(line, &mut outstanding);
                if write_client_response(&mut client_out, framed, line).is_err() {
                    break 0;
                }
            }
            SupervisorEvent::ChildExited {
                generation: exited_generation,
            } => {
                if exited_generation != child.generation {
                    continue;
                }
                let respawned = recover_child(
                    &mut spawn,
                    &event_tx,
                    &mut generation,
                    &mut consecutive_failures,
                    &mut child,
                    cached_initialize.as_deref(),
                    cached_initialized_notification.as_deref(),
                    &mut swallow_response_id,
                    &mut outstanding,
                    &mut client_out,
                    &mut pending_resend,
                );
                if !respawned {
                    break 1;
                }
            }
        }
    };

    (child.terminate)();
    exit_code
}

/// After the client hangs up, closes the child's stdin and keeps forwarding
/// its remaining responses until it exits, so one-shot piped sessions and
/// session teardown never lose in-flight replies.
fn drain_child_after_client_eof<W: Write>(
    child: &mut ActiveChild,
    event_rx: &mpsc::Receiver<SupervisorEvent>,
    swallow_response_id: &mut Option<String>,
    outstanding: &mut HashMap<String, bool>,
    client_out: &mut W,
) -> i32 {
    child.stdin = None;
    let deadline = Instant::now() + SHUTDOWN_DRAIN_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return 0;
        }
        match event_rx.recv_timeout(remaining) {
            Ok(SupervisorEvent::FromChild { generation, text })
                if generation == child.generation =>
            {
                let line = text.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    continue;
                }
                if let Some(swallow_id) = swallow_response_id.as_deref() {
                    if response_id_key(line).as_deref() == Some(swallow_id) {
                        *swallow_response_id = None;
                        continue;
                    }
                }
                let framed = response_framing(line, outstanding);
                if write_client_response(client_out, framed, line).is_err() {
                    return 0;
                }
            }
            Ok(SupervisorEvent::ChildExited { generation }) if generation == child.generation => {
                return 0;
            }
            Ok(_) => continue,
            Err(_) => return 0,
        }
    }
}

/// Caches the handshake messages and registers request ids so responses can
/// be correlated, framed correctly, and failed over on a child crash.
fn track_client_message(
    line: &str,
    framed: bool,
    cached_initialize: &mut Option<String>,
    cached_initialized_notification: &mut Option<String>,
    outstanding: &mut HashMap<String, bool>,
) {
    let Ok(parsed) = serde_json::from_str::<Value>(line) else {
        // Forwarded verbatim; the child answers with its own parse error.
        return;
    };
    let method = parsed.get("method").and_then(Value::as_str);
    match method {
        Some("initialize") => *cached_initialize = Some(line.to_string()),
        Some("notifications/initialized") => {
            *cached_initialized_notification = Some(line.to_string());
        }
        _ => {}
    }
    match &parsed {
        Value::Array(batch) => {
            for item in batch {
                if let Some(key) = id_key(item.get("id")) {
                    outstanding.insert(key, framed);
                }
            }
        }
        value => {
            if let Some(key) = id_key(value.get("id")) {
                outstanding.insert(key, framed);
            }
        }
    }
}

fn id_key(id: Option<&Value>) -> Option<String> {
    match id {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.to_string()),
    }
}

fn response_id_key(line: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(line).ok()?;
    id_key(parsed.get("id"))
}

/// Resolves the framing for a child response from the request that opened it
/// and clears the request from the outstanding set.
fn response_framing(line: &str, outstanding: &mut HashMap<String, bool>) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    match &parsed {
        Value::Array(batch) => {
            let mut framed = false;
            let mut framing_known = false;
            for item in batch {
                if let Some(key) = id_key(item.get("id")) {
                    if let Some(item_framed) = outstanding.remove(&key) {
                        if !framing_known {
                            framed = item_framed;
                            framing_known = true;
                        }
                    }
                }
            }
            framed
        }
        value => id_key(value.get("id"))
            .and_then(|key| outstanding.remove(&key))
            .unwrap_or(false),
    }
}

fn write_client_response<W: Write>(
    client_out: &mut W,
    framed: bool,
    response: &str,
) -> std::io::Result<()> {
    if framed {
        write_framed_jsonrpc(client_out, response)
    } else {
        writeln!(client_out, "{response}")?;
        client_out.flush()
    }
}

/// Fails over after a child death: answers every in-flight request with a
/// retryable error, respawns the child, and replays the handshake plus any
/// message the dead child never received.
#[allow(clippy::too_many_arguments)]
fn recover_child(
    spawn: &mut impl FnMut() -> std::io::Result<ChildHandles>,
    event_tx: &mpsc::Sender<SupervisorEvent>,
    generation: &mut u64,
    consecutive_failures: &mut u32,
    child: &mut ActiveChild,
    cached_initialize: Option<&str>,
    cached_initialized_notification: Option<&str>,
    swallow_response_id: &mut Option<String>,
    outstanding: &mut HashMap<String, bool>,
    client_out: &mut impl Write,
    pending_resend: &mut Option<String>,
) -> bool {
    (child.terminate)();
    if child.spawned_at.elapsed() >= STABLE_CHILD_LIFETIME {
        *consecutive_failures = 0;
    } else {
        // Crash-looping children must hit the backoff/give-up path even when
        // every individual spawn succeeds.
        *consecutive_failures += 1;
    }
    // The message that never reached the dead child will be re-sent and
    // answered for real, so it must NOT also get a retryable error here:
    // emitting both would hand the client two replies for one id (a protocol
    // violation strict clients tear the session down over). Pull its ids out
    // of the outstanding set first, preserving their original framing for the
    // resend below.
    let resend_framing = take_resend_framing(pending_resend.as_deref(), outstanding);
    for (key, framed) in outstanding.drain() {
        let id = serde_json::from_str::<Value>(&key).unwrap_or(Value::Null);
        let response = jsonrpc_error(
            id,
            -32603,
            "Internal error",
            JsonRpcErrorData::internal_error(
                "tokenzero MCP server restarted while this request was in flight; retry the call",
            ),
        )
        .to_string();
        if write_client_response(client_out, framed, &response).is_err() {
            return false;
        }
    }
    let Some(new_child) = start_child(
        spawn,
        event_tx,
        generation,
        consecutive_failures,
        cached_initialize,
        cached_initialized_notification,
        swallow_response_id,
    ) else {
        return false;
    };
    *child = new_child;
    if let Some(line) = pending_resend.take() {
        reinstate_resend_framing(&line, &resend_framing, outstanding);
        if write_child_line(child, &line).is_err() {
            *pending_resend = Some(line);
            return recover_child(
                spawn,
                event_tx,
                generation,
                consecutive_failures,
                child,
                cached_initialize,
                cached_initialized_notification,
                swallow_response_id,
                outstanding,
                client_out,
                pending_resend,
            );
        }
    }
    true
}

/// Removes the resent message's ids from the outstanding set (so the failover
/// drain does not emit a retryable error for them) and returns their original
/// framing, keyed by id, for re-registration when the message is resent.
fn take_resend_framing(
    pending_resend: Option<&str>,
    outstanding: &mut HashMap<String, bool>,
) -> HashMap<String, bool> {
    match pending_resend {
        Some(line) => message_id_keys(line)
            .into_iter()
            .filter_map(|key| outstanding.remove(&key).map(|framed| (key, framed)))
            .collect(),
        None => HashMap::new(),
    }
}

/// Every request id carried by a (possibly batched) client message.
fn message_id_keys(line: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match &parsed {
        Value::Array(batch) => batch
            .iter()
            .filter_map(|item| id_key(item.get("id")))
            .collect(),
        value => id_key(value.get("id")).into_iter().collect(),
    }
}

/// Re-registers a re-sent message's ids ahead of writing it to the new child,
/// restoring the framing each id arrived with so the eventual response is
/// framed the way the client expects.
fn reinstate_resend_framing(
    line: &str,
    resend_framing: &HashMap<String, bool>,
    outstanding: &mut HashMap<String, bool>,
) {
    for key in message_id_keys(line) {
        let framed = resend_framing.get(&key).copied().unwrap_or(false);
        outstanding.insert(key, framed);
    }
}

fn start_child(
    spawn: &mut impl FnMut() -> std::io::Result<ChildHandles>,
    event_tx: &mpsc::Sender<SupervisorEvent>,
    generation: &mut u64,
    consecutive_failures: &mut u32,
    cached_initialize: Option<&str>,
    cached_initialized_notification: Option<&str>,
    swallow_response_id: &mut Option<String>,
) -> Option<ActiveChild> {
    loop {
        if *consecutive_failures >= MAX_CONSECUTIVE_SPAWN_FAILURES {
            return None;
        }
        if *consecutive_failures > 0 {
            let exponent = (*consecutive_failures - 1).min(16);
            let backoff = BASE_RESPAWN_BACKOFF
                .saturating_mul(1u32 << exponent)
                .min(MAX_RESPAWN_BACKOFF);
            thread::sleep(backoff);
        }
        let handles = match spawn() {
            Ok(handles) => handles,
            Err(_) => {
                *consecutive_failures += 1;
                continue;
            }
        };
        *generation += 1;
        let child_generation = *generation;
        let mut active = ActiveChild {
            stdin: Some(handles.stdin),
            terminate: handles.terminate,
            spawned_at: Instant::now(),
            generation: child_generation,
        };
        pump_child_stdout(handles.stdout, event_tx.clone(), child_generation);
        if let Some(initialize) = cached_initialize {
            *swallow_response_id = serde_json::from_str::<Value>(initialize)
                .ok()
                .and_then(|parsed| id_key(parsed.get("id")));
            if write_child_line(&mut active, initialize).is_err() {
                (active.terminate)();
                *consecutive_failures += 1;
                continue;
            }
            if let Some(initialized) = cached_initialized_notification {
                if write_child_line(&mut active, initialized).is_err() {
                    (active.terminate)();
                    *consecutive_failures += 1;
                    continue;
                }
            }
        }
        return Some(active);
    }
}

/// Streams child stdout lines into the supervisor event loop; sends
/// ChildExited when the child closes its end of the pipe.
fn pump_child_stdout(
    stdout: Box<dyn Read + Send>,
    event_tx: mpsc::Sender<SupervisorEvent>,
    generation: u64,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let (raw_tx, raw_rx) = mpsc::channel();
        let forward = thread::spawn(move || {
            read_stdio_events_from_reader(&mut reader, raw_tx);
        });
        while let Ok(event) = raw_rx.recv() {
            match event {
                StdioEvent::Message { text, .. } => {
                    let event = SupervisorEvent::FromChild { generation, text };
                    if event_tx.send(event).is_err() {
                        return;
                    }
                }
                StdioEvent::ParseError { .. } => continue,
                StdioEvent::Eof => break,
            }
        }
        let _ = forward.join();
        let _ = event_tx.send(SupervisorEvent::ChildExited { generation });
    });
}

#[cfg(test)]
mod tests;
