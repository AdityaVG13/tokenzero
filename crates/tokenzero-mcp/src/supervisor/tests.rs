use super::*;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU32, Ordering};

struct ChannelWriter {
    tx: mpsc::Sender<Vec<u8>>,
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tx
            .send(buf.to_vec())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    position: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.pending.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.pending = chunk;
                    self.position = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let available = &self.pending[self.position..];
        let count = available.len().min(buf.len());
        buf[..count].copy_from_slice(&available[..count]);
        self.position += count;
        Ok(count)
    }
}

fn channel_pipe() -> (ChannelWriter, ChannelReader) {
    let (tx, rx) = mpsc::channel();
    (
        ChannelWriter { tx },
        ChannelReader {
            rx,
            pending: Vec::new(),
            position: 0,
        },
    )
}

#[derive(Clone)]
struct SharedOutput(Arc<StdMutex<Vec<u8>>>);

impl Write for SharedOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Scripted echo server: answers every request with an empty result and
/// dies (drops both pipes) right after receiving a line that contains
/// `die_on`, without responding to it.
fn scripted_child(die_on: Option<String>) -> ChildHandles {
    let (stdin_writer, mut stdin_reader) = channel_pipe();
    let (mut stdout_writer, stdout_reader) = channel_pipe();
    thread::spawn(move || {
        let mut reader = BufReader::new(&mut stdin_reader);
        loop {
            let mut line = String::new();
            let Ok(bytes) = std::io::BufRead::read_line(&mut reader, &mut line) else {
                return;
            };
            if bytes == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            if let Some(marker) = die_on.as_deref() {
                if line.contains(marker) {
                    return;
                }
            }
            let Ok(parsed) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if parsed.get("id").is_none() {
                continue;
            }
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": parsed.get("id").cloned().unwrap_or(Value::Null),
                "result": {}
            });
            if writeln!(stdout_writer, "{response}").is_err() {
                return;
            }
        }
    });
    ChildHandles {
        stdin: Box::new(stdin_writer),
        stdout: Box::new(stdout_reader),
        terminate: Box::new(|| {}),
    }
}

fn from_client(text: &str) -> SupervisorEvent {
    SupervisorEvent::FromClient(StdioEvent::Message {
        framed: false,
        text: text.to_string(),
    })
}

#[test]
fn supervisor_respawns_child_and_replays_initialize_after_crash() {
    let spawn_count = Arc::new(AtomicU32::new(0));
    let spawn_count_for_spawner = Arc::clone(&spawn_count);
    let spawner = move || {
        let count = spawn_count_for_spawner.fetch_add(1, Ordering::SeqCst);
        // First child dies on tools/call; the replacement serves forever.
        Ok(scripted_child(if count == 0 {
            Some("tools/call".to_string())
        } else {
            None
        }))
    };

    let (event_tx, event_rx) = mpsc::channel();
    let output = SharedOutput(Arc::new(StdMutex::new(Vec::new())));
    let output_capture = output.clone();

    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
    let tool_call =
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read","arguments":{}}}"#;
    let ping = r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#;

    let driver_tx = event_tx.clone();
    let supervisor =
        thread::spawn(move || run_supervisor_loop(spawner, event_tx, event_rx, output_capture));

    driver_tx.send(from_client(initialize)).unwrap();
    wait_for_output(&output, "\"id\":1");
    driver_tx.send(from_client(tool_call)).unwrap();
    wait_for_output(&output, "\"id\":2");
    driver_tx.send(from_client(ping)).unwrap();
    wait_for_output(&output, "\"id\":3");
    driver_tx
        .send(SupervisorEvent::FromClient(StdioEvent::Eof))
        .unwrap();

    let exit_code = supervisor.join().unwrap();
    assert_eq!(exit_code, 0);
    assert!(spawn_count.load(Ordering::SeqCst) >= 2, "child respawned");

    let written = output.0.lock().unwrap().clone();
    let written = String::from_utf8(written).unwrap();
    assert_eq!(
        written.matches("\"id\":1").count(),
        1,
        "replayed initialize response must be swallowed: {written}"
    );
    assert!(
        written.contains("\"id\":2") && written.contains("-32603"),
        "in-flight request fails over with a retryable error: {written}"
    );
    assert!(
        written.contains("\"id\":3") && written.contains("result"),
        "requests after the crash are served by the respawned child: {written}"
    );
}

fn wait_for_output(output: &SharedOutput, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        {
            let written = output.0.lock().unwrap();
            if String::from_utf8_lossy(&written).contains(needle) {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {needle} in supervisor output"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn supervisor_gives_up_after_repeated_spawn_failures() {
    let spawner = || Err(std::io::Error::other("spawn always fails in this test"));
    let (event_tx, event_rx) = mpsc::channel();
    let output = SharedOutput(Arc::new(StdMutex::new(Vec::new())));

    let exit_code = run_supervisor_loop(spawner, event_tx, event_rx, output);

    assert_eq!(exit_code, 1);
}
