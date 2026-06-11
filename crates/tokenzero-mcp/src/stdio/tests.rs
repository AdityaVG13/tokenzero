use super::*;
use proptest::prelude::*;
use std::io::{BufReader, Cursor, ErrorKind};

#[test]
fn framed_stdio_parser_reads_content_length_message() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    let mut reader = BufReader::new(input.as_bytes());

    let parsed = read_framed_jsonrpc(&mut reader).unwrap().unwrap();

    assert_eq!(parsed, body);
}

#[test]
fn stdio_event_reader_detects_framed_message_from_partial_prefix() {
    let body = r#"{"jsonrpc":"2.0","id":"partial-prefix","method":"ping"}"#;
    let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    let mut reader = BufReader::with_capacity(1, Cursor::new(input.into_bytes()));
    let (tx, rx) = mpsc::channel();

    read_stdio_events_from_reader(&mut reader, tx);

    match rx.recv().unwrap() {
        StdioEvent::Message { framed, text } => {
            assert!(framed);
            assert_eq!(text, body);
        }
        other => panic!("expected framed message, got {other:?}"),
    }
    assert!(matches!(rx.recv().unwrap(), StdioEvent::Eof));
    assert!(rx.try_recv().is_err());
}

#[test]
fn stdio_event_reader_detects_framed_message_with_content_type_header() {
    let body = r#"{"jsonrpc":"2.0","id":"content-type","method":"ping"}"#;
    let input = format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut reader = BufReader::new(Cursor::new(input.into_bytes()));
    let (tx, rx) = mpsc::channel();

    read_stdio_events_from_reader(&mut reader, tx);

    match rx.recv().unwrap() {
        StdioEvent::Message { framed, text } => {
            assert!(framed);
            assert_eq!(text, body);
        }
        other => panic!("expected framed message, got {other:?}"),
    }
    assert!(matches!(rx.recv().unwrap(), StdioEvent::Eof));
    assert!(rx.try_recv().is_err());
}

#[test]
fn framed_stdio_parser_rejects_oversized_content_length_before_body() {
    let oversized = MAX_MCP_STDIO_FRAME_BYTES + 1;
    let input = format!("Content-Length: {oversized}\r\n\r\n");
    let mut reader = BufReader::new(input.as_bytes());

    let err = read_framed_jsonrpc(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(err.to_string().contains("exceeds maximum"));
}

#[test]
fn framed_stdio_parser_rejects_duplicate_content_length() {
    let body = "{}";
    let input = format!(
        "Content-Length: 1\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut reader = BufReader::new(input.as_bytes());

    let err = read_framed_jsonrpc(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(err.to_string().contains("duplicate Content-Length"));
}

#[test]
fn framed_stdio_parser_rejects_eof_before_header_terminator() {
    let mut reader = BufReader::new("Content-Length: 2\r\n".as_bytes());

    let err = read_framed_jsonrpc(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    assert!(err.to_string().contains("header terminator"));
}

#[test]
fn framed_stdio_parser_rejects_eof_in_header_line() {
    let mut reader = BufReader::new("Content-Length: 2".as_bytes());

    let err = read_framed_jsonrpc(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    assert!(err.to_string().contains("before newline"));
}

#[test]
fn framed_stdio_parser_rejects_oversized_header_line_before_body() {
    let input = format!(
        "Content-Length: 2\r\nX-Long: {}\r\n\r\n{}",
        "a".repeat(MAX_MCP_STDIO_HEADER_LINE_BYTES),
        "{}"
    );
    let mut reader = BufReader::new(input.as_bytes());

    let err = read_framed_jsonrpc(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(err.to_string().contains("header line exceeds maximum"));
}

#[test]
fn framed_stdio_parser_rejects_oversized_header_section_before_body() {
    let header = "X-Trace: ok\r\n";
    let repeat_count = (MAX_MCP_STDIO_HEADER_SECTION_BYTES / header.len()) + 1;
    let input = format!(
        "Content-Length: 2\r\n{}\r\n{}",
        header.repeat(repeat_count),
        "{}"
    );
    let mut reader = BufReader::new(input.as_bytes());

    let err = read_framed_jsonrpc(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(err.to_string().contains("header section exceeds maximum"));
}

#[test]
fn unframed_stdio_parser_reads_newline_delimited_message() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let input = format!("{body}\n");
    let mut reader = BufReader::new(input.as_bytes());

    let parsed = read_unframed_jsonrpc_line(&mut reader).unwrap().unwrap();

    assert_eq!(parsed, input);
}

#[test]
fn unframed_stdio_parser_reads_eof_terminated_message() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let mut reader = BufReader::new(body.as_bytes());

    let parsed = read_unframed_jsonrpc_line(&mut reader).unwrap().unwrap();

    assert_eq!(parsed, body);
}

#[test]
fn unframed_stdio_parser_allows_maximum_line() {
    let input = format!("{}\n", " ".repeat(MAX_MCP_STDIO_LINE_BYTES - 1));
    let mut reader = BufReader::new(input.as_bytes());

    let parsed = read_unframed_jsonrpc_line(&mut reader).unwrap().unwrap();

    assert_eq!(parsed.len(), MAX_MCP_STDIO_LINE_BYTES);
}

#[test]
fn unframed_stdio_parser_rejects_oversized_line() {
    let input = "x".repeat(MAX_MCP_STDIO_LINE_BYTES + 1);
    let mut reader = BufReader::new(input.as_bytes());

    let err = read_unframed_jsonrpc_line(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(err.to_string().contains("unframed line exceeds maximum"));
}

#[test]
fn unframed_stdio_parser_rejects_invalid_utf8() {
    let mut reader = BufReader::new(Cursor::new([0xff, b'\n']));

    let err = read_unframed_jsonrpc_line(&mut reader).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(err.to_string().contains("invalid UTF-8 line"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn framed_stdio_parser_is_total_for_small_generated_bodies(
        body in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let mut input = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        input.extend_from_slice(&body);
        let expected = String::from_utf8(body);
        let mut reader = BufReader::new(Cursor::new(input));

        let result = read_framed_jsonrpc(&mut reader);

        match expected {
            Ok(expected) => prop_assert_eq!(result.unwrap().unwrap(), expected),
            Err(_) => prop_assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData),
        }
    }
}

#[test]
fn framed_stdio_writer_emits_content_length() {
    let response = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let mut output = Vec::new();

    write_framed_jsonrpc(&mut output, response).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.starts_with(&format!("Content-Length: {}\r\n\r\n", response.len())));
    assert!(output.ends_with(response));
}

#[test]
fn stdio_event_reader_recovers_after_oversized_unframed_line() {
    let ping = r#"{"jsonrpc":"2.0","id":"after-bad-line","method":"ping"}"#;
    let input = format!("{}\n{ping}\n", "x".repeat(MAX_MCP_STDIO_LINE_BYTES + 1));
    let mut reader = BufReader::new(Cursor::new(input.into_bytes()));
    let (tx, rx) = mpsc::channel();

    read_stdio_events_from_reader(&mut reader, tx);

    match rx.recv().unwrap() {
        StdioEvent::ParseError { recoverable, .. } => assert!(recoverable),
        other => panic!("expected recoverable parse error, got {other:?}"),
    }
    match rx.recv().unwrap() {
        StdioEvent::Message { framed, text } => {
            assert!(!framed);
            assert_eq!(text.trim_end(), ping);
        }
        other => panic!("expected message after bad line, got {other:?}"),
    }
    assert!(matches!(rx.recv().unwrap(), StdioEvent::Eof));
}

use std::sync::{Arc as TestArc, Mutex as TestMutex};

#[derive(Clone)]
struct SharedOutput(TestArc<TestMutex<Vec<u8>>>);

impl Write for SharedOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn test_engine_config() -> EngineConfig {
    let root = std::env::temp_dir();
    let mut config = EngineConfig::for_root(&root);
    config.mcp_idle_timeout = None;
    config
}

fn run_core_session(messages: &[&str]) -> String {
    let (event_tx, event_rx) = mpsc::channel();
    for message in messages {
        event_tx
            .send(StdioEvent::Message {
                framed: false,
                text: (*message).to_string(),
            })
            .unwrap();
    }
    event_tx.send(StdioEvent::Eof).unwrap();
    drop(event_tx);
    let output = SharedOutput(TestArc::new(TestMutex::new(Vec::new())));
    let exit_code = run_stdio_core(test_engine_config(), event_rx, output.clone());
    assert_eq!(exit_code, 0);
    let written = output.0.lock().unwrap().clone();
    String::from_utf8(written).unwrap()
}

#[test]
fn server_loop_survives_invalid_json_and_keeps_serving() {
    let output = run_core_session(&[
        "this is not json",
        r#"{"jsonrpc":"2.0","id":"alive","method":"ping"}"#,
    ]);

    assert!(output.contains("-32700"), "parse error answered: {output}");
    assert!(
        output.contains("\"id\":\"alive\""),
        "server keeps serving after invalid JSON: {output}"
    );
}

#[test]
fn server_loop_isolates_tool_panics_and_keeps_serving() {
    let output = run_core_session(&[
        r#"{"jsonrpc":"2.0","id":"boom","method":"tokenzero/internal/test-panic"}"#,
        r#"{"jsonrpc":"2.0","id":"alive","method":"ping"}"#,
    ]);

    assert!(
        output.contains("-32603") && output.contains("\"id\":\"boom\""),
        "panic becomes an internal error response: {output}"
    );
    assert!(
        output.contains("test-induced tool panic"),
        "panic reason surfaced for diagnosis: {output}"
    );
    assert!(
        output.contains("\"id\":\"alive\""),
        "server keeps serving after a tool panic: {output}"
    );
}

#[test]
fn server_loop_routes_tools_call_through_worker_pool() {
    let output = run_core_session(&[
        r#"{"jsonrpc":"2.0","id":"pooled","method":"tools/call","params":{"name":"definitely-not-a-tool","arguments":{}}}"#,
    ]);

    assert!(
        output.contains("\"id\":\"pooled\""),
        "tools/call dispatched via workers still answers: {output}"
    );
}

#[test]
fn server_loop_recoverable_parse_error_does_not_end_session() {
    let (event_tx, event_rx) = mpsc::channel();
    event_tx
        .send(StdioEvent::ParseError {
            framed: false,
            error: "unframed line exceeds maximum".to_string(),
            recoverable: true,
        })
        .unwrap();
    event_tx
        .send(StdioEvent::Message {
            framed: false,
            text: r#"{"jsonrpc":"2.0","id":"alive","method":"ping"}"#.to_string(),
        })
        .unwrap();
    event_tx.send(StdioEvent::Eof).unwrap();
    drop(event_tx);
    let output = SharedOutput(TestArc::new(TestMutex::new(Vec::new())));

    let exit_code = run_stdio_core(test_engine_config(), event_rx, output.clone());

    assert_eq!(exit_code, 0);
    let written = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    assert!(written.contains("-32700"), "{written}");
    assert!(written.contains("\"id\":\"alive\""), "{written}");
}
