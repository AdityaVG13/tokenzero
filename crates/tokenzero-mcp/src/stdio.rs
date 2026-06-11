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

pub fn run_stdio(config: EngineConfig) -> i32 {
    let (event_tx, event_rx) = mpsc::channel();
    thread::spawn(move || read_stdio_events(event_tx));
    run_stdio_core(config, event_rx, std::io::stdout())
}

/// Transport-agnostic server loop. Lightweight JSON-RPC methods are answered
/// inline; tools/call requests and batches are dispatched to a worker pool so
/// a long-running tool cannot block keepalive pings. All responses funnel
/// through a single writer thread that owns the output stream.
fn run_stdio_core<W: Write + Send + 'static>(
    config: EngineConfig,
    events: mpsc::Receiver<StdioEvent>,
    writer: W,
) -> i32 {
    let engine = Arc::new(TokenZeroEngine::new(config));
    let idle_timeout = engine.config.mcp_idle_timeout;

    let (response_tx, response_rx) = mpsc::channel::<OutgoingResponse>();
    let writer_thread = thread::spawn(move || write_stdio_responses(writer, response_rx));

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

    loop {
        let event = if let Some(timeout) = idle_timeout {
            match events.recv_timeout(timeout) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match events.recv() {
                Ok(event) => event,
                Err(_) => break,
            }
        };

        match event {
            StdioEvent::Message { framed, text } => {
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        if send_parse_error(&response_tx, framed, err.to_string()).is_err() {
                            break;
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
                        break;
                    }
                } else if let Some(text) = dispatch_jsonrpc(&engine, parsed) {
                    let outgoing = OutgoingResponse { framed, text };
                    if response_tx.send(outgoing).is_err() {
                        break;
                    }
                }
            }
            StdioEvent::ParseError {
                framed,
                error,
                recoverable,
            } => {
                if send_parse_error(&response_tx, framed, error).is_err() {
                    break;
                }
                if !recoverable {
                    break;
                }
            }
            StdioEvent::Eof => break,
        }
    }

    // Orderly shutdown: let in-flight tool calls finish and flush their
    // responses before the process exits.
    drop(work_tx);
    for worker in worker_threads {
        let _ = worker.join();
    }
    drop(response_tx);
    let _ = writer_thread.join();
    0
}

struct WorkItem {
    framed: bool,
    message: Value,
}

struct OutgoingResponse {
    framed: bool,
    text: String,
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
fn write_stdio_responses<W: Write>(mut writer: W, responses: mpsc::Receiver<OutgoingResponse>) {
    while let Ok(response) = responses.recv() {
        let written = if response.framed {
            write_framed_jsonrpc(&mut writer, &response.text)
        } else {
            writeln!(writer, "{}", response.text).and_then(|_| flush_retry(&mut writer))
        };
        if written.is_err() {
            break;
        }
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

        if framed {
            match read_framed_jsonrpc(reader) {
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
                    // A desynced framed stream has no recoverable message
                    // boundary, so framed errors stay fatal.
                    let _ = tx.send(StdioEvent::ParseError {
                        framed,
                        error: err.to_string(),
                        recoverable: false,
                    });
                    break;
                }
            }
        } else {
            let line = match read_unframed_jsonrpc_line(reader) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    let _ = tx.send(StdioEvent::Eof);
                    break;
                }
                Err(err) => {
                    // Unframed parsing leaves the stream at the next line
                    // boundary, so one bad line must not end the session.
                    if tx
                        .send(StdioEvent::ParseError {
                            framed,
                            error: err.to_string(),
                            recoverable: true,
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            if tx.send(StdioEvent::Message { framed, text: line }).is_err() {
                break;
            }
        }
    }
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
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "MCP stdio frame ended before header terminator",
            ));
        }
        if !header_line.ends_with(b"\n") {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "MCP stdio header line ended before newline",
            ));
        }
        header_bytes = header_bytes.checked_add(bytes).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                "MCP stdio header section length overflow",
            )
        })?;
        if header_bytes > MAX_MCP_STDIO_HEADER_SECTION_BYTES {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "MCP stdio header section exceeds maximum {MAX_MCP_STDIO_HEADER_SECTION_BYTES}"
                ),
            ));
        }
        let header = std::str::from_utf8(&header_line)
            .map_err(|err| Error::new(ErrorKind::InvalidData, err.to_string()))?
            .trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("invalid MCP stdio header: {header}"),
            ));
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "duplicate Content-Length header",
                ));
            }
            let parsed = value.trim().parse::<usize>().map_err(|err| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!("invalid Content-Length header: {err}"),
                )
            })?;
            if parsed > MAX_MCP_STDIO_FRAME_BYTES {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("Content-Length {parsed} exceeds maximum {MAX_MCP_STDIO_FRAME_BYTES}"),
                ));
            }
            content_length = Some(parsed);
        }
    }

    let Some(content_length) = content_length else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "missing Content-Length header",
        ));
    };
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|err| Error::new(ErrorKind::InvalidData, format!("invalid UTF-8 body: {err}")))
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
        .map_err(|err| Error::new(ErrorKind::InvalidData, format!("invalid UTF-8 line: {err}")))
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
        let next_len = line.len().checked_add(bytes_to_consume).ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, format!("{label} length overflow"))
        })?;
        if next_len > max_bytes {
            drain_to_line_boundary(reader)?;
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("{label} exceeds maximum {max_bytes}"),
            ));
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
mod tests;
