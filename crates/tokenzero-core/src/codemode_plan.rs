//! Lexical validation of CodeMode plan method references.
//!
//! TokenZero owns the CodeMode plan contract. Plans arrive as raw text
//! (JavaScript / recipe forms) and reference host methods through the `zero.*`
//! and `codemode.*` connectors. This module scans the plan text for those
//! references and verifies each against the operation ABI registry, so
//! hallucinated or misspelled method names fail closed before a round trip.
//!
//! The scanner is lexical, not a full JavaScript parser: string literals
//! (`'...'`, `"..."`), template-literal text (`` `...` ``), and `//` / `/* */`
//! comments are skipped entirely, so prose that merely *mentions* a method
//! name cannot trigger the ban. ` ${...}` interpolation inside template
//! literals is real code and is still scanned. Real member-call syntax outside
//! strings and comments must resolve to a known operation.
//!
//! Regression: tokenzero-plan-validator-string-false-positive-hwj — the
//! hub-side raw-text scan flagged a nonexistent `zero.*` method name that
//! appeared inside a bead-description string literal, costing a full failed
//! round trip.

use std::fmt;

use crate::operation_abi::resolve_operation;

/// Connectors whose dotted member paths are CodeMode method references.
const CONNECTORS: [&[u8]; 2] = [b"zero", b"codemode"];

/// One rejected method reference found in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMethodIssue {
    /// The dotted method path as written, e.g. `zero.nonexistentMethod`.
    pub method: String,
    /// Byte offset in the plan text where the reference starts.
    pub byte_offset: usize,
}

impl fmt::Display for PlanMethodIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown CodeMode method `{}` at byte {}",
            self.method, self.byte_offset
        )
    }
}

/// Validate every `zero.*` / `codemode.*` method reference in a CodeMode plan.
///
/// String literals, template-literal text, and comments are skipped; template
/// `${...}` interpolation is scanned as real code. The first member reference
/// outside those regions that does not resolve to a known operation fails the
/// plan (fail closed). Bare connector identifiers (`zero` alone) and property
/// access on other objects are not method references.
pub fn validate_plan_methods(plan: &str) -> Result<(), PlanMethodIssue> {
    let bytes = plan.as_bytes();
    let mut i = 0;
    scan_code(bytes, &mut i, None)
}

/// Scan a run of code until EOF or, inside `${...}` interpolation, the
/// matching `}` (which is consumed). `stop == Some(b'}')` tracks brace depth so
/// nested blocks and object literals inside interpolation are handled.
fn scan_code(bytes: &[u8], i: &mut usize, stop: Option<u8>) -> Result<(), PlanMethodIssue> {
    while *i < bytes.len() {
        let b = bytes[*i];
        if Some(b) == stop {
            *i += 1;
            return Ok(());
        }
        match b {
            b'\'' | b'"' => *i = skip_string_literal(bytes, *i),
            b'`' => *i = skip_template_literal(bytes, *i)?,
            b'/' if bytes.get(*i + 1) == Some(&b'/') => *i = skip_line_comment(bytes, *i),
            b'/' if bytes.get(*i + 1) == Some(&b'*') => *i = skip_block_comment(bytes, *i),
            b'{' if stop == Some(b'}') => {
                // Nested brace inside a ${...} interpolation: scan the nested
                // block as code until its own matching `}`.
                *i += 1;
                scan_code(bytes, i, stop)?;
            }
            b if is_ident_start(b) => {
                let (chain, next) = read_member_chain(bytes, *i);
                if CONNECTORS.contains(&chain[0]) {
                    if let Some(issue) = check_chain(&chain, *i) {
                        return Err(issue);
                    }
                }
                *i = next;
            }
            _ => *i += 1,
        }
    }
    Ok(())
}

/// Skip a `'...'` or `"..."` literal starting at `start` (the quote byte).
/// Backslash escapes are honored; an unterminated literal swallows to EOF.
fn skip_string_literal(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Skip a `` `...` `` template literal starting at `start`. Literal text and
/// escapes are skipped; `${...}` interpolation is scanned as real code so
/// method references there still validate.
fn skip_template_literal(bytes: &[u8], start: usize) -> Result<usize, PlanMethodIssue> {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => return Ok(i + 1),
            b'$' if bytes.get(i + 1) == Some(&b'{') => {
                i += 1; // at '{'
                scan_code(bytes, &mut i, Some(b'}'))?;
            }
            _ => i += 1,
        }
    }
    Ok(bytes.len())
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
        i += 1;
    }
    if i + 1 < bytes.len() {
        i + 2
    } else {
        bytes.len()
    }
}

/// Read a maximal `ident (. ident)*` member chain starting at an identifier.
/// Whitespace around dots is allowed (as in JavaScript). Returns the chain
/// segments and the index just past the last consumed identifier.
fn read_member_chain(bytes: &[u8], start: usize) -> (Vec<&[u8]>, usize) {
    let mut segments = Vec::new();
    let mut i = start;
    let ident_start = i;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    segments.push(&bytes[ident_start..i]);
    loop {
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if bytes.get(j) != Some(&b'.') {
            break;
        }
        let mut k = j + 1;
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        if k >= bytes.len() || !is_ident_start(bytes[k]) {
            break;
        }
        let seg_start = k;
        while k < bytes.len() && is_ident_continue(bytes[k]) {
            k += 1;
        }
        segments.push(&bytes[seg_start..k]);
        i = k;
    }
    (segments, i)
}

/// Check a connector-rooted member chain. The longest known prefix wins:
/// trailing segments may be property access on a result (e.g. `zero.read.then`
/// is the `then` property of the `zero.read` result, not a method). Only when
/// no prefix resolves does the reference fail closed.
fn check_chain(chain: &[&[u8]], byte_offset: usize) -> Option<PlanMethodIssue> {
    if chain.len() < 2 {
        // Bare connector identifier (`zero`), not a method reference.
        return None;
    }
    for end in (2..=chain.len()).rev() {
        let candidate = join_dotted(chain, end);
        if resolve_operation(&candidate).is_some() {
            return None;
        }
    }
    Some(PlanMethodIssue {
        method: join_dotted(chain, chain.len()),
        byte_offset,
    })
}

fn join_dotted(chain: &[&[u8]], end: usize) -> String {
    let mut out = String::new();
    for (idx, seg) in chain[..end].iter().enumerate() {
        if idx > 0 {
            out.push('.');
        }
        // Identifier segments are pure ASCII by construction.
        out.push_str(std::str::from_utf8(seg).expect("ASCII identifier segment"));
    }
    out
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: a bead-description string literal contained the text of a
    /// nonexistent method name followed by an open paren; raw-text scanning
    /// read it as a call and rejected the whole plan.
    #[test]
    fn bead_description_string_with_nonexistent_method_passes() {
        let plan = r#"
            const bead = {
                id: "tokenzero-plan-validator-string-false-positive-hwj",
                description: "Fix: strip string literals and comments before method scanning (zero.nonexistentMethod(...)).",
            };
            await zero.read("AGENTS.md");
        "#;
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn string_literal_mentioning_nonexistent_method_is_not_flagged() {
        for plan in [
            r#"const a = 'zero.nope()';"#,
            r#"const b = "zero.nope(\"arg\")";"#,
            r#"const c = `call zero.nope() later`;"#,
            r#"console.log("zero.nope", 'codemode.nope()');"#,
            r#"const note = "also fine: codemode.notARealMethod(...)";"#,
        ] {
            assert_eq!(validate_plan_methods(plan), Ok(()), "plan: {plan}");
        }
    }

    #[test]
    fn line_and_block_comments_are_skipped() {
        let plan = "// zero.nope()\n/* zero.nope() */\nconst x = 1;";
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn real_call_syntax_with_unknown_method_fails_closed() {
        let err = validate_plan_methods(r#"await zero.nonexistentMethod("x");"#)
            .expect_err("unknown method call must fail");
        assert_eq!(err.method, "zero.nonexistentMethod");
        assert_eq!(err.byte_offset, 6);
        assert_eq!(
            err.to_string(),
            "unknown CodeMode method `zero.nonexistentMethod` at byte 6"
        );
    }

    #[test]
    fn unknown_codemode_connector_method_fails_closed() {
        let err = validate_plan_methods("codemode.definitelyNotAThing();")
            .expect_err("unknown codemode method must fail");
        assert_eq!(err.method, "codemode.definitelyNotAThing");
    }

    #[test]
    fn unknown_nested_path_fails_with_full_path() {
        let err = validate_plan_methods("zero.token.nope();").expect_err("unknown path must fail");
        assert_eq!(err.method, "zero.token.nope");
    }

    #[test]
    fn unknown_method_in_template_interpolation_fails_closed() {
        // ${...} interpolation is real code, not string text.
        let err = validate_plan_methods(r#"const msg = `value: ${zero.nope()}`;"#)
            .expect_err("interpolation must be scanned");
        assert_eq!(err.method, "zero.nope");
    }

    #[test]
    fn known_method_calls_and_prose_pass() {
        let plan = r#"
            // Read then search; prose may mention method names in strings.
            const ref = await zero.read("src/main.rs", { max_visible_tokens: 800 });
            const hits = await zero.grep("fn main", "crates/");
            const expanded = await zero.token.expand({ ref: "tz://x" });
            await zero.batch([{ tool: "zero.tree", args: {} }]);
            const alias = await zero.expand(ref);
            await codemode.describe("zero.read");
            const note = "reminder: zero.totallyFakeMethod(...) is not real";
        "#;
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn property_access_on_known_result_is_allowed() {
        // `then` is property access on the zero.read result, not a method.
        let plan = "const thenRef = zero.read.then;";
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn property_access_on_other_objects_is_not_scanned() {
        // The chain root is `obj`, not a connector.
        let plan = "obj.zero.nope();";
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn bare_connector_identifier_is_not_a_method_reference() {
        let plan = "const z = zero; const c = codemode;";
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn whitespace_around_dots_is_tolerated() {
        let plan = "zero . read(\"a\");\nzero.token . expand(ref);";
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }

    #[test]
    fn unterminated_string_swallows_to_end_without_flagging() {
        let plan = r#"const s = "zero.nope();"#;
        assert_eq!(validate_plan_methods(plan), Ok(()));
    }
}
