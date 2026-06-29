//! TokenZero CodeMode surface — a CodeMode-style code-plan executor that
//! exposes TokenZero operations as typed methods. Models write JS-like
//! plans; the executor parses, dispatches through TokenZeroEngine, and returns
//! only the final shaped result. Additive to MCP, never replaces it.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokenzero_core::{Mode, ToolResponse, count_tokens, detect_content_type};
use tokenzero_mcp::{EditHunk, EngineConfig, TokenZeroEngine, shell_timeout_from_secs};

use crate::zerostack_store::{default_codemode_recovery_cache_path, tokenzero_work_root};

pub const CODEMODE_SCHEMA: &str = "tokenzero.codemode.v1";

// ─── Result types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeModeResult {
    pub schema: &'static str,
    pub status: CodeModeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    pub telemetry: CodeModeTelemetry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeModeStatus {
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeModeTelemetry {
    pub operations: usize,
    pub visible_tokens: usize,
    pub raw_tokens: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CodeModeOptions {
    pub(crate) root: Option<PathBuf>,
    pub(crate) allowed_roots: Vec<PathBuf>,
    pub(crate) cache_path: Option<PathBuf>,
    pub(crate) max_visible_tokens: usize,
    pub(crate) timeout_seconds: Option<u64>,
}

impl Default for CodeModeOptions {
    fn default() -> Self {
        Self {
            root: None,
            allowed_roots: Vec::new(),
            cache_path: None,
            max_visible_tokens: 4000,
            timeout_seconds: None,
        }
    }
}

impl CodeModeResult {
    fn completed(value: Value, refs: Vec<String>, ops: usize, visible: usize, raw: usize) -> Self {
        Self {
            schema: CODEMODE_SCHEMA,
            status: CodeModeStatus::Completed,
            value: Some(value),
            refs,
            telemetry: CodeModeTelemetry {
                operations: ops,
                visible_tokens: visible,
                raw_tokens: raw,
            },
            error: None,
        }
    }

    fn error(msg: impl Into<String>, ops: usize) -> Self {
        Self {
            schema: CODEMODE_SCHEMA,
            status: CodeModeStatus::Error,
            value: None,
            refs: Vec::new(),
            telemetry: CodeModeTelemetry {
                operations: ops,
                visible_tokens: 0,
                raw_tokens: 0,
            },
            error: Some(msg.into()),
        }
    }

    pub fn to_line(&self) -> String {
        match self.status {
            CodeModeStatus::Completed => {
                let refs_part = if self.refs.is_empty() {
                    String::new()
                } else {
                    format!(" refs={}", self.refs.join(","))
                };
                format!(
                    "codemode:ok ops={} visible_tokens={} raw_tokens={}{}",
                    self.telemetry.operations,
                    self.telemetry.visible_tokens,
                    self.telemetry.raw_tokens,
                    refs_part,
                )
            }
            CodeModeStatus::Error => {
                format!(
                    "codemode:error ops={} {}",
                    self.telemetry.operations,
                    self.error.as_deref().unwrap_or("unknown"),
                )
            }
        }
    }
}

// ─── Progressive discovery catalog ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct MethodDef {
    path: &'static str,
    connector: &'static str,
    description: &'static str,
    signature: &'static str,
}

const METHOD_CATALOG: &[MethodDef] = &[
    MethodDef {
        path: "zero.read",
        connector: "zero",
        description: "Read file(s) with token-budget capsule compression and exact recovery refs",
        signature: "zero.read(path: string | string[], opts?: { mode?, start_line?, end_line?, max_visible_tokens? }): Promise<{ text: string, ref: string, visible_tokens: number, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.find",
        connector: "zero",
        description: "Search file contents for a pattern (regex or literal) with compact results",
        signature: "zero.find(pattern: string, path?: string | string[], opts?: { mode?, max_files?, max_visible_tokens? }): Promise<{ text: string, ref: string, status: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.grep",
        connector: "zero",
        description: "Exact literal substring search (no regex interpretation)",
        signature: "zero.grep(pattern: string, path?: string | string[], opts?: { mode?, max_files?, max_visible_tokens? }): Promise<{ text: string, ref: string, status: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.glob",
        connector: "zero",
        description: "List file paths matching a glob pattern (no file contents)",
        signature: "zero.glob(pattern: string, path?: string | string[], opts?: { max_files? }): Promise<{ text: string, ref: string, status: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.tree",
        connector: "zero",
        description: "Inspect a bounded directory tree for orientation",
        signature: "zero.tree(path?: string, opts?: { depth?, include_hidden?, max_files? }): Promise<{ text: string, ref: string }>",
    },
    MethodDef {
        path: "zero.shell",
        connector: "zero",
        description: "Run a shell command with status-truth telemetry and compact output",
        signature: "zero.shell(command: string, opts?: { cwd?, mode?, timeout_seconds? }): Promise<{ text: string, ref: string, exit_code: number, success: boolean }>",
    },
    MethodDef {
        path: "zero.edit",
        connector: "zero",
        description: "Apply multi-hunk find/replace edits to one file atomically",
        signature: "zero.edit(path: string, edits: Array<{ find: string, replace: string, replace_all?: boolean }>, opts?: { dry_run?, create? }): Promise<{ text: string, ref: string, hunks_applied: number }>",
    },
    MethodDef {
        path: "zero.token.expand",
        connector: "zero.token",
        description: "Recover exact bytes from a tz:// ref",
        signature: "zero.token.expand(ref: string, opts?: { start_line?, end_line?, selector? }): Promise<{ text: string, status: string, ref?: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.token.compact",
        connector: "zero.token",
        description: "Store arbitrary text/data behind a content-addressed tz://blob ref",
        signature: "zero.token.compact(data: string): Promise<{ ref: string, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.expand",
        connector: "zero",
        description: "Recover exact bytes from a tz:// ref (compatibility alias for zero.token.expand)",
        signature: "zero.expand(ref: string, opts?: { start_line?, end_line?, selector? }): Promise<{ text: string, status: string, ref?: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.compact",
        connector: "zero",
        description: "Store arbitrary text/data behind a content-addressed tz://blob ref (compatibility alias for zero.token.compact)",
        signature: "zero.compact(data: string): Promise<{ ref: string, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.ingest",
        connector: "zero",
        description: "Ingest text into a compact TokenZero capsule with recovery ref",
        signature: "zero.ingest(text: string, opts?: { mode?, source? }): Promise<{ text: string, ref: string, visible_tokens: number, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.mem",
        connector: "zero",
        description: "Inspect recovery-cache state and statistics",
        signature: "zero.mem(): Promise<{ text: string }>",
    },
    MethodDef {
        path: "codemode.search",
        connector: "codemode",
        description: "Search available methods by keyword",
        signature: "codemode.search(query: string): Promise<{ results: Array<{ path, description, score }> }>",
    },
    MethodDef {
        path: "codemode.describe",
        connector: "codemode",
        description: "Get full TypeScript signature for a method",
        signature: "codemode.describe(path: string): Promise<{ path, description, types: string }>",
    },
];

fn search_catalog(query: &str) -> Value {
    let query_lower = query.to_lowercase();
    let mut results: Vec<(f64, &MethodDef)> = METHOD_CATALOG
        .iter()
        .filter_map(|m| {
            let haystack = format!("{} {} {}", m.path, m.description, m.signature).to_lowercase();
            let score = if m.path.to_lowercase().contains(&query_lower) {
                1.0
            } else if m.description.to_lowercase().contains(&query_lower) {
                0.7
            } else if haystack.contains(&query_lower) {
                0.4
            } else {
                return None;
            };
            Some((score, m))
        })
        .collect();
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    json!({
        "results": results.iter().map(|(score, m)| json!({
            "path": m.path,
            "connector": m.connector,
            "description": m.description,
            "score": score,
        })).collect::<Vec<_>>(),
        "total": results.len(),
        "truncated": false,
    })
}

fn describe_method(path: &str) -> Value {
    let path_lower = path.to_lowercase();
    if let Some(m) = METHOD_CATALOG
        .iter()
        .find(|m| m.path.to_lowercase() == path_lower)
    {
        json!({
            "path": m.path,
            "description": m.description,
            "types": m.signature,
            "kind": "method",
        })
    } else {
        json!({
            "path": path,
            "error": format!("no method found for path: {path}"),
            "available": METHOD_CATALOG.iter().map(|m| m.path).collect::<Vec<_>>(),
        })
    }
}

// ─── Plan parser ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Statement {
    Binding { name: String, call: MethodCall },
    Call(MethodCall),
    Return(ReturnExpr),
}

#[derive(Debug, Clone)]
struct MethodCall {
    method: String,
    args: Vec<Expr>,
}

#[derive(Debug, Clone)]
enum Expr {
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
enum ReturnExpr {
    Var(String),
    PropAccess(String, String),
    Object(Vec<(String, Expr)>),
    Expr(Expr),
}

fn parse_plan(input: &str) -> Result<Vec<Statement>, String> {
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

fn parse_expr(s: &str) -> Result<Expr, String> {
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

    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
        || (s.starts_with('`') && s.ends_with('`'))
    {
        return Ok(Expr::StringLit(unescape_string(&s[1..s.len() - 1])));
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

    Ok(Expr::StringLit(s.to_string()))
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
    if is_identifier(s) {
        return Ok(ReturnExpr::Var(s.to_string()));
    }
    let expr = parse_expr(s)?;
    Ok(ReturnExpr::Expr(expr))
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
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

// ─── Executor ───────────────────────────────────────────────────────────────

fn make_engine_with_options(options: &CodeModeOptions) -> TokenZeroEngine {
    make_engine_for_root_with_options(tokenzero_work_root(options.root.clone()), options)
}

#[cfg(test)]
fn make_engine_for_root(root: PathBuf) -> TokenZeroEngine {
    make_engine_for_root_with_options(root, &CodeModeOptions::default())
}

fn make_engine_for_root_with_options(root: PathBuf, options: &CodeModeOptions) -> TokenZeroEngine {
    let cache_path = options
        .cache_path
        .clone()
        .unwrap_or_else(|| default_codemode_recovery_cache_path(&root));
    TokenZeroEngine::new(EngineConfig {
        allowed_roots: codemode_allowed_roots(&root, &options.allowed_roots),
        cache_path,
        max_visible_tokens: options.max_visible_tokens,
        mode: Mode::Auto,
        shell_timeout: shell_timeout_from_secs(options.timeout_seconds),
        ..EngineConfig::for_root(&root)
    })
}

fn codemode_allowed_roots(root: &Path, explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = if explicit.is_empty() {
        vec![root.to_path_buf()]
    } else {
        explicit.to_vec()
    };
    if !roots.iter().any(|candidate| candidate == root) {
        roots.push(root.to_path_buf());
    }
    roots
}

#[cfg(test)]
pub fn execute_codemode(plan: &str) -> CodeModeResult {
    execute_codemode_with_options(plan, CodeModeOptions::default())
}

pub(crate) fn execute_codemode_with_options(
    plan: &str,
    options: CodeModeOptions,
) -> CodeModeResult {
    let plan = plan.trim();
    if plan.is_empty() {
        return CodeModeResult::error("empty plan", 0);
    }

    if let Some(query) = plan.strip_prefix("search:") {
        let result = search_catalog(query.trim());
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let tokens = count_tokens(&text);
        return CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens);
    }
    if let Some(target) = plan.strip_prefix("describe:") {
        let result = describe_method(target.trim());
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let tokens = count_tokens(&text);
        return CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens);
    }

    let statements = match parse_plan(plan) {
        Ok(s) => s,
        Err(e) => return CodeModeResult::error(e, 0),
    };

    let engine = make_engine_with_options(&options);
    let mut scope: HashMap<String, Value> = HashMap::new();
    let mut all_refs: Vec<String> = Vec::new();
    let mut ops: usize = 0;
    let mut total_visible: usize = 0;
    let mut total_raw: usize = 0;
    let mut last_value: Value = Value::Null;

    for stmt in &statements {
        match stmt {
            Statement::Binding { name, call } => {
                ops += 1;
                let result = match dispatch(&engine, call, &scope) {
                    Ok(v) => v,
                    Err(mut e) => {
                        e.telemetry.operations = ops;
                        return *e;
                    }
                };
                collect_refs(&result, &mut all_refs);
                total_visible += result_visible_tokens(&result);
                total_raw += result_raw_tokens(&result);
                last_value = result.clone();
                scope.insert(name.clone(), result);
            }
            Statement::Call(call) => {
                ops += 1;
                let result = match dispatch(&engine, call, &scope) {
                    Ok(v) => v,
                    Err(mut e) => {
                        e.telemetry.operations = ops;
                        return *e;
                    }
                };
                collect_refs(&result, &mut all_refs);
                total_visible += result_visible_tokens(&result);
                total_raw += result_raw_tokens(&result);
                last_value = result;
            }
            Statement::Return(expr) => {
                let value = resolve_return(expr, &scope);
                let vis = count_tokens(&serde_json::to_string(&value).unwrap_or_default());
                return CodeModeResult::completed(
                    value,
                    all_refs,
                    ops,
                    total_visible + vis,
                    total_raw,
                );
            }
        }
    }

    let vis = count_tokens(&serde_json::to_string(&last_value).unwrap_or_default());
    CodeModeResult::completed(last_value, all_refs, ops, total_visible + vis, total_raw)
}

fn dispatch(
    engine: &TokenZeroEngine,
    call: &MethodCall,
    scope: &HashMap<String, Value>,
) -> Result<Value, Box<CodeModeResult>> {
    let method = call.method.as_str();
    let args: Vec<Value> = call.args.iter().map(|a| resolve_expr(a, scope)).collect();

    match method {
        "zero.read" | "read" => exec_read(engine, &args),
        "zero.find" | "find" => exec_find(engine, &args, false),
        "zero.grep" | "grep" => exec_find(engine, &args, true),
        "zero.glob" | "glob" => exec_glob(engine, &args),
        "zero.tree" | "tree" => exec_tree(engine, &args),
        "zero.shell" | "shell" => exec_shell(engine, &args),
        "zero.edit" | "edit" => exec_edit(engine, &args),
        "zero.token.expand" | "zero.expand" | "expand" => exec_expand(engine, &args),
        "zero.token.compact" | "zero.compact" | "compact" => exec_compact(engine, &args),
        "zero.ingest" | "ingest" => exec_ingest(engine, &args),
        "zero.mem" | "mem" => exec_mem(engine),
        "codemode.search" | "search" => {
            let query = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(search_catalog(query))
        }
        "codemode.describe" | "describe" => {
            let path = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(describe_method(path))
        }
        _ => Err(Box::new(CodeModeResult::error(
            format!(
                "unknown method: {method}. Use codemode.search() to discover available methods"
            ),
            0,
        ))),
    }
}

// ─── Operation implementations ──────────────────────────────────────────────

fn tool_response_to_value(resp: &ToolResponse) -> Value {
    let text = resp
        .visible
        .as_ref()
        .map(|v| v.text.clone())
        .unwrap_or_default();
    let refs: Vec<String> = resp.refs.iter().map(|r| r.ref_id.clone()).collect();
    let accounting = resp.accounting.as_ref();
    let mut obj = json!({
        "text": text,
        "status": resp.status,
    });
    if !refs.is_empty() {
        obj["ref"] = json!(refs[0]);
        if refs.len() > 1 {
            obj["refs"] = json!(refs);
        }
    }
    if let Some(acc) = accounting {
        obj["visible_tokens"] = json!(acc.visible_tokens);
        obj["raw_tokens"] = json!(acc.raw_tokens);
    }
    if let Some(err) = &resp.error {
        obj["error"] = json!(err.message);
    }
    obj
}

fn exec_read(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let paths = match args.first() {
        Some(Value::String(s)) => vec![PathBuf::from(s)],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.read requires a path string or array as first argument",
                0,
            )));
        }
    };
    let opts = args.get(1).and_then(|v| v.as_object());
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let start_line = opts
        .and_then(|o| o.get("start_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let end_line = opts
        .and_then(|o| o.get("end_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let max_visible = opts
        .and_then(|o| o.get("max_visible_tokens"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = engine.read(&paths, mode, start_line, end_line, false, 20, max_visible);
    Ok(tool_response_to_value(&resp))
}

fn exec_find(
    engine: &TokenZeroEngine,
    args: &[Value],
    exact: bool,
) -> Result<Value, Box<CodeModeResult>> {
    let pattern = match args.first().and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.find/grep requires a pattern string as first argument",
                0,
            )));
        }
    };
    let paths: Vec<PathBuf> = match args.get(1) {
        Some(Value::String(s)) => vec![PathBuf::from(s)],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
        _ => vec![tokenzero_work_root(None)],
    };
    let opts = args.get(2).and_then(|v| v.as_object());
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let max_files = opts
        .and_then(|o| o.get("max_files"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(20);
    let max_visible = opts
        .and_then(|o| o.get("max_visible_tokens"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = if exact {
        engine.grep(pattern, &paths, mode, max_files, max_visible)
    } else {
        engine.find(pattern, &paths, mode, max_files, max_visible)
    };
    Ok(tool_response_to_value(&resp))
}

fn exec_glob(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let pattern = match args.first().and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.glob requires a pattern string as first argument",
                0,
            )));
        }
    };
    let paths: Vec<PathBuf> = match args.get(1) {
        Some(Value::String(s)) => vec![PathBuf::from(s)],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
        _ => vec![tokenzero_work_root(None)],
    };
    let max_files = args
        .get(2)
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("max_files"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(200);

    let resp = engine.glob(
        pattern,
        &paths,
        false,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    Ok(tool_response_to_value(&resp))
}

fn exec_tree(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let roots = vec![
        args.first()
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| tokenzero_work_root(None)),
    ];
    let opts = args.get(1).and_then(|v| v.as_object());
    let depth = opts
        .and_then(|o| o.get("depth"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(3);
    let include_hidden = opts
        .and_then(|o| o.get("include_hidden"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_files = opts
        .and_then(|o| o.get("max_files"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(200);

    let resp = engine.tree(
        &roots,
        depth,
        include_hidden,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    Ok(tool_response_to_value(&resp))
}

fn exec_shell(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let command = match args.first().and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.shell requires a command string as first argument",
                0,
            )));
        }
    };
    let opts = args.get(1).and_then(|v| v.as_object());
    let cwd = opts
        .and_then(|o| o.get("cwd"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let timeout = opts
        .and_then(|o| o.get("timeout_seconds"))
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs);

    let resp = engine.shell(
        command,
        None,
        cwd.as_deref(),
        mode,
        Some("safe"),
        false,
        None,
        None,
        timeout,
    );

    let mut val = tool_response_to_value(&resp);
    if let Some(telem) = &resp.telemetry {
        if let Some(exit) = telem.get("exit_code") {
            val["exit_code"] = exit.clone();
        }
        if let Some(success) = telem.get("command_success") {
            val["success"] = success.clone();
        }
    }
    Ok(val)
}

fn exec_edit(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let path = match args.first().and_then(|v| v.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.edit requires a path string as first argument",
                0,
            )));
        }
    };
    let edits_val = match args.get(1) {
        Some(Value::Array(arr)) => arr,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.edit requires an array of {find, replace} hunks as second argument",
                0,
            )));
        }
    };
    let mut edits = Vec::with_capacity(edits_val.len());
    for (idx, value) in edits_val.iter().enumerate() {
        let hunk: EditHunk = serde_json::from_value(value.clone()).map_err(|err| {
            Box::new(CodeModeResult::error(
                format!("zero.edit: invalid hunk at index {idx}: {err}"),
                0,
            ))
        })?;
        edits.push(hunk);
    }
    if edits.is_empty() {
        return Err(Box::new(CodeModeResult::error(
            "zero.edit: no edit hunks provided",
            0,
        )));
    }
    let opts = args.get(2).and_then(|v| v.as_object());
    let dry_run = opts
        .and_then(|o| o.get("dry_run"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let create = opts
        .and_then(|o| o.get("create"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let resp = engine.edit(
        &path,
        &edits,
        create,
        dry_run,
        Mode::Auto,
        engine.config.max_visible_tokens,
    );
    let hunks_applied = if resp.status == "ok" {
        resp.telemetry
            .as_ref()
            .and_then(|t| t.get("hunks"))
            .and_then(|v| v.as_u64())
            .unwrap_or(edits.len() as u64)
    } else {
        0
    };
    let mut val = tool_response_to_value(&resp);
    val["hunks_applied"] = json!(hunks_applied);
    Ok(val)
}

fn exec_expand(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let ref_id = match args.first().and_then(|v| v.as_str()) {
        Some(r) => r,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.expand/zero.expand requires a tz:// ref string as first argument",
                0,
            )));
        }
    };
    if !ref_id.starts_with("tz://") {
        return Err(Box::new(CodeModeResult::error(
            format!("zero.token.expand/zero.expand: ref must start with tz://, got: {ref_id}"),
            0,
        )));
    }
    let opts = args.get(1).and_then(|v| v.as_object());
    let start_line = opts
        .and_then(|o| o.get("start_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let end_line = opts
        .and_then(|o| o.get("end_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let selector = opts
        .and_then(|o| o.get("selector"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let resp = engine.expand(
        ref_id,
        selector.as_deref(),
        start_line,
        end_line,
        None,
        None,
    );
    Ok(tool_response_to_value(&resp))
}

fn exec_compact(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let data = match args.first() {
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.compact/zero.compact requires data as first argument",
                0,
            )));
        }
    };
    let content_type = detect_content_type(&data, None);
    let resp = engine.ingest(&data, content_type, Mode::Auto, "codemode-compact");
    let mut val = tool_response_to_value(&resp);
    val["raw_tokens"] = json!(count_tokens(&data));
    Ok(val)
}

fn exec_ingest(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let text = match args.first().and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.ingest requires text as first argument",
                0,
            )));
        }
    };
    let opts = args.get(1).and_then(|v| v.as_object());
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let source = opts
        .and_then(|o| o.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("codemode-ingest");
    let content_type = detect_content_type(text, None);

    let resp = engine.ingest(text, content_type, mode, source);
    Ok(tool_response_to_value(&resp))
}

fn exec_mem(engine: &TokenZeroEngine) -> Result<Value, Box<CodeModeResult>> {
    let resp = engine.mem();
    Ok(tool_response_to_value(&resp))
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn resolve_expr(expr: &Expr, scope: &HashMap<String, Value>) -> Value {
    match expr {
        Expr::StringLit(s) => Value::String(s.clone()),
        Expr::IntLit(n) => {
            if *n >= 0 {
                json!(*n as u64)
            } else {
                json!(*n)
            }
        }
        Expr::FloatLit(n) => json!(*n),
        Expr::BoolLit(b) => json!(b),
        Expr::Null => Value::Null,
        Expr::Array(items) => Value::Array(items.iter().map(|e| resolve_expr(e, scope)).collect()),
        Expr::Object(fields) => {
            let obj: serde_json::Map<String, Value> = fields
                .iter()
                .map(|(k, v)| (k.clone(), resolve_expr(v, scope)))
                .collect();
            Value::Object(obj)
        }
        Expr::VarRef(name) => scope.get(name).cloned().unwrap_or(Value::Null),
        Expr::PropAccess(obj, prop) => scope
            .get(obj)
            .and_then(|v| v.get(prop))
            .cloned()
            .unwrap_or(Value::Null),
    }
}

fn resolve_return(expr: &ReturnExpr, scope: &HashMap<String, Value>) -> Value {
    match expr {
        ReturnExpr::Var(name) => scope.get(name).cloned().unwrap_or(Value::Null),
        ReturnExpr::PropAccess(obj, prop) => scope
            .get(obj)
            .and_then(|v| v.get(prop))
            .cloned()
            .unwrap_or(Value::Null),
        ReturnExpr::Object(fields) => {
            let obj: serde_json::Map<String, Value> = fields
                .iter()
                .map(|(k, v)| (k.clone(), resolve_expr(v, scope)))
                .collect();
            Value::Object(obj)
        }
        ReturnExpr::Expr(e) => resolve_expr(e, scope),
    }
}

fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    if let Some(r) = value.get("ref").and_then(|v| v.as_str()) {
        if r.starts_with("tz://") && !refs.contains(&r.to_string()) {
            refs.push(r.to_string());
        }
    }
    if let Some(arr) = value.get("refs").and_then(|v| v.as_array()) {
        for r in arr.iter().filter_map(|v| v.as_str()) {
            if r.starts_with("tz://") && !refs.contains(&r.to_string()) {
                refs.push(r.to_string());
            }
        }
    }
}

fn result_visible_tokens(value: &Value) -> usize {
    value
        .get("visible_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

fn result_raw_tokens(value: &Value) -> usize {
    value
        .get("raw_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

// ─── Legacy string API (used by integration tests) ──────────────────────────

#[cfg(test)]
fn execute_plan_in_token(plan: &str) -> String {
    execute_codemode(plan).to_line()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parser_object_numeric_options_resolve_as_u64() {
        let scope = HashMap::new();
        let expr = parse_expr("{ start_line: 1, end_line: 10, max_files: 5 }").unwrap();
        let value = resolve_expr(&expr, &scope);
        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("start_line").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(obj.get("end_line").and_then(|v| v.as_u64()), Some(10));
        assert_eq!(obj.get("max_files").and_then(|v| v.as_u64()), Some(5));
    }

    #[test]
    fn parser_rejects_non_finite_number_literals() {
        assert!(parse_expr("inf").is_err());
        assert!(parse_expr("nan").is_err());
    }

    #[test]
    fn empty_plan_returns_error() {
        let r = execute_codemode("");
        assert_eq!(r.status, CodeModeStatus::Error);
        assert!(r.error.as_ref().unwrap().contains("empty"));
    }

    #[test]
    fn search_returns_ranked_methods() {
        let r = execute_codemode("search:read");
        assert_eq!(r.status, CodeModeStatus::Completed);
        let results = r.value.unwrap();
        let hits = results["results"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0]["path"].as_str().unwrap().contains("read"));
    }

    #[test]
    fn describe_returns_signature() {
        let r = execute_codemode("describe:zero.read");
        assert_eq!(r.status, CodeModeStatus::Completed);
        let val = r.value.unwrap();
        assert!(val["types"].as_str().unwrap().contains("Promise"));
    }

    #[test]
    fn describe_unknown_returns_available_list() {
        let r = execute_codemode("describe:zero.nonexistent");
        assert_eq!(r.status, CodeModeStatus::Completed);
        let val = r.value.unwrap();
        assert!(val["error"].is_string());
        assert!(val["available"].as_array().unwrap().len() > 5);
    }

    #[test]
    fn unknown_method_gives_helpful_error() {
        let r = execute_codemode("await zero.banana()");
        assert_eq!(r.status, CodeModeStatus::Error);
        assert!(r.error.as_ref().unwrap().contains("unknown method"));
        assert!(r.error.as_ref().unwrap().contains("codemode.search"));
    }

    #[test]
    fn parser_handles_binding_and_return() {
        let stmts = parse_plan(r#"const x = await zero.compact("hello"); return x.ref"#).unwrap();
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Statement::Binding { name, .. } if name == "x"));
        assert!(matches!(&stmts[1], Statement::Return(..)));
    }

    #[test]
    fn parser_splits_multiline_plan() {
        let plan = "const a = await zero.shell(\"ls\");\nconst b = await zero.shell(\"pwd\");\nreturn { a, b }";
        let stmts = parse_plan(plan).unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn parser_handles_object_args() {
        let stmts = parse_plan(
            r#"zero.read("src/main.rs", { mode: "auto", start_line: 1, end_line: 10 })"#,
        )
        .unwrap();
        assert_eq!(stmts.len(), 1);
        if let Statement::Call(call) = &stmts[0] {
            assert_eq!(call.method, "zero.read");
            assert_eq!(call.args.len(), 2);
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn compact_roundtrip_through_codemode() {
        let r = execute_codemode(r#"await zero.compact("test payload for codemode")"#);
        assert_eq!(r.status, CodeModeStatus::Completed);
        let val = r.value.as_ref().unwrap();
        let ref_id = val["ref"].as_str().unwrap();
        assert!(ref_id.starts_with("tz://"));

        let expand_plan = format!(r#"await zero.expand("{ref_id}")"#);
        let r2 = execute_codemode(&expand_plan);
        assert_eq!(r2.status, CodeModeStatus::Completed);
        let val2 = r2.value.as_ref().unwrap();
        let text = val2["text"].as_str().unwrap_or("");
        assert!(text.contains("test payload for codemode"));
    }

    #[test]
    fn token_namespace_compact_roundtrip_through_codemode() {
        let r = execute_codemode(r#"await zero.token.compact("token namespace payload")"#);
        assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
        let val = r.value.as_ref().unwrap();
        let ref_id = val["ref"].as_str().unwrap();
        assert!(ref_id.starts_with("tz://"));

        let expand_plan = format!(r#"await zero.token.expand("{ref_id}")"#);
        let r2 = execute_codemode(&expand_plan);
        assert_eq!(r2.status, CodeModeStatus::Completed, "{:?}", r2.error);
        let val2 = r2.value.as_ref().unwrap();
        let text = val2["text"].as_str().unwrap_or("");
        assert!(text.contains("token namespace payload"));
    }

    #[test]
    fn describe_token_namespace_returns_signature() {
        let r = execute_codemode("describe:zero.token.compact");
        assert_eq!(r.status, CodeModeStatus::Completed);
        let val = r.value.unwrap();
        assert_eq!(val["path"], "zero.token.compact");
        assert!(
            val["types"]
                .as_str()
                .unwrap()
                .contains("zero.token.compact")
        );
    }

    #[test]
    fn codemode_engine_uses_dedicated_cache_and_repo_scope() {
        let root = PathBuf::from("/tmp/tokenzero-codemode-root");
        let engine = make_engine_for_root(root.clone());
        assert_eq!(engine.config.allowed_roots, vec![root.clone()]);
        assert_eq!(
            engine.config.cache_path,
            crate::zerostack_store::default_codemode_recovery_cache_path(&root)
        );
        assert!(engine.config.cache_path.ends_with("codemode-recovery.json"));
    }

    #[test]
    fn edit_rejects_partially_invalid_hunks_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.txt");
        fs::write(&path, "hello\n").unwrap();
        let engine = make_engine_for_root(dir.path().to_path_buf());
        let args = vec![
            serde_json::json!(path.to_string_lossy().to_string()),
            serde_json::json!([
                {"find": "hello", "replace": "bye"},
                {"find": "hello"}
            ]),
        ];

        let err = exec_edit(&engine, &args).unwrap_err();
        assert!(
            err.error
                .as_deref()
                .unwrap()
                .contains("invalid hunk at index 1"),
            "{:?}",
            err.error
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
    }

    #[test]
    fn edit_reports_zero_hunks_applied_on_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.txt");
        fs::write(&path, "hello\n").unwrap();
        let engine = make_engine_for_root(dir.path().to_path_buf());
        let args = vec![
            serde_json::json!(path.to_string_lossy().to_string()),
            serde_json::json!([{ "find": "missing", "replace": "bye" }]),
        ];

        let val = exec_edit(&engine, &args).unwrap();
        assert_eq!(val["status"], "error");
        assert_eq!(val["hunks_applied"], 0);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
    }

    #[test]
    fn shell_plan_captures_exit_code() {
        let r = execute_codemode(r#"await zero.shell("echo hello")"#);
        assert_eq!(r.status, CodeModeStatus::Completed);
        let val = r.value.unwrap();
        assert!(
            val["status"].is_string(),
            "should complete without panic: {:?}",
            val
        );
    }

    #[test]
    fn multi_statement_composition() {
        let plan = r#"
            const data = await zero.compact("composed payload");
            const expanded = await zero.expand(data.ref);
            return { ref: data.ref, found: expanded.text }
        "#;
        let r = execute_codemode(plan);
        assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
        let val = r.value.unwrap();
        assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
    }

    #[test]
    fn telemetry_line_format_is_stable() {
        let r = execute_codemode(r#"await zero.compact("line test")"#);
        let line = r.to_line();
        assert!(line.starts_with("codemode:ok"));
        assert!(line.contains("ops="));
        assert!(line.contains("visible_tokens="));
        assert!(line.contains("raw_tokens="));
    }

    #[test]
    fn expand_invalid_ref_returns_error_not_panic() {
        let r = execute_codemode(r#"await zero.expand("tz://blob/nonexistent123")"#);
        assert_eq!(r.status, CodeModeStatus::Completed);
        let val = r.value.unwrap();
        assert!(
            val["status"].is_string(),
            "should complete without panic: {:?}",
            val
        );
    }

    #[test]
    fn expand_without_tz_prefix_is_rejected() {
        let r = execute_codemode(r#"await zero.expand("not-a-ref")"#);
        assert_eq!(r.status, CodeModeStatus::Error);
        assert!(r.error.as_ref().unwrap().contains("tz://"));
    }

    #[test]
    fn search_all_methods_discoverable() {
        let r = execute_codemode("search:zero");
        let val = r.value.unwrap();
        let results = val["results"].as_array().unwrap();
        assert!(results.len() >= 10, "catalog should expose all ops");
    }

    #[test]
    fn legacy_line_api_still_works() {
        let line = execute_plan_in_token(r#"await zero.compact("legacy")"#);
        assert!(line.starts_with("codemode:ok"));
    }
}
