use crate::{EngineConfig, JsonRpcErrorData, TokenZeroEngine, handle_jsonrpc_value, jsonrpc_error};
use serde_json::Value;
use std::io::{BufRead, BufReader, Error, ErrorKind, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

pub(crate) const MAX_MCP_STDIO_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_MCP_STDIO_LINE_BYTES: usize = MAX_MCP_STDIO_FRAME_BYTES + 2;
pub(crate) const MAX_MCP_STDIO_HEADER_LINE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_MCP_STDIO_HEADER_SECTION_BYTES: usize = 256 * 1024;
/// Heavy tool calls run on this many worker threads so lightweight liveness
/// traffic (ping, initialize, tools/list) is never starved by a slow tool.
pub(crate) const MCP_TOOL_WORKER_THREADS: usize = 4;

/// Run the hand-rolled MCP stdio server.
///
/// Normal client EOF joins the stdin reader and returns an exit code. A forced
/// shutdown exits the process after draining workers because a blocking stdin
/// read cannot be cancelled portably without leaving a detached reader.
pub fn run_stdio(config: EngineConfig) -> i32 {
    let (event_tx, event_rx) = mpsc::channel();
    let reader_tx = event_tx.clone();
    let reader_thread = thread::spawn(move || read_stdio_events(reader_tx));
    let outcome = run_stdio_core(config, event_tx, event_rx, std::io::stdout());
    if outcome.stdin_stopped {
        return match reader_thread.join() {
            Ok(()) => outcome.exit_code,
            Err(_) => {
                eprintln!("TokenZero MCP stdin reader panicked during shutdown");
                1
            }
        };
    }

    // A blocking std::io::stdin read cannot be cancelled portably. The only
    // non-input shutdowns are an explicit idle timeout or a broken downstream
    // transport, and the CLI caller exits immediately after this entry point.
    // Exit here rather than return and silently detach the live reader.
    std::process::exit(outcome.exit_code)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StdioLoopOutcome {
    exit_code: i32,
    stdin_stopped: bool,
}

/// Transport-agnostic server loop. Lightweight JSON-RPC methods are answered
/// inline; tools/call requests and batches are dispatched to a worker pool so
/// a long-running tool cannot block keepalive pings. All responses funnel
/// through a single writer thread that owns the output stream.
fn run_stdio_core<W: Write + Send + 'static>(
    config: EngineConfig,
    event_tx: mpsc::Sender<StdioEvent>,
    events: mpsc::Receiver<StdioEvent>,
    writer: W,
) -> StdioLoopOutcome {
    let engine = Arc::new(TokenZeroEngine::new(config));
    let idle_timeout = engine.config.mcp_idle_timeout;

    let (response_tx, response_rx) = mpsc::channel::<OutgoingResponse>();
    let writer_thread = thread::spawn(move || {
        let outcome = match catch_unwind(AssertUnwindSafe(|| {
            write_stdio_responses(writer, response_rx)
        })) {
            Ok(Ok(())) => WriterOutcome::Clean,
            Ok(Err(error)) => WriterOutcome::Failed(error.to_string()),
            Err(_) => WriterOutcome::Panicked,
        };
        if !matches!(outcome, WriterOutcome::Clean) {
            let _ = event_tx.send(StdioEvent::OutputFailed);
        }
        outcome
    });

    let (work_tx, work_rx) = mpsc::channel::<WorkItem>();
    let work_rx = Arc::new(Mutex::new(work_rx));
    let mut worker_threads = Vec::with_capacity(MCP_TOOL_WORKER_THREADS);
    for _ in 0..MCP_TOOL_WORKER_THREADS {
        let work_rx = Arc::clone(&work_rx);
        let engine = Arc::clone(&engine);
        let response_tx = response_tx.clone();
        worker_threads.push(thread::spawn(move || {
            loop {
                let item = {
                    let Ok(receiver) = work_rx.lock() else {
                        break;
                    };
                    receiver.recv()
                };
                let Ok(item) = item else {
                    break;
                };
                if let Some(text) = dispatch_jsonrpc(&engine, item.message) {
                    let outgoing = OutgoingResponse {
                        framed: item.framed,
                        text,
                    };
                    if response_tx.send(outgoing).is_err() {
                        break;
                    }
                }
            }
        }));
    }

    let mut exit_code = 0;
    let stdin_stopped = 'server: loop {
        let event = match idle_timeout {
            Some(timeout) => match events.recv_timeout(timeout) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => break 'server false,
                Err(mpsc::RecvTimeoutError::Disconnected) => break 'server true,
            },
            None => match events.recv() {
                Ok(event) => event,
                Err(_) => break 'server true,
            },
        };

        match event {
            StdioEvent::Message { framed, text } => {
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        if send_parse_error(&response_tx, framed, err.to_string()).is_err() {
                            break 'server false;
                        }
                        continue;
                    }
                };
                if needs_tool_worker(&parsed) {
                    if work_tx
                        .send(WorkItem {
                            framed,
                            message: parsed,
                        })
                        .is_err()
                    {
                        break 'server false;
                    }
                } else if let Some(text) = dispatch_jsonrpc(&engine, parsed) {
                    let outgoing = OutgoingResponse { framed, text };
                    if response_tx.send(outgoing).is_err() {
                        break 'server false;
                    }
                }
            }
            StdioEvent::ParseError {
                framed,
                error,
                recoverable,
            } => {
                if send_parse_error(&response_tx, framed, error).is_err() {
                    break 'server false;
                }
                if !recoverable {
                    break 'server true;
                }
            }
            StdioEvent::Eof => break 'server true,
            StdioEvent::OutputFailed => {
                exit_code = 1;
                break 'server false;
            }
        }
    };

    // Orderly shutdown: let in-flight tool calls finish and flush their
    // responses before the process exits.
    drop(work_tx);
    for worker in worker_threads {
        let _ = worker.join();
    }
    drop(response_tx);
    let writer_outcome = match writer_thread.join() {
        Ok(outcome) => outcome,
        Err(_) => WriterOutcome::Panicked,
    };
    match writer_outcome {
        WriterOutcome::Clean => {}
        WriterOutcome::Failed(error) => {
            eprintln!("TokenZero MCP stdout writer failed: {error}");
            exit_code = 1;
        }
        WriterOutcome::Panicked => {
            eprintln!("TokenZero MCP stdout writer panicked during shutdown");
            exit_code = 1;
        }
    }
    StdioLoopOutcome {
        exit_code,
        stdin_stopped,
    }
}

struct WorkItem {
    framed: bool,
    message: Value,
}

struct OutgoingResponse {
    framed: bool,
    text: String,
}

enum WriterOutcome {
    Clean,
    Failed(String),
    Panicked,
}

/// tools/call requests and batches go to the worker pool; everything else
/// (ping, initialize, lists, logging, resources) is cheap enough to answer
/// inline without risking liveness.
fn needs_tool_worker(message: &Value) -> bool {
    match message {
        Value::Array(_) => true,
        Value::Object(object) => object.get("method").and_then(Value::as_str) == Some("tools/call"),
        _ => false,
    }
}

/// Handles one parsed JSON-RPC message with panic isolation: a panicking tool
/// yields an Internal error response instead of killing the server.
fn dispatch_jsonrpc(engine: &TokenZeroEngine, message: Value) -> Option<String> {
    let panic_id = message
        .as_object()
        .and_then(|object| object.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    match catch_unwind(AssertUnwindSafe(|| handle_jsonrpc_value(engine, message))) {
        Ok(response) => response.map(|value| value.to_string()),
        Err(panic) => Some(
            jsonrpc_error(
                panic_id,
                -32603,
                "Internal error",
                JsonRpcErrorData::internal_error(panic_text(panic.as_ref())),
            )
            .to_string(),
        ),
    }
}

fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn send_parse_error(
    response_tx: &mpsc::Sender<OutgoingResponse>,
    framed: bool,
    error: String,
) -> Result<(), mpsc::SendError<OutgoingResponse>> {
    let text = jsonrpc_error(
        Value::Null,
        -32700,
        "Parse error",
        JsonRpcErrorData::parse_error(error),
    )
    .to_string();
    response_tx.send(OutgoingResponse { framed, text })
}

/// Single owner of the output stream. Stops draining only when the stream
/// itself fails (client side of the pipe is gone).
fn write_stdio_responses<W: Write>(
    mut writer: W,
    responses: mpsc::Receiver<OutgoingResponse>,
) -> std::io::Result<()> {
    while let Ok(response) = responses.recv() {
        write_jsonrpc_response(&mut writer, response.framed, &response.text)?;
    }
    Ok(())
}

pub(crate) fn write_jsonrpc_response<W: Write>(
    writer: &mut W,
    framed: bool,
    response: &str,
) -> std::io::Result<()> {
    if framed {
        write_framed_jsonrpc(writer, response)
    } else {
        writeln!(writer, "{response}").and_then(|_| flush_retry(writer))
    }
}

fn flush_retry<W: Write>(writer: &mut W) -> std::io::Result<()> {
    loop {
        match writer.flush() {
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[derive(Debug)]
pub(crate) enum StdioEvent {
    Message {
        framed: bool,
        text: String,
    },
    ParseError {
        framed: bool,
        error: String,
        recoverable: bool,
    },
    Eof,
    /// Internal wakeup emitted when the response writer fails or panics.
    OutputFailed,
}

fn read_stdio_events(tx: mpsc::Sender<StdioEvent>) {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    read_stdio_events_from_reader(&mut reader, tx);
}

pub(crate) fn read_stdio_events_from_reader<R: BufRead>(
    reader: &mut R,
    tx: mpsc::Sender<StdioEvent>,
) {
    loop {
        let framed = match fill_buf_retry(reader) {
            Ok([]) => {
                let _ = tx.send(StdioEvent::Eof);
                break;
            }
            Ok(buffer) => starts_with_framed_header(buffer),
            Err(err) => {
                let _ = tx.send(StdioEvent::ParseError {
                    framed: false,
                    error: err.to_string(),
                    recoverable: false,
                });
                break;
            }
        };
        let message = if framed {
            read_framed_jsonrpc(reader)
        } else {
            read_unframed_jsonrpc_line(reader)
        };
        match message {
            Ok(Some(text)) if !framed && text.trim().is_empty() => {}
            Ok(Some(text)) => {
                if tx.send(StdioEvent::Message { framed, text }).is_err() {
                    break;
                }
            }
            Ok(None) => {
                let _ = tx.send(StdioEvent::Eof);
                break;
            }
            Err(err) => {
                let recoverable = !framed;
                if tx
                    .send(StdioEvent::ParseError {
                        framed,
                        error: err.to_string(),
                        recoverable,
                    })
                    .is_err()
                    || !recoverable
                {
                    break;
                }
            }
        }
    }
}

fn invalid_data(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, message.into())
}

fn unexpected_eof(message: &'static str) -> Error {
    Error::new(ErrorKind::UnexpectedEof, message)
}

fn starts_with_framed_header(buffer: &[u8]) -> bool {
    [b"content-length:" as &[u8], b"content-type:"]
        .iter()
        .any(|prefix| {
            let compare_len = buffer.len().min(prefix.len());
            compare_len > 0 && buffer[..compare_len].eq_ignore_ascii_case(&prefix[..compare_len])
        })
}

pub(crate) fn read_framed_jsonrpc<R: BufRead>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut content_length = None;
    let mut header_line = Vec::new();
    let mut header_bytes = 0usize;
    loop {
        let bytes = read_mcp_header_line(reader, &mut header_line)?;
        if bytes == 0 {
            if header_bytes == 0 {
                return Ok(None);
            }
            return Err(unexpected_eof(
                "MCP stdio frame ended before header terminator",
            ));
        }
        if !header_line.ends_with(b"\n") {
            return Err(unexpected_eof("MCP stdio header line ended before newline"));
        }
        header_bytes = header_bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid_data("MCP stdio header section length overflow"))?;
        if header_bytes > MAX_MCP_STDIO_HEADER_SECTION_BYTES {
            return Err(invalid_data(format!(
                "MCP stdio header section exceeds maximum {MAX_MCP_STDIO_HEADER_SECTION_BYTES}"
            )));
        }
        let header = std::str::from_utf8(&header_line)
            .map_err(|err| invalid_data(err.to_string()))?
            .trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(invalid_data(format!("invalid MCP stdio header: {header}")));
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(invalid_data("duplicate Content-Length header"));
            }
            let parsed = value
                .trim()
                .parse::<usize>()
                .map_err(|err| invalid_data(format!("invalid Content-Length header: {err}")))?;
            if parsed > MAX_MCP_STDIO_FRAME_BYTES {
                return Err(invalid_data(format!(
                    "Content-Length {parsed} exceeds maximum {MAX_MCP_STDIO_FRAME_BYTES}"
                )));
            }
            content_length = Some(parsed);
        }
    }

    let Some(content_length) = content_length else {
        return Err(invalid_data("missing Content-Length header"));
    };
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|err| invalid_data(format!("invalid UTF-8 body: {err}")))
}

pub(crate) fn read_unframed_jsonrpc_line<R: BufRead>(
    reader: &mut R,
) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    let bytes = read_bounded_stdio_line(
        reader,
        &mut line,
        MAX_MCP_STDIO_LINE_BYTES,
        "MCP stdio unframed line",
    )?;
    if bytes == 0 {
        return Ok(None);
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|err| invalid_data(format!("invalid UTF-8 line: {err}")))
}

fn read_mcp_header_line<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> std::io::Result<usize> {
    read_bounded_stdio_line(
        reader,
        line,
        MAX_MCP_STDIO_HEADER_LINE_BYTES,
        "MCP stdio header line",
    )
}

fn read_bounded_stdio_line<R: BufRead>(
    reader: &mut R,
    line: &mut Vec<u8>,
    max_bytes: usize,
    label: &str,
) -> std::io::Result<usize> {
    line.clear();
    loop {
        let buffer = fill_buf_retry(reader)?;
        if buffer.is_empty() {
            return Ok(if line.is_empty() { 0 } else { line.len() });
        }
        let bytes_to_consume = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |position| position + 1);
        let next_len = line
            .len()
            .checked_add(bytes_to_consume)
            .ok_or_else(|| invalid_data(format!("{label} length overflow")))?;
        if next_len > max_bytes {
            drain_to_line_boundary(reader)?;
            return Err(invalid_data(format!("{label} exceeds maximum {max_bytes}")));
        }
        line.extend_from_slice(&buffer[..bytes_to_consume]);
        reader.consume(bytes_to_consume);
        if line.ends_with(b"\n") {
            return Ok(line.len());
        }
    }
}

/// Consumes input through the next newline (or EOF) so a rejected oversized
/// line leaves the stream positioned at a clean message boundary.
fn drain_to_line_boundary<R: BufRead>(reader: &mut R) -> std::io::Result<()> {
    loop {
        let buffer = fill_buf_retry(reader)?;
        if buffer.is_empty() {
            return Ok(());
        }
        match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => {
                reader.consume(position + 1);
                return Ok(());
            }
            None => {
                let length = buffer.len();
                reader.consume(length);
            }
        }
    }
}

fn fill_buf_retry<R: BufRead>(reader: &mut R) -> std::io::Result<&[u8]> {
    loop {
        // Polonius-style workaround: probe in a scoped borrow, then re-borrow.
        match reader.fill_buf() {
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
            Ok(_) => break,
        }
    }
    reader.fill_buf()
}

pub(crate) fn write_framed_jsonrpc<W: Write>(
    writer: &mut W,
    response: &str,
) -> std::io::Result<()> {
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        response.len(),
        response
    )?;
    writer.flush()
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::time::Duration;

    struct FailingWriter;
    struct PanickingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(Error::new(ErrorKind::BrokenPipe, "closed stdout fixture"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Write for PanickingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            panic!("stdout panic fixture")
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn stdout_failure_wakes_loop_without_idle_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let config = EngineConfig::for_root(dir.path());
        assert_eq!(config.mcp_idle_timeout, None);
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(StdioEvent::Message {
                framed: false,
                text: r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_string(),
            })
            .unwrap();

        let (outcome_tx, outcome_rx) = mpsc::channel();
        let core = thread::spawn(move || {
            let outcome = run_stdio_core(config, event_tx, event_rx, FailingWriter);
            let _ = outcome_tx.send(outcome);
        });
        let outcome = outcome_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("stdout failure must wake the loop without an idle timeout");
        core.join().expect("stdio core thread");

        assert_eq!(
            outcome,
            StdioLoopOutcome {
                exit_code: 1,
                stdin_stopped: false,
            }
        );
    }

    #[test]
    fn stdout_panic_wakes_loop_without_idle_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let config = EngineConfig::for_root(dir.path());
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(StdioEvent::Message {
                framed: false,
                text: r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_string(),
            })
            .unwrap();
        let (outcome_tx, outcome_rx) = mpsc::channel();
        let core = thread::spawn(move || {
            let outcome = run_stdio_core(config, event_tx, event_rx, PanickingWriter);
            let _ = outcome_tx.send(outcome);
        });
        let outcome = outcome_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("stdout panic must wake the loop without an idle timeout");
        core.join().expect("stdio core thread");

        assert_eq!(
            outcome,
            StdioLoopOutcome {
                exit_code: 1,
                stdin_stopped: false,
            }
        );
    }

    #[test]
    fn eof_before_writer_failure_still_returns_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let config = EngineConfig::for_root(dir.path());
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(StdioEvent::Message {
                framed: false,
                text: r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_string(),
            })
            .unwrap();
        event_tx.send(StdioEvent::Eof).unwrap();

        let outcome = run_stdio_core(config, event_tx, event_rx, FailingWriter);

        assert_eq!(
            outcome,
            StdioLoopOutcome {
                exit_code: 1,
                stdin_stopped: true,
            }
        );
    }

    #[test]
    fn eof_marks_stdin_reader_safe_to_join() {
        let dir = tempfile::tempdir().unwrap();
        let config = EngineConfig::for_root(dir.path());
        let (event_tx, event_rx) = mpsc::channel();
        event_tx.send(StdioEvent::Eof).unwrap();

        let outcome = run_stdio_core(config, event_tx, event_rx, Vec::<u8>::new());
        assert_eq!(
            outcome,
            StdioLoopOutcome {
                exit_code: 0,
                stdin_stopped: true,
            }
        );
    }

    #[test]
    fn idle_timeout_requires_process_shutdown_instead_of_reader_detach() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = EngineConfig::for_root(dir.path());
        config.mcp_idle_timeout = Some(Duration::from_millis(1));
        let (event_tx, event_rx) = mpsc::channel();

        let outcome = run_stdio_core(config, event_tx, event_rx, Vec::<u8>::new());
        assert_eq!(
            outcome,
            StdioLoopOutcome {
                exit_code: 0,
                stdin_stopped: false,
            }
        );
    }
}
