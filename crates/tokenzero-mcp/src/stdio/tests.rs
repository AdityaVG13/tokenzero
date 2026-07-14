use super::*;
use proptest::prelude::*;
use std::io::{BufReader, Cursor, ErrorKind};

fn framed(body: &str, extra_headers: &str) -> String {
    format!("{extra_headers}Content-Length: {}\r\n\r\n{body}", body.len())
}

#[test]
fn framed_parser_and_event_reader_accept_supported_headers() {
    let base = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    assert_eq!(
        read_framed_jsonrpc(&mut BufReader::new(framed(base, "").as_bytes())).unwrap(),
        Some(base.to_string())
    );

    for (capacity, body, headers) in [
        (1, r#"{"jsonrpc":"2.0","id":"partial-prefix","method":"ping"}"#, ""),
        (64, r#"{"jsonrpc":"2.0","id":"content-type","method":"ping"}"#, "Content-Type: application/json\r\n"),
    ] {
        let input = framed(body, headers);
        let mut reader = BufReader::with_capacity(capacity, Cursor::new(input.into_bytes()));
        let (tx, rx) = mpsc::channel();
        read_stdio_events_from_reader(&mut reader, tx);
        match rx.recv().unwrap() {
            StdioEvent::Message { framed: true, text } => assert_eq!(text, body),
            other => panic!("expected framed message, got {other:?}"),
        }
        assert!(matches!(rx.recv().unwrap(), StdioEvent::Eof));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn framed_parser_error_matrix() {
    let oversized_header = format!(
        "Content-Length: 2\r\nX-Long: {}\r\n\r\n{{}}",
        "a".repeat(MAX_MCP_STDIO_HEADER_LINE_BYTES)
    );
    let section_line = "X-Trace: ok\r\n";
    let oversized_section = format!(
        "Content-Length: 2\r\n{}\r\n{{}}",
        section_line.repeat((MAX_MCP_STDIO_HEADER_SECTION_BYTES / section_line.len()) + 1)
    );
    for (input, kind, phrase) in [
        (format!("Content-Length: {}\r\n\r\n", MAX_MCP_STDIO_FRAME_BYTES + 1), ErrorKind::InvalidData, "exceeds maximum"),
        ("Content-Length: 1\r\nContent-Length: 2\r\n\r\n{}".to_string(), ErrorKind::InvalidData, "duplicate Content-Length"),
        ("Content-Length: 2\r\n".to_string(), ErrorKind::UnexpectedEof, "header terminator"),
        ("Content-Length: 2".to_string(), ErrorKind::UnexpectedEof, "before newline"),
        (oversized_header, ErrorKind::InvalidData, "header line exceeds maximum"),
        (oversized_section, ErrorKind::InvalidData, "header section exceeds maximum"),
    ] {
        let err = read_framed_jsonrpc(&mut BufReader::new(input.as_bytes())).unwrap_err();
        assert_eq!(err.kind(), kind, "{phrase}");
        assert!(err.to_string().contains(phrase), "{err}");
    }
}

#[test]
fn unframed_parser_boundary_matrix() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let newline = format!("{body}\n");
    let maximum = format!("{}\n", " ".repeat(MAX_MCP_STDIO_LINE_BYTES - 1));
    for (input, expected) in [
        (newline.clone(), newline),
        (body.to_string(), body.to_string()),
        (maximum.clone(), maximum),
    ] {
        let parsed = read_unframed_jsonrpc_line(&mut BufReader::new(input.as_bytes())).unwrap();
        assert_eq!(parsed.as_deref(), Some(expected.as_str()));
    }

    for (input, phrase) in [
        ("x".repeat(MAX_MCP_STDIO_LINE_BYTES + 1).into_bytes(), "unframed line exceeds maximum"),
        (vec![0xff, b'\n'], "invalid UTF-8 line"),
    ] {
        let err = read_unframed_jsonrpc_line(&mut BufReader::new(Cursor::new(input))).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert!(err.to_string().contains(phrase), "{err}");
    }
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
        let result = read_framed_jsonrpc(&mut BufReader::new(Cursor::new(input)));
        match expected {
            Ok(expected) => prop_assert_eq!(result.unwrap().unwrap(), expected),
            Err(_) => prop_assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData),
        }
    }
}

#[test]
fn framed_writer_preserves_wire_format() {
    let response = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let mut output = Vec::new();
    write_framed_jsonrpc(&mut output, response).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.starts_with(&format!("Content-Length: {}\r\n\r\n", response.len())));
    assert!(output.ends_with(response));
}

#[test]
fn event_reader_recovers_after_oversized_unframed_line() {
    let ping = r#"{"jsonrpc":"2.0","id":"after-bad-line","method":"ping"}"#;
    let input = format!("{}\n{ping}\n", "x".repeat(MAX_MCP_STDIO_LINE_BYTES + 1));
    let (tx, rx) = mpsc::channel();
    read_stdio_events_from_reader(&mut BufReader::new(Cursor::new(input.into_bytes())), tx);
    assert!(matches!(rx.recv().unwrap(), StdioEvent::ParseError { recoverable: true, .. }));
    match rx.recv().unwrap() {
        StdioEvent::Message { framed: false, text } => assert_eq!(text.trim_end(), ping),
        other => panic!("expected message after bad line, got {other:?}"),
    }
    assert!(matches!(rx.recv().unwrap(), StdioEvent::Eof));
}

fn test_engine_config() -> EngineConfig {
    let mut config = EngineConfig::for_root(&std::env::temp_dir());
    config.mcp_idle_timeout = None;
    config
}

fn run_core_session(messages: &[&str]) -> String {
    let (event_tx, event_rx) = mpsc::channel();
    for message in messages {
        event_tx.send(StdioEvent::Message { framed: false, text: (*message).to_string() }).unwrap();
    }
    event_tx.send(StdioEvent::Eof).unwrap();
    drop(event_tx);
    let output = TestOutput::default();
    assert_eq!(run_stdio_core(test_engine_config(), event_rx, output.clone()), 0);
    let written = output.bytes();
    String::from_utf8(written).unwrap()
}

#[test]
fn server_loop_error_and_worker_matrix_keeps_serving() {
    for (messages, expected) in [
        (
            vec!["this is not json", r#"{"jsonrpc":"2.0","id":"alive","method":"ping"}"#],
            vec!["-32700", "\"id\":\"alive\""],
        ),
        (
            vec![r#"{"jsonrpc":"2.0","id":"boom","method":"tokenzero/internal/test-panic"}"#, r#"{"jsonrpc":"2.0","id":"alive","method":"ping"}"#],
            vec!["-32603", "\"id\":\"boom\"", "test-induced tool panic", "\"id\":\"alive\""],
        ),
        (
            vec![r#"{"jsonrpc":"2.0","id":"pooled","method":"tools/call","params":{"name":"definitely-not-a-tool","arguments":{}}}"#],
            vec!["\"id\":\"pooled\""],
        ),
    ] {
        let output = run_core_session(&messages);
        for needle in expected {
            assert!(output.contains(needle), "missing {needle}: {output}");
        }
    }
}

#[test]
fn recoverable_parse_error_does_not_end_session() {
    let (event_tx, event_rx) = mpsc::channel();
    event_tx.send(StdioEvent::ParseError {
        framed: false,
        error: "unframed line exceeds maximum".to_string(),
        recoverable: true,
    }).unwrap();
    event_tx.send(StdioEvent::Message {
        framed: false,
        text: r#"{"jsonrpc":"2.0","id":"alive","method":"ping"}"#.to_string(),
    }).unwrap();
    event_tx.send(StdioEvent::Eof).unwrap();
    drop(event_tx);
    let output = TestOutput::default();
    assert_eq!(run_stdio_core(test_engine_config(), event_rx, output.clone()), 0);
    let written = String::from_utf8(output.bytes()).unwrap();
    assert!(written.contains("-32700"), "{written}");
    assert!(written.contains("\"id\":\"alive\""), "{written}");
}
