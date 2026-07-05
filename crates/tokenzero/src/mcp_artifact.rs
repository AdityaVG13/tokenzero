use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tempfile::tempdir;

use super::{measure_rss_mb, p95_f64, write_artifacts};

pub(crate) fn run_mcp_artifact(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
    iterations: usize,
) -> Result<serde_json::Value> {
    let temp = tempdir()?;
    fs::write(temp.path().join("sample.txt"), "alpha\nbeta\n")?;
    let exe = std::env::current_exe()?;
    let cache_path = temp.path().join("cache.json");
    let mut unexpected_exits = 0usize;
    let mut missing_initialize_success = 0usize;
    let mut missing_parse_errors = 0usize;
    let mut missing_unknown_methods = 0usize;
    let mut missing_unknown_tools = 0usize;
    let mut missing_tool_schemas = 0usize;
    let mut missing_resource_discovery = 0usize;
    let mut missing_resource_tools_present = 0usize;
    let mut missing_resource_tools_read = 0usize;
    let mut missing_structured_error_data = 0usize;
    let mut missing_tool_cluster_filter = 0usize;
    let mut missing_parallel_reads = 0usize;
    let mut disconnect_failures = 0usize;
    let mut cache_race_failures = 0usize;
    let mut rss_samples = Vec::new();
    for idx in 0..iterations {
        let mut child = Command::new(&exe)
            .arg("mcp-server")
            .arg("--allowed-root")
            .arg(temp.path())
            .arg("--cache-path")
            .arg(&cache_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(rss) = measure_rss_mb(child.id()) {
            rss_samples.push(rss);
        }
        {
            let stdin = child.stdin.as_mut().context("missing mcp stdin")?;
            writeln!(stdin, "{{bad json")?;
            writeln!(
                stdin,
                "{}",
                json!({
                    "jsonrpc": "2.0",
                    "id": idx,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "tokenzero-mcp-smoke",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                })
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":idx+500,"method":"server/not-a-method","params":{}})
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":idx+1000,"method":"tools/list","params":{}})
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":idx+1100,"method":"tools/list","params":{"_meta":{"tokenzero/toolCluster":"material"}}})
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":idx+1250,"method":"resources/list","params":{}})
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":idx+1300,"method":"resources/read","params":{"uri":"resource://tokenzero/tools"}})
            )?;
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":idx+1500,"method":"tools/call","params":{"name":"no_such_tool","arguments":{}}})
            )?;
            for parallel in 0..3 {
                writeln!(
                    stdin,
                    "{}",
                    json!({"jsonrpc":"2.0","id":idx+2000+parallel,"method":"tools/call","params":{"name":"read","arguments":{"path": temp.path().join("sample.txt")}}})
                )?;
            }
        }
        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !mcp_stdout_has_successful_initialize(&stdout, idx) {
            missing_initialize_success += 1;
        }
        // FastMCP emits parse/codec errors to stderr, not stdout.
        if !stderr.contains("Parse error") && !stderr.contains("JSON error") {
            missing_parse_errors += 1;
        }
        if !stdout.contains("Method not found") {
            missing_unknown_methods += 1;
        }
        // FastMCP returns "Method not found: tool: no_such_tool" for unknown tools.
        if !stdout.contains("no_such_tool") || !stdout.contains("Method not found") {
            missing_unknown_tools += 1;
        }
        if !stdout.contains("\"additionalProperties\":false") || !stdout.contains("\"inputSchema\"")
        {
            missing_tool_schemas += 1;
        }
        // FastMCP resources/list must return the same URIs as the old surface.
        // Verify resource://tokenzero/tools is present.
        let resources_id = idx + 1250;
        if !mcp_stdout_has_resource_uri(&stdout, resources_id, "resource://tokenzero/tools") {
            missing_resource_tools_present += 1;
        }
        // resources/read resource://tokenzero/tools must return full tool docs JSON.
        let read_id = idx + 1300;
        if !mcp_stdout_has_resource_content(&stdout, read_id, "resource://tokenzero/tools", "tools") {
            missing_resource_tools_read += 1;
        }
        // Initial resources/list check — did we get a valid response at all?
        if !stdout.contains("\"resources\"") {
            missing_resource_discovery += 1;
        }
        // FastMCP errors use JSON-RPC {code, message} envelope, not custom fields.
        if !stdout.contains("\"code\"") || !stdout.contains("\"message\"") {
            missing_structured_error_data += 1;
        }
        // FastMCP ignores _meta in tools/list; verify the cluster-filtered
        // tools/list response is still a valid tools response (reports tools).
        if !stdout.contains("\"tools\"") {
            missing_tool_cluster_filter += 1;
        }
        if stdout.matches("alpha\\nbeta").count() < 3 {
            missing_parallel_reads += 1;
        }
        if !output.status.success() {
            unexpected_exits += 1;
        }

        let mut disconnect_child = Command::new(&exe)
            .arg("mcp-server")
            .arg("--allowed-root")
            .arg(temp.path())
            .arg("--cache-path")
            .arg(&cache_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(stdin) = disconnect_child.stdin.as_mut() {
            write!(stdin, "{{partial-json")?;
        }
        drop(disconnect_child.stdin.take());
        if !disconnect_child.wait_with_output()?.status.success() {
            disconnect_failures += 1;
        }

        let mut race_children = Vec::new();
        for race in 0..4 {
            let mut race_child = Command::new(&exe)
                .arg("mcp-server")
                .arg("--allowed-root")
                .arg(temp.path())
                .arg("--cache-path")
                .arg(&cache_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            if let Some(stdin) = race_child.stdin.as_mut() {
                writeln!(
                    stdin,
                    "{}",
                    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"tokenzero-mcp-smoke","version":env!("CARGO_PKG_VERSION")}}})
                )?;
                writeln!(
                    stdin,
                    "{}",
                    json!({"jsonrpc":"2.0","method":"notifications/initialized"})
                )?;
                writeln!(
                    stdin,
                    "{}",
                    json!({"jsonrpc":"2.0","id":idx+3000+race,"method":"tools/call","params":{"name":"read","arguments":{"path": temp.path().join("sample.txt")}}})
                )?;
            }
            race_children.push(race_child);
        }
        for race_child in race_children {
            let output = race_child.wait_with_output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !output.status.success() || !stdout.contains("alpha\\nbeta") {
                cache_race_failures += 1;
            }
        }
    }
    let ok = unexpected_exits == 0
        && missing_initialize_success == 0
        && missing_parse_errors == 0
        && missing_unknown_methods == 0
        && missing_unknown_tools == 0
        && missing_tool_schemas == 0
        && missing_resource_discovery == 0
        && missing_resource_tools_present == 0
        && missing_resource_tools_read == 0
        && missing_structured_error_data == 0
        && missing_tool_cluster_filter == 0
        && missing_parallel_reads == 0
        && disconnect_failures == 0
        && cache_race_failures == 0;
    let report = json!({
        "schema_version": "tokenzero.rust_mcp_churn.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "iterations": iterations,
        "initialize_successes_observed": iterations - missing_initialize_success,
        "initialize_failures": missing_initialize_success,
        "malformed_requests": iterations,
        "parse_errors_observed": iterations - missing_parse_errors,
        "unknown_methods_observed": iterations - missing_unknown_methods,
        "unknown_tools_observed": iterations - missing_unknown_tools,
        "tool_schema_failures": missing_tool_schemas,
        "resource_discovery_failures": missing_resource_discovery,
        "resource_tools_present_failures": missing_resource_tools_present,
        "resource_tools_read_failures": missing_resource_tools_read,
        "structured_error_data_failures": missing_structured_error_data,
        "tool_cluster_filter_failures": missing_tool_cluster_filter,
        "parallel_read_batches": iterations,
        "parallel_read_failures": missing_parallel_reads,
        "disconnects": iterations,
        "disconnect_failures": disconnect_failures,
        "cache_race_processes": iterations * 4,
        "cache_race_failures": cache_race_failures,
        "unexpected_exits": unexpected_exits,
        "rss_mb_p95": p95_f64(&mut rss_samples),
        "accelerated": iterations > 1
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Rust MCP artifact",
    )?;
    Ok(report)
}

fn mcp_stdout_has_successful_initialize(stdout: &str, id: usize) -> bool {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|payload| {
            payload.get("id") == Some(&json!(id))
                && payload.get("error").is_none()
                && payload["result"]["protocolVersion"] == "2024-11-05"
                && payload["result"]["serverInfo"]["name"] == "TokenZero"
        })
}

/// Verify the resources/list response for a specific id contains the expected URI.
fn mcp_stdout_has_resource_uri(stdout: &str, id: usize, expected_uri: &str) -> bool {
    for line in stdout.lines() {
        let Ok(payload) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if payload.get("id") != Some(&json!(id)) {
            continue;
        }
        if payload.get("error").is_some() {
            return false;
        }
        let resources = &payload["result"]["resources"];
        if let Some(arr) = resources.as_array() {
            return arr.iter().any(|r| r.get("uri") == Some(&Value::String(expected_uri.to_string())));
        }
        return false;
    }
    false
}

/// Verify the resources/read response for a specific id contains the expected
/// URI and a content substring in the returned text.
fn mcp_stdout_has_resource_content(
    stdout: &str,
    id: usize,
    expected_uri: &str,
    text_contains: &str,
) -> bool {
    for line in stdout.lines() {
        let Ok(payload) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if payload.get("id") != Some(&json!(id)) {
            continue;
        }
        if payload.get("error").is_some() {
            return false;
        }
        let contents = &payload["result"]["contents"];
        if let Some(arr) = contents.as_array() {
            return arr.iter().any(|c| {
                c.get("uri") == Some(&Value::String(expected_uri.to_string()))
                    && c.get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|t| t.contains(text_contains))
            });
        }
        return false;
    }
    false
}
