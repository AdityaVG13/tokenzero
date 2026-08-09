//! Crash-transparent supervisor for the stdio MCP server.
//!
//! The supervisor process owns the client-facing stdio pipes for the entire
//! session and proxies JSON-RPC messages to an inner server child process.
//! If the child ever dies — panic-abort, OOM kill, anything the in-process
//! hardening cannot absorb — the supervisor respawns it with backoff, replays
//! the cached MCP `initialize` handshake so the new child is immediately
//! usable, answers the requests that were in flight with a retryable error,
//! and keeps proxying. The client never observes a disconnect.

use crate::stdio::{
    StdioEvent, read_stdio_events_from_reader, write_jsonrpc_response as write_stdio_response,
};
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

/// Runs the supervisor until client input stops or a forced shutdown occurs.
///
/// Client EOF joins both input pumps and returns. A forced shutdown exits after
/// reaping the child because a blocking stdin read cannot be cancelled portably.
pub fn run_supervised_stdio(program: OsString, child_args: Vec<OsString>) -> i32 {
    let (event_tx, event_rx) = mpsc::channel();
    let client_tx = event_tx.clone();
    let client_pumps = spawn_client_pumps(client_tx, move |raw_tx| {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        read_stdio_events_from_reader(&mut reader, raw_tx);
    });
    let spawner = move || spawn_server_child(&program, &child_args);
    let outcome = run_supervisor_loop(spawner, event_tx, event_rx, std::io::stdout());
    if outcome.client_stopped {
        return match client_pumps.join() {
            Ok(()) => outcome.exit_code,
            Err(message) => {
                eprintln!("{message}");
                1
            }
        };
    }

    // A blocking std::io::stdin read cannot be cancelled portably. The loop
    // has already terminated any child it owned; exit here rather than return
    // while either client pump remains detached.
    std::process::exit(outcome.exit_code)
}

struct ClientPumps {
    reader: thread::JoinHandle<()>,
    forwarder: thread::JoinHandle<()>,
}

impl ClientPumps {
    fn join(self) -> Result<(), &'static str> {
        let reader = self.reader.join();
        let forwarder = self.forwarder.join();
        if reader.is_err() {
            return Err("TokenZero MCP supervisor stdin reader panicked during shutdown");
        }
        if forwarder.is_err() {
            return Err("TokenZero MCP supervisor stdin forwarder panicked during shutdown");
        }
        Ok(())
    }
}

fn spawn_client_pumps(
    client_tx: mpsc::Sender<SupervisorEvent>,
    read: impl FnOnce(mpsc::Sender<StdioEvent>) + Send + 'static,
) -> ClientPumps {
    let (raw_tx, raw_rx) = mpsc::channel();
    let reader = thread::spawn(move || read(raw_tx));
    let forwarder = thread::spawn(move || {
        while let Ok(event) = raw_rx.recv() {
            if client_tx.send(SupervisorEvent::FromClient(event)).is_err() {
                break;
            }
        }
    });
    ClientPumps { reader, forwarder }
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
    /// `None` after client EOF, while in-flight responses drain.
    stdin: Option<Box<dyn Write + Send>>,
    terminate: Box<dyn FnMut() + Send>,
    spawned_at: Instant,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SupervisorLoopOutcome {
    exit_code: i32,
    client_stopped: bool,
}

impl SupervisorLoopOutcome {
    const fn client(exit_code: i32) -> Self {
        Self {
            exit_code,
            client_stopped: true,
        }
    }

    const fn forced(exit_code: i32) -> Self {
        Self {
            exit_code,
            client_stopped: false,
        }
    }
}

#[derive(Default)]
struct SupervisorState {
    generation: u64,
    consecutive_failures: u32,
    cached_initialize: Option<String>,
    cached_initialized_notification: Option<String>,
    outstanding: HashMap<String, bool>,
    swallow_response_id: Option<String>,
    pending_resend: Option<String>,
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
) -> SupervisorLoopOutcome {
    let mut state = SupervisorState::default();
    let mut child = match start_child(&mut spawn, &event_tx, &mut state) {
        Some(child) => child,
        None => return SupervisorLoopOutcome::forced(1),
    };

    let outcome = loop {
        let Ok(event) = event_rx.recv() else {
            break SupervisorLoopOutcome::forced(0);
        };
        match event {
            SupervisorEvent::FromClient(StdioEvent::Message { framed, text }) => {
                let line = text.trim_end_matches(['\r', '\n']).to_string();
                if line.is_empty() {
                    continue;
                }
                track_client_message(&line, framed, &mut state);
                if write_child_line(&mut child, &line).is_err() {
                    // The child is gone and this message never reached it.
                    state.pending_resend = Some(line);
                    let respawned = recover_child(
                        &mut spawn,
                        &event_tx,
                        &mut child,
                        &mut state,
                        &mut client_out,
                    );
                    if !respawned {
                        break SupervisorLoopOutcome::forced(1);
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
                if write_stdio_response(&mut client_out, framed, &response).is_err() {
                    break SupervisorLoopOutcome::forced(1);
                }
                if !recoverable {
                    break SupervisorLoopOutcome::client(1);
                }
            }
            SupervisorEvent::FromClient(StdioEvent::Eof) => {
                let exit_code = drain_child_after_client_eof(
                    &mut child,
                    &event_rx,
                    &mut state,
                    &mut client_out,
                );
                break SupervisorLoopOutcome::client(exit_code);
            }
            SupervisorEvent::FromClient(StdioEvent::OutputFailed) => {
                break SupervisorLoopOutcome::forced(1);
            }
            SupervisorEvent::FromChild { generation, text } => {
                if forward_child_response(
                    generation,
                    &text,
                    child.generation,
                    &mut state,
                    &mut client_out,
                )
                .is_err()
                {
                    break SupervisorLoopOutcome::forced(1);
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
                    &mut child,
                    &mut state,
                    &mut client_out,
                );
                if !respawned {
                    break SupervisorLoopOutcome::forced(1);
                }
            }
        }
    };

    (child.terminate)();
    outcome
}

/// After the client hangs up, closes the child's stdin and keeps forwarding
/// its remaining responses until it exits, so one-shot piped sessions and
/// session teardown never lose in-flight replies.
fn forward_child_response<W: Write>(
    generation: u64,
    text: &str,
    active_generation: u64,
    state: &mut SupervisorState,
    client_out: &mut W,
) -> std::io::Result<()> {
    if generation != active_generation {
        return Ok(());
    }
    let line = text.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return Ok(());
    }
    let swallow = state
        .swallow_response_id
        .as_deref()
        .is_some_and(|id| response_id_key(line).as_deref() == Some(id));
    if swallow {
        state.swallow_response_id = None;
        return Ok(());
    }
    let framed = response_framing(line, &mut state.outstanding);
    write_stdio_response(client_out, framed, line)
}

fn drain_child_after_client_eof<W: Write>(
    child: &mut ActiveChild,
    event_rx: &mpsc::Receiver<SupervisorEvent>,
    state: &mut SupervisorState,
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
            Ok(SupervisorEvent::FromChild { generation, text }) => {
                if forward_child_response(generation, &text, child.generation, state, client_out)
                    .is_err()
                {
                    return 1;
                }
            }
            Ok(SupervisorEvent::ChildExited { generation }) if generation == child.generation => {
                return 0;
            }
            Ok(_) => {}
            Err(_) => return 0,
        }
    }
}

/// Caches the handshake messages and registers request ids so responses can
/// be correlated, framed correctly, and failed over on a child crash.
fn track_client_message(line: &str, framed: bool, state: &mut SupervisorState) {
    let Ok(parsed) = serde_json::from_str::<Value>(line) else {
        return;
    };
    match parsed.get("method").and_then(Value::as_str) {
        Some("initialize") => state.cached_initialize = Some(line.to_string()),
        Some("notifications/initialized") => {
            state.cached_initialized_notification = Some(line.to_string());
        }
        _ => {}
    }
    for key in value_id_keys(&parsed) {
        state.outstanding.insert(key, framed);
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
    let mut framing = None;
    for key in value_id_keys(&parsed) {
        if let Some(item_framing) = outstanding.remove(&key) {
            framing.get_or_insert(item_framing);
        }
    }
    framing.unwrap_or(false)
}

/// Fails over after a child death: answers every in-flight request with a
/// retryable error, respawns the child, and replays the handshake plus any
/// message the dead child never received.
fn recover_child(
    spawn: &mut impl FnMut() -> std::io::Result<ChildHandles>,
    event_tx: &mpsc::Sender<SupervisorEvent>,
    child: &mut ActiveChild,
    state: &mut SupervisorState,
    client_out: &mut impl Write,
) -> bool {
    (child.terminate)();
    if child.spawned_at.elapsed() >= STABLE_CHILD_LIFETIME {
        state.consecutive_failures = 0;
    } else {
        state.consecutive_failures += 1;
    }
    let resend_framing =
        take_resend_framing(state.pending_resend.as_deref(), &mut state.outstanding);
    for (key, framed) in state.outstanding.drain() {
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
        if write_stdio_response(client_out, framed, &response).is_err() {
            return false;
        }
    }
    let Some(new_child) = start_child(spawn, event_tx, state) else {
        return false;
    };
    *child = new_child;
    if let Some(line) = state.pending_resend.take() {
        reinstate_resend_framing(&line, &resend_framing, &mut state.outstanding);
        if write_child_line(child, &line).is_err() {
            state.pending_resend = Some(line);
            return recover_child(spawn, event_tx, child, state, client_out);
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
    pending_resend
        .into_iter()
        .flat_map(message_id_keys)
        .filter_map(|key| outstanding.remove(&key).map(|framed| (key, framed)))
        .collect()
}

/// Every request id carried by a (possibly batched) client message.
fn message_id_keys(line: &str) -> Vec<String> {
    serde_json::from_str::<Value>(line)
        .map(|value| value_id_keys(&value))
        .unwrap_or_default()
}

fn value_id_keys(value: &Value) -> Vec<String> {
    match value {
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
    state: &mut SupervisorState,
) -> Option<ActiveChild> {
    loop {
        if state.consecutive_failures >= MAX_CONSECUTIVE_SPAWN_FAILURES {
            return None;
        }
        if state.consecutive_failures > 0 {
            let exponent = (state.consecutive_failures - 1).min(16);
            let backoff = BASE_RESPAWN_BACKOFF
                .saturating_mul(1u32 << exponent)
                .min(MAX_RESPAWN_BACKOFF);
            thread::sleep(backoff);
        }
        let handles = match spawn() {
            Ok(handles) => handles,
            Err(_) => {
                state.consecutive_failures += 1;
                continue;
            }
        };
        state.generation += 1;
        let generation = state.generation;
        let mut active = ActiveChild {
            stdin: Some(handles.stdin),
            terminate: handles.terminate,
            spawned_at: Instant::now(),
            generation,
        };
        pump_child_stdout(handles.stdout, event_tx.clone(), generation);
        if let Some(initialize) = state.cached_initialize.as_deref() {
            state.swallow_response_id = serde_json::from_str::<Value>(initialize)
                .ok()
                .and_then(|parsed| id_key(parsed.get("id")));
            if write_child_line(&mut active, initialize).is_err() {
                (active.terminate)();
                state.consecutive_failures += 1;
                continue;
            }
            if let Some(initialized) = state.cached_initialized_notification.as_deref() {
                if write_child_line(&mut active, initialized).is_err() {
                    (active.terminate)();
                    state.consecutive_failures += 1;
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
                StdioEvent::Eof | StdioEvent::OutputFailed => break,
            }
        }
        let _ = forward.join();
        let _ = event_tx.send(SupervisorEvent::ChildExited { generation });
    });
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn client_pumps_forward_input_and_join_after_eof() {
        let (client_tx, client_rx) = mpsc::channel();
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n".to_vec();
        let pumps = spawn_client_pumps(client_tx, move |raw_tx| {
            let mut reader = BufReader::new(Cursor::new(input));
            read_stdio_events_from_reader(&mut reader, raw_tx);
        });

        match client_rx.recv().expect("message event") {
            SupervisorEvent::FromClient(StdioEvent::Message { text, .. }) => {
                assert!(text.contains("\"method\":\"ping\""));
            }
            _ => panic!("expected a forwarded client message"),
        }
        assert!(matches!(
            client_rx.recv().expect("EOF event"),
            SupervisorEvent::FromClient(StdioEvent::Eof)
        ));
        assert_eq!(pumps.join(), Ok(()));
    }

    #[test]
    fn client_eof_selects_the_joinable_shutdown_path() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(SupervisorEvent::FromClient(StdioEvent::Eof))
            .expect("queue EOF");
        let spawn = || {
            Ok(ChildHandles {
                stdin: Box::new(std::io::sink()),
                stdout: Box::new(Cursor::new(Vec::<u8>::new())),
                terminate: Box::new(|| {}),
            })
        };

        let outcome = run_supervisor_loop(spawn, event_tx, event_rx, Vec::<u8>::new());

        assert_eq!(outcome, SupervisorLoopOutcome::client(0));
    }

    #[test]
    fn client_pumps_join_after_unrecoverable_framed_parse_error() {
        let (client_tx, client_rx) = mpsc::channel();
        let input = b"Content-Length: 5\r\n\r\n{}".to_vec();
        let pumps = spawn_client_pumps(client_tx, move |raw_tx| {
            let mut reader = BufReader::new(Cursor::new(input));
            read_stdio_events_from_reader(&mut reader, raw_tx);
        });

        assert!(matches!(
            client_rx.recv().expect("parse-error event"),
            SupervisorEvent::FromClient(StdioEvent::ParseError {
                recoverable: false,
                ..
            })
        ));
        assert_eq!(pumps.join(), Ok(()));
    }

    #[test]
    fn unrecoverable_framed_parse_error_terminates_with_exit_one() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(SupervisorEvent::FromClient(StdioEvent::ParseError {
                framed: true,
                error: "bad frame".to_string(),
                recoverable: false,
            }))
            .unwrap();
        let spawn = || {
            Ok(ChildHandles {
                stdin: Box::new(std::io::sink()),
                stdout: Box::new(Cursor::new(Vec::<u8>::new())),
                terminate: Box::new(|| {}),
            })
        };
        let outcome = run_supervisor_loop(spawn, event_tx, event_rx, Vec::<u8>::new());
        assert_eq!(outcome, SupervisorLoopOutcome::client(1));
    }

    #[test]
    fn recoverable_parse_error_continues_not_terminates() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(SupervisorEvent::FromClient(StdioEvent::ParseError {
                framed: false,
                error: "{bad json".to_string(),
                recoverable: true,
            }))
            .unwrap();
        event_tx
            .send(SupervisorEvent::FromClient(StdioEvent::Eof))
            .unwrap();
        let spawn = || {
            Ok(ChildHandles {
                stdin: Box::new(std::io::sink()),
                stdout: Box::new(Cursor::new(Vec::<u8>::new())),
                terminate: Box::new(|| {}),
            })
        };
        let outcome = run_supervisor_loop(spawn, event_tx, event_rx, Vec::<u8>::new());
        assert_eq!(outcome, SupervisorLoopOutcome::client(0));
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "client output failed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn parse_error_client_write_failure_exits_one() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(SupervisorEvent::FromClient(StdioEvent::ParseError {
                framed: false,
                error: "bad json".to_string(),
                recoverable: true,
            }))
            .unwrap();
        let spawn = || {
            Ok(ChildHandles {
                stdin: Box::new(std::io::sink()),
                stdout: Box::new(Cursor::new(Vec::<u8>::new())),
                terminate: Box::new(|| {}),
            })
        };
        let outcome = run_supervisor_loop(spawn, event_tx, event_rx, FailingWriter);
        assert_eq!(outcome, SupervisorLoopOutcome::forced(1));
    }

    #[test]
    fn child_response_forward_failure_exits_one() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(SupervisorEvent::FromChild {
                generation: 1,
                text: r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_string(),
            })
            .unwrap();
        let spawn = || {
            Ok(ChildHandles {
                stdin: Box::new(std::io::sink()),
                stdout: Box::new(Cursor::new(Vec::<u8>::new())),
                terminate: Box::new(|| {}),
            })
        };
        let outcome = run_supervisor_loop(spawn, event_tx, event_rx, FailingWriter);
        assert_eq!(outcome, SupervisorLoopOutcome::forced(1));
    }

    #[test]
    fn drain_child_response_forward_failure_exits_one() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(SupervisorEvent::FromClient(StdioEvent::Eof))
            .unwrap();
        event_tx
            .send(SupervisorEvent::FromChild {
                generation: 1,
                text: r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_string(),
            })
            .unwrap();
        let spawn = || {
            Ok(ChildHandles {
                stdin: Box::new(std::io::sink()),
                stdout: Box::new(Cursor::new(Vec::<u8>::new())),
                terminate: Box::new(|| {}),
            })
        };
        let outcome = run_supervisor_loop(spawn, event_tx, event_rx, FailingWriter);
        assert_eq!(outcome, SupervisorLoopOutcome::client(1));
    }

    #[test]
    fn client_pumps_join_forwarder_after_reader_panic() {
        let (client_tx, _client_rx) = mpsc::channel();
        let pumps = spawn_client_pumps(client_tx, |_raw_tx| panic!("reader panic fixture"));

        assert_eq!(
            pumps.join(),
            Err("TokenZero MCP supervisor stdin reader panicked during shutdown")
        );
    }
}
