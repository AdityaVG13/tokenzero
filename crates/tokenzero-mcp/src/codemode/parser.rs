//! JS-like plan parser for CodeMode execution.

use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) enum Statement {
    Binding { name: String, call: MethodCall },
    Call(MethodCall),
    Return(ReturnExpr),
}

#[derive(Debug, Clone)]
pub(crate) struct MethodCall {
    pub(crate) method: String,
    pub(crate) args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    Null,
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    VarRef(String),
    PropAccess(String, String),
}

#[derive(Debug, Clone)]
pub(crate) enum ReturnExpr {
    Var(String),
    PropAccess(String, String),
    Object(Vec<(String, Expr)>),
    Expr(Expr),
}

pub(crate) fn parse_plan(input: &str) -> Result<Vec<Statement>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("empty plan".to_string());
    }

    let raw_stmts = split_statements(input);
    let mut statements = Vec::new();

    for raw in &raw_stmts {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        if let Some(stmt) = parse_statement(s)? {
            statements.push(stmt);
        }
    }

    if statements.is_empty() {
        return Err("no executable statements found in plan".to_string());
    }
    Ok(statements)
}

fn parse_statement(s: &str) -> Result<Option<Statement>, String> {
    let s = strip_await(s);
    let s = s.trim().trim_end_matches(';').trim();
    if s.is_empty() {
        return Ok(None);
    }

    if s.starts_with("return ") || s.starts_with("return{") {
        let expr_str = s.strip_prefix("return").unwrap().trim();
        return Ok(Some(Statement::Return(parse_return_expr(expr_str)?)));
    }

    if let Some(rest) = s
        .strip_prefix("const ")
        .or_else(|| s.strip_prefix("let "))
        .or_else(|| s.strip_prefix("var "))
    {
        let (name, call_str) = split_binding(rest)?;
        let call_str = strip_await(call_str.trim());
        let call = parse_method_call(call_str.trim())?;
        return Ok(Some(Statement::Binding { name, call }));
    }

    let call = parse_method_call(s)?;
    Ok(Some(Statement::Call(call)))
}

fn strip_await(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix("await ").unwrap_or(s)
}

fn split_binding(s: &str) -> Result<(String, &str), String> {
    let eq_pos = s
        .find('=')
        .ok_or_else(|| format!("binding without '=': {s}"))?;
    let name = s[..eq_pos].trim().to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!("invalid variable name: {name}"));
    }
    let rhs = s[eq_pos + 1..].trim();
    let rhs = strip_await(rhs);
    Ok((name, rhs))
}

fn parse_method_call(s: &str) -> Result<MethodCall, String> {
    let paren = s
        .find('(')
        .ok_or_else(|| format!("expected method call with '(': {s}"))?;
    let method = s[..paren].trim().to_string();
    if method.is_empty() {
        return Err("empty method name".to_string());
    }
    let args_str = &s[paren + 1..];
    let args_str = args_str
        .rfind(')')
        .map(|i| &args_str[..i])
        .unwrap_or(args_str);
    let args = parse_args(args_str)?;
    Ok(MethodCall { method, args })
}

fn parse_args(s: &str) -> Result<Vec<Expr>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let parts = split_top_level_commas(s);
    parts.iter().map(|p| parse_expr(p.trim())).collect()
}

pub(crate) fn parse_expr(s: &str) -> Result<Expr, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Expr::Null);
    }

    if s == "null" || s == "undefined" {
        return Ok(Expr::Null);
    }
    if s == "true" {
        return Ok(Expr::BoolLit(true));
    }
    if s == "false" {
        return Ok(Expr::BoolLit(false));
    }

    if let Ok(n) = s.parse::<i64>() {
        return Ok(Expr::IntLit(n));
    }
    if let Ok(n) = s.parse::<f64>() {
        if !n.is_finite() {
            return Err(format!("invalid number literal: {s}"));
        }
        return Ok(Expr::FloatLit(n));
    }

    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('\'') && s.ends_with('\''))
            || (s.starts_with('`') && s.ends_with('`')))
    {
        return Ok(Expr::StringLit(unescape_string(&s[1..s.len() - 1])));
    }
    if matches!(s, "\"" | "'" | "`") {
        return Err(format!("unterminated string literal: {s}"));
    }

    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        if inner.trim().is_empty() {
            return Ok(Expr::Array(Vec::new()));
        }
        let items = split_top_level_commas(inner);
        let exprs: Result<Vec<Expr>, String> = items.iter().map(|i| parse_expr(i.trim())).collect();
        return Ok(Expr::Array(exprs?));
    }

    if s.starts_with('{') && s.ends_with('}') {
        let inner = &s[1..s.len() - 1];
        return parse_object_fields(inner).map(Expr::Object);
    }

    if s.contains('.') && !s.starts_with(|c: char| c.is_ascii_digit()) {
        if let Some(dot) = s.rfind('.') {
            let obj = &s[..dot];
            let prop = &s[dot + 1..];
            if is_identifier(obj) && is_identifier(prop) {
                return Ok(Expr::PropAccess(obj.to_string(), prop.to_string()));
            }
        }
    }

    if is_identifier(s) {
        return Ok(Expr::VarRef(s.to_string()));
    }

    // NO lenient fallback: turning unrecognized expressions into string
    // literals of their own SOURCE TEXT silently corrupted results (observed
    // live: `s.text ?? s.stdout` surfaced as the literal string
    // "s.text ?? s.stdout"). Failing here routes the plan to the full
    // QuickJS sandbox, which evaluates real JavaScript.
    Err(format!("unsupported expression for lowered plan: {s}"))
}

fn parse_object_fields(s: &str) -> Result<Vec<(String, Expr)>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let parts = split_top_level_commas(s);
    let mut fields = Vec::new();
    for part in &parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon) = find_colon_outside_strings(part) {
            let key = part[..colon].trim().trim_matches(|c| c == '"' || c == '\'');
            let val = parse_expr(part[colon + 1..].trim())?;
            fields.push((key.to_string(), val));
        } else if is_identifier(part) {
            fields.push((part.to_string(), Expr::VarRef(part.to_string())));
        }
    }
    Ok(fields)
}

fn find_colon_outside_strings(s: &str) -> Option<usize> {
    let mut in_str = false;
    let mut quote = '"';
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                in_str = false;
            }
        } else if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            quote = c;
        } else if c == ':' {
            return Some(i);
        }
    }
    None
}

fn parse_return_expr(s: &str) -> Result<ReturnExpr, String> {
    let s = s.trim().trim_end_matches(';').trim();
    if s.starts_with('{') && s.ends_with('}') {
        let inner = &s[1..s.len() - 1];
        let fields = parse_object_fields(inner)?;
        return Ok(ReturnExpr::Object(fields));
    }
    if s.contains('.') {
        if let Some(dot) = s.rfind('.') {
            let obj = &s[..dot];
            let prop = &s[dot + 1..];
            if is_identifier(obj) && is_identifier(prop) {
                return Ok(ReturnExpr::PropAccess(obj.to_string(), prop.to_string()));
            }
        }
    }
    // Delegate to parse_expr which handles booleans, numbers, strings,
    // null, and identifiers in the correct precedence order.
    let expr = parse_expr(s)?;
    match expr {
        Expr::VarRef(name) => Ok(ReturnExpr::Var(name)),
        _ => Ok(ReturnExpr::Expr(expr)),
    }
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn unescape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('`') => out.push('`'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn split_statements(input: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut current = String::new();
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_str = false;
    let mut str_quote = '"';
    let mut escaped = false;

    for ch in input.chars() {
        if in_str {
            current.push(ch);
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == str_quote {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => {
                in_str = true;
                str_quote = ch;
                current.push(ch);
            }
            '(' => {
                depth_paren += 1;
                current.push(ch);
            }
            ')' => {
                depth_paren -= 1;
                current.push(ch);
            }
            '{' => {
                depth_brace += 1;
                current.push(ch);
            }
            '}' => {
                depth_brace -= 1;
                current.push(ch);
            }
            '[' => {
                depth_bracket += 1;
                current.push(ch);
            }
            ']' => {
                depth_bracket -= 1;
                current.push(ch);
            }
            ';' | '\n' if depth_paren <= 0 && depth_brace <= 0 && depth_bracket <= 0 => {
                let s = current.trim().to_string();
                if !s.is_empty() {
                    stmts.push(s);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let s = current.trim().to_string();
    if !s.is_empty() {
        stmts.push(s);
    }
    stmts
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut str_quote = '"';
    let mut escaped = false;
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == str_quote {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => {
                in_str = true;
                str_quote = ch;
            }
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

pub(crate) fn resolve_expr(expr: &Expr, scope: &HashMap<String, Value>) -> Result<Value, String> {
    match expr {
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::IntLit(n) => {
            if *n >= 0 {
                Ok(json!(*n as u64))
            } else {
                Ok(json!(*n))
            }
        }
        Expr::FloatLit(n) => Ok(json!(*n)),
        Expr::BoolLit(b) => Ok(json!(b)),
        Expr::Null => Ok(Value::Null),
        Expr::Array(items) => Ok(Value::Array(
            items
                .iter()
                .map(|item| resolve_expr(item, scope))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Expr::Object(fields) => {
            let obj: serde_json::Map<String, Value> = fields
                .iter()
                .map(|(key, value)| {
                    resolve_expr(value, scope).map(|resolved| (key.clone(), resolved))
                })
                .collect::<Result<_, _>>()?;
            Ok(Value::Object(obj))
        }
        Expr::VarRef(name) => scope
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined variable: {name}")),
        Expr::PropAccess(obj, prop) => {
            let value = scope
                .get(obj)
                .ok_or_else(|| format!("undefined variable: {obj}"))?;
            value
                .get(prop)
                .cloned()
                .ok_or_else(|| format!("undefined property: {obj}.{prop}"))
        }
    }
}

pub(crate) fn resolve_return(
    expr: &ReturnExpr,
    scope: &HashMap<String, Value>,
) -> Result<Value, String> {
    match expr {
        ReturnExpr::Var(name) => scope
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined variable: {name}")),
        ReturnExpr::PropAccess(obj, prop) => {
            let value = scope
                .get(obj)
                .ok_or_else(|| format!("undefined variable: {obj}"))?;
            value
                .get(prop)
                .cloned()
                .ok_or_else(|| format!("undefined property: {obj}.{prop}"))
        }
        ReturnExpr::Object(fields) => {
            let obj: serde_json::Map<String, Value> = fields
                .iter()
                .map(|(key, value)| {
                    resolve_expr(value, scope).map(|resolved| (key.clone(), resolved))
                })
                .collect::<Result<_, _>>()?;
            Ok(Value::Object(obj))
        }
        ReturnExpr::Expr(expression) => resolve_expr(expression, scope),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_literal_int_folds_correctly() {
        let plan = "return 12345;";
        let stmts = parse_plan(plan).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Return(ReturnExpr::Expr(Expr::IntLit(n))) => assert_eq!(*n, 12345),
            other => panic!("expected Return(Expr(IntLit(12345))), got {:?}", other),
        }
    }

    #[test]
    fn return_literal_string_folds_correctly() {
        let plan = r#"return "hello";"#;
        let stmts = parse_plan(plan).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Return(ReturnExpr::Expr(Expr::StringLit(s))) => assert_eq!(s, "hello"),
            other => panic!("expected Return(Expr(StringLit(hello))), got {:?}", other),
        }
    }

    #[test]
    fn return_literal_bool_folds_correctly() {
        let plan = "return true;";
        let stmts = parse_plan(plan).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Statement::Return(ReturnExpr::Expr(Expr::BoolLit(b))) => assert!(b),
            other => panic!("expected Return(Expr(BoolLit(true))), got {:?}", other),
        }
    }
}
