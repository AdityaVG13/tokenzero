//! JS-like plan parser for CodeMode execution.

use serde_json::{json, Value};
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
    LogicalOr(Box<Expr>, Box<Expr>),
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

    if let Some(index) = find_top_level_logical_or(s) {
        return Ok(Expr::LogicalOr(
            Box::new(parse_expr(&s[..index])?),
            Box::new(parse_expr(&s[index + 2..])?),
        ));
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
    match parse_expr(s.trim().trim_end_matches(';').trim())? {
        Expr::Object(fields) => Ok(ReturnExpr::Object(fields)),
        Expr::PropAccess(object, property) => Ok(ReturnExpr::PropAccess(object, property)),
        Expr::VarRef(name) => Ok(ReturnExpr::Var(name)),
        expression => Ok(ReturnExpr::Expr(expression)),
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

#[derive(Default)]
struct Scanner {
    depths: [i32; 3],
    quote: Option<char>,
    escaped: bool,
}
impl Scanner {
    fn at_top(&self) -> bool {
        self.quote.is_none() && self.depths.iter().all(|depth| *depth <= 0)
    }
    fn consume(&mut self, ch: char) {
        if let Some(quote) = self.quote {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == quote {
                self.quote = None;
            }
            return;
        }
        if ch == '"' || ch == '\'' || ch as u32 == 96 {
            self.quote = Some(ch);
            return;
        }
        match ch {
            '(' => self.depths[0] += 1,
            ')' => self.depths[0] -= 1,
            '{' => self.depths[1] += 1,
            '}' => self.depths[1] -= 1,
            '[' => self.depths[2] += 1,
            ']' => self.depths[2] -= 1,
            _ => {}
        }
    }
}
fn split_statements(input: &str) -> Vec<String> {
    let (mut parts, mut current, mut scanner) = (Vec::new(), String::new(), Scanner::default());
    for ch in input.chars() {
        if matches!(ch, ';' | '\n') && scanner.at_top() {
            if !current.trim().is_empty() {
                parts.push(current.trim().to_string());
            }
            current.clear();
        } else {
            current.push(ch);
            scanner.consume(ch);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}
fn find_top_level_logical_or(s: &str) -> Option<usize> {
    let mut scanner = Scanner::default();
    let mut chars = s.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '|' && scanner.at_top() && chars.peek().is_some_and(|(_, next)| *next == '|') {
            return Some(index);
        }
        scanner.consume(ch);
    }
    None
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let (mut parts, mut start, mut scanner) = (Vec::new(), 0, Scanner::default());
    for (index, ch) in s.char_indices() {
        if ch == ',' && scanner.at_top() {
            parts.push(&s[start..index]);
            start = index + 1;
        } else {
            scanner.consume(ch);
        }
    }
    parts.push(&s[start..]);
    parts
}

fn resolve_name(
    scope: &HashMap<String, Value>,
    object: &str,
    property: Option<&str>,
) -> Result<Value, String> {
    let value = scope
        .get(object)
        .ok_or_else(|| format!("undefined variable: {object}"))?;
    property.map_or_else(
        || Ok(value.clone()),
        |property| {
            value
                .get(property)
                .cloned()
                .ok_or_else(|| format!("undefined property: {object}.{property}"))
        },
    )
}
fn resolve_fields(
    fields: &[(String, Expr)],
    scope: &HashMap<String, Value>,
) -> Result<Value, String> {
    fields
        .iter()
        .map(|(key, value)| resolve_expr(value, scope).map(|value| (key.clone(), value)))
        .collect::<Result<serde_json::Map<_, _>, _>>()
        .map(Value::Object)
}
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value
            .as_f64()
            .is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

pub(crate) fn resolve_expr(expr: &Expr, scope: &HashMap<String, Value>) -> Result<Value, String> {
    match expr {
        Expr::StringLit(value) => Ok(Value::String(value.clone())),
        Expr::IntLit(value) if *value >= 0 => Ok(json!(*value as u64)),
        Expr::IntLit(value) => Ok(json!(*value)),
        Expr::FloatLit(value) => Ok(json!(*value)),
        Expr::BoolLit(value) => Ok(json!(value)),
        Expr::Null => Ok(Value::Null),
        Expr::Array(items) => items
            .iter()
            .map(|item| resolve_expr(item, scope))
            .collect::<Result<_, _>>()
            .map(Value::Array),
        Expr::Object(fields) => resolve_fields(fields, scope),
        Expr::VarRef(name) => resolve_name(scope, name, None),
        Expr::PropAccess(object, property) => resolve_name(scope, object, Some(property)),
        Expr::LogicalOr(left, right) => {
            let left = resolve_expr(left, scope)?;
            if js_truthy(&left) {
                Ok(left)
            } else {
                resolve_expr(right, scope)
            }
        }
    }
}
pub(crate) fn resolve_return(
    expr: &ReturnExpr,
    scope: &HashMap<String, Value>,
) -> Result<Value, String> {
    match expr {
        ReturnExpr::Var(name) => resolve_name(scope, name, None),
        ReturnExpr::PropAccess(object, property) => resolve_name(scope, object, Some(property)),
        ReturnExpr::Object(fields) => resolve_fields(fields, scope),
        ReturnExpr::Expr(expression) => resolve_expr(expression, scope),
    }
}
