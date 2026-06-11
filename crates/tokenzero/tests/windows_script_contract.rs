use std::fs;
use std::path::Path;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under crates/tokenzero")
}

fn extract_function_body<'a>(text: &'a str, name: &str) -> &'a str {
    let marker = format!("function {name} {{");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing PowerShell function {name}"));
    let body = &text[start..];
    let end = body
        .find("\n}\n")
        .unwrap_or_else(|| panic!("missing end of PowerShell function {name}"));
    &body[..end]
}

#[test]
fn windows_mcp_initialize_writes_bom_free_utf8_to_stdin() {
    for script in [
        "scripts/rust_windows_global_rehearsal.ps1",
        "scripts/rust_windows_migrate.ps1",
    ] {
        let path = repo_root().join(script);
        let text = fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", path.display());
        });
        let function = extract_function_body(&text, "Invoke-McpInitialize");

        assert!(
            function.contains("[System.Text.UTF8Encoding]::new($false).GetBytes"),
            "{script} must encode MCP initialize as BOM-free UTF-8 bytes"
        );
        assert!(
            function.contains("$proc.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)"),
            "{script} must write raw bytes to stdin BaseStream"
        );
        assert!(
            function.contains("$proc.StandardInput.BaseStream.Flush()"),
            "{script} must flush raw MCP initialize bytes before closing stdin"
        );
        assert!(
            !function.contains("$proc.StandardInput.Write("),
            "{script} must not use StreamWriter.Write for MCP initialize; Windows PowerShell 5.1 can prepend encoding bytes"
        );
    }
}
