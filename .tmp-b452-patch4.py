"""b452 phase 4: denial job carries the sandbox error kind."""

P = "crates/tokenzero-mcp/src/codemode/exec.rs"
s = open(P).read()

old = '''            result: Mutex::new(Some(tz_error_json(&message, "mutating binding denied"))),'''
new = '''            result: Mutex::new(Some(
                serde_json::to_string(&json!({
                    "__tz_error": message,
                    "__tz_error_kind": "sandbox",
                }))
                .unwrap_or_else(|_| {
                    r#"{"__tz_error":"mutating binding denied","__tz_error_kind":"sandbox"}"#
                        .to_string()
                }),
            )),'''
assert s.count(old) == 1
s = s.replace(old, new)
open(P, "w").write(s)
print("phase4 ok")
