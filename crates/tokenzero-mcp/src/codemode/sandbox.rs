//! Sandboxed JavaScript plan admission and lowering.
//!
//! V1 intentionally exposes a constrained JavaScript subset: no ambient host
//! objects, no imports, no timers, no network, no process/env, no raw FS, no
//! direct store/DB access, and no mutating bindings until transaction rollback
//! is implemented. Accepted code is lowered onto the native CodeMode dispatcher so execution
//! remains direct-to-TokenZeroEngine, never MCP self-calls.

use super::store::CodeModeLimits;

const DENIED_TOKENS: &[(&str, &str)] = &[
    ("fetch", "network/fetch denied"),
    ("XMLHttpRequest", "network denied"),
    ("WebSocket", "network denied"),
    ("process", "process/env denied"),
    ("Deno", "process/env denied"),
    ("Bun", "process/env denied"),
    ("require", "native module loading denied"),
    ("import(", "native module loading denied"),
    ("import ", "native module loading denied"),
    ("fs.", "raw host FS denied"),
    ("store.", "direct store denied"),
    ("db.", "direct DB denied"),
    ("indexedDB", "direct DB denied"),
    ("node:", "native module loading denied"),
    ("child_process", "process/spawn denied"),
    ("spawn", "process/spawn denied"),
    ("exec(", "process/spawn denied"),
    ("setTimeout", "unbounded timer denied"),
    ("setInterval", "unbounded timer denied"),
    ("while (true", "unbounded loop denied"),
    ("while(true", "unbounded loop denied"),
    ("for (;;", "unbounded loop denied"),
    ("globalThis", "ambient global denied"),
    ("Function(", "dynamic code loading denied"),
    ("eval(", "dynamic code loading denied"),
];

pub(crate) fn lower_code_plan(plan: &str, limits: &CodeModeLimits) -> Result<String, String> {
    let is_function_plan = plan.trim_start().starts_with("export default")
        || plan.trim_start().starts_with("async function")
        || plan.trim_start().starts_with("function");
    if is_function_plan && plan.len() > limits.max_code_bytes {
        return Err(format!(
            "sandbox: code exceeds max_code_bytes {}",
            limits.max_code_bytes
        ));
    }
    let scanned = mask_string_literals(plan);
    for (token, reason) in DENIED_TOKENS {
        if scanned.contains(token) {
            return Err(format!("sandbox: {reason}: {token}"));
        }
    }

    let mut code = plan.trim().to_string();
    if is_function_plan {
        code = extract_function_body(&code)?;
    }
    code = code
        .replace(" token.compactMany", " zero.token.compactMany")
        .replace(" token.expandMany", " zero.token.expandMany")
        .replace(" token.compact", " zero.token.compact")
        .replace(" token.expand", " zero.token.expand")
        .replace("=token.compactMany", "=zero.token.compactMany")
        .replace("=token.expandMany", "=zero.token.expandMany")
        .replace("=token.compact", "=zero.token.compact")
        .replace("=token.expand", "=zero.token.expand")
        .replace("ctx.ref", "zero.token.compact")
        .replace("ctx.step", "zero.step")
        .replace("api.", "zero.")
        .replace("zero.zero.", "zero.");
    Ok(code)
}

fn mask_string_literals(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut in_str = false;
    let mut quote = '\0';
    let mut escaped = false;
    for ch in code.chars() {
        if in_str {
            if escaped {
                escaped = false;
                out.push(' ');
                continue;
            }
            if ch == '\\' {
                escaped = true;
                out.push(' ');
                continue;
            }
            if ch == quote {
                in_str = false;
                out.push(ch);
            } else {
                out.push(' ');
            }
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            in_str = true;
            quote = ch;
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    out
}

fn extract_function_body(code: &str) -> Result<String, String> {
    let start = code
        .find(") {")
        .map(|pos| pos + 2)
        .or_else(|| code.find("){ ").map(|pos| pos + 1))
        .or_else(|| code.find("){ ").map(|pos| pos + 1))
        .or_else(|| code.find("){ ").map(|pos| pos + 1))
        .or_else(|| code.find("){\n").map(|pos| pos + 1))
        .or_else(|| code.find("){").map(|pos| pos + 1))
        .ok_or_else(|| "sandbox: function body missing '{'".to_string())?;
    let end = code
        .rfind('}')
        .ok_or_else(|| "sandbox: function body missing '}'".to_string())?;
    if end <= start {
        return Err("sandbox: malformed function body".to_string());
    }
    Ok(code[start + 1..end].trim().to_string())
}
