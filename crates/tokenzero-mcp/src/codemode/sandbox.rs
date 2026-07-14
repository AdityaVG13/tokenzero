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
    let trimmed = plan.trim_start();
    let is_function_plan = ["export default", "function", "function"]
        .iter().any(|prefix| trimmed.starts_with(prefix));
    if is_function_plan && plan.len() > limits.max_code_bytes {
        return Err(format!(
            "sandbox: code exceeds max_code_bytes {}",
            limits.max_code_bytes
        ));
    }
    let scanned = mask_string_literals(plan);
    for (token, reason) in DENIED_TOKENS {
        if contains_token_at_identifier_boundary(&scanned, token) {
            return Err(format!("sandbox: {reason}: {token}"));
        }
    }

    let mut code = plan.trim().to_string();
    if is_function_plan {
        code = extract_function_body(&code)?;
    }
    for (token, replacement) in [
        ("token.", "zero.token."),
        ("ctx.ref", "zero.token.compact"),
        ("ctx.step", "zero.step"),
        ("api.", "zero."),
        ("zero.zero.", "zero."),
    ] {
        code = rewrite_outside_strings(&code, token, replacement);
    }
    Ok(code)
}

/// Rewrite alias tokens onto their `zero.*` bindings without touching string
/// literals (`"/tmp/fab-api.txt"` must survive) or identifier tails
/// (`myapi.` must not become `myzero.`).
fn rewrite_outside_strings(code: &str, token: &str, replacement: &str) -> String {
    let masked = mask_string_literals(code);
    let bytes = masked.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut cursor = 0;
    let mut search_from = 0;
    while let Some(rel) = masked[search_from..].find(token) {
        let start = search_from + rel;
        let boundary_before = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let already_prefixed = replacement.ends_with(token)
            && masked[..start].ends_with(&replacement[..replacement.len() - token.len()]);
        if boundary_before && !already_prefixed {
            out.push_str(&code[cursor..start]);
            out.push_str(replacement);
            cursor = start + token.len();
        }
        search_from = start + token.len().max(1);
    }
    out.push_str(&code[cursor..]);
    out
}

/// Match a denied token only where it starts (and, if it ends with an
/// identifier character, also ends) at an identifier boundary, so `refs.`
/// does not trip the `fs.` guard and `subprocess`/`respawn` do not trip
/// `process`/`spawn`.
fn contains_token_at_identifier_boundary(haystack: &str, token: &str) -> bool {
    let bytes = haystack.as_bytes();
    let check_end = token.as_bytes().last().is_some_and(|byte| is_identifier_byte(*byte));
    haystack.match_indices(token).any(|(start, _)| {
        let end = start + token.len();
        (start == 0 || !is_identifier_byte(bytes[start - 1]))
            && (!check_end || end == bytes.len() || !is_identifier_byte(bytes[end]))
    })
}

fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
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
    let start = [") {", "){ ", "){\n", "){"].iter().find_map(|pattern| {
        code.find(pattern).map(|position| position + pattern.find('{').unwrap())
    }).ok_or_else(|| "sandbox: function body missing '{'".to_string())?;
    let end = code.rfind('}').ok_or_else(|| "sandbox: function body missing '}'".to_string())?;
    if end <= start { return Err("sandbox: malformed function body".to_string()); }
    Ok(code[start + 1..end].trim().to_string())
}
