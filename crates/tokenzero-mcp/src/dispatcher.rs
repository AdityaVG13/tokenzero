//! In-process typed domain dispatcher (tokenzero-irx9.2).
//!
//! One entry point for FastMCP, CodeMode single-op paths, CLI compatibility,
//! and the private raw worker. Adapters must not call each other, round-trip
//! through JSON-RPC, or re-implement auth/root/mutation/ref/telemetry
//! semantics. Transport framing stays at the edges; this module owns
//! surface-tagged domain execution and dispatcher-only profiling.

use crate::TokenZeroEngine;
use crate::tools::{self, DomainDispatchError};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokenzero_core::ToolResponse;
use tokenzero_core::operation_abi::{
    DomainError, DomainErrorKind, DomainResult, resolve_operation,
};

/// Which adapter invoked the shared domain dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DispatchSurface {
    Cli = 1,
    Mcp = 2,
    CodeMode = 3,
    RawWorker = 4,
}

impl DispatchSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Mcp => "mcp",
            Self::CodeMode => "codemode",
            Self::RawWorker => "raw_worker",
        }
    }
}

/// Result of one domain dispatch: transport-neutral domain envelope plus the
/// existing `ToolResponse` product object adapters already understand.
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub result: DomainResult,
    /// Full product response (visible capsule, refs, accounting, tool errors).
    pub tool_response: Option<ToolResponse>,
    /// When domain dispatch rejected before kernel work (unknown op / bad args).
    pub domain_error: Option<DomainError>,
    pub dispatcher_overhead_ns: u64,
    pub wall_ns: u64,
    pub kernel_ns: u64,
    pub surface: DispatchSurface,
    pub op: String,
}

impl DispatchOutcome {
    pub fn is_ok(&self) -> bool {
        self.domain_error.is_none()
            && self
                .tool_response
                .as_ref()
                .map(|r| r.status == "ok")
                .unwrap_or(false)
    }

    /// Map a tool-level error response into a DomainError when present.
    pub fn tool_domain_error(&self) -> Option<DomainError> {
        if let Some(err) = &self.domain_error {
            return Some(err.clone());
        }
        let resp = self.tool_response.as_ref()?;
        let cli = resp.error.as_ref()?;
        Some(map_tool_error_to_domain(&resp.tool, cli.code.as_str(), &cli.message))
    }
}

/// Last recorded dispatcher profile sample (benchmark subtraction).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchProfile {
    pub dispatcher_overhead_ns: u64,
    pub wall_ns: u64,
    pub kernel_ns: u64,
    /// 1=cli 2=mcp 3=codemode 4=raw_worker
    pub surface: u8,
}

static LAST_DISPATCH_OVERHEAD_NS: AtomicU64 = AtomicU64::new(0);
static LAST_DISPATCH_WALL_NS: AtomicU64 = AtomicU64::new(0);
static LAST_DISPATCH_KERNEL_NS: AtomicU64 = AtomicU64::new(0);
static LAST_DISPATCH_SURFACE: AtomicU64 = AtomicU64::new(0);
static DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);

fn record_profile(surface: DispatchSurface, overhead_ns: u64, wall_ns: u64, kernel_ns: u64) {
    LAST_DISPATCH_OVERHEAD_NS.store(overhead_ns, Ordering::Relaxed);
    LAST_DISPATCH_WALL_NS.store(wall_ns, Ordering::Relaxed);
    LAST_DISPATCH_KERNEL_NS.store(kernel_ns, Ordering::Relaxed);
    LAST_DISPATCH_SURFACE.store(surface as u64, Ordering::Relaxed);
    DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Profiling sample for the most recent dispatch (benchmark subtraction).
pub fn last_dispatch_profile() -> DispatchProfile {
    DispatchProfile {
        dispatcher_overhead_ns: LAST_DISPATCH_OVERHEAD_NS.load(Ordering::Relaxed),
        wall_ns: LAST_DISPATCH_WALL_NS.load(Ordering::Relaxed),
        kernel_ns: LAST_DISPATCH_KERNEL_NS.load(Ordering::Relaxed),
        surface: LAST_DISPATCH_SURFACE.load(Ordering::Relaxed) as u8,
    }
}

/// Number of domain dispatches since process start (tests / identity checks).
pub fn dispatch_count() -> u64 {
    DISPATCH_COUNT.load(Ordering::Relaxed)
}

/// Convert a product `ToolResponse` into the ABI `DomainResult` envelope.
pub fn tool_response_to_domain(response: &ToolResponse) -> DomainResult {
    let refs: Vec<String> = response.refs.iter().map(|r| r.ref_id.clone()).collect();
    let value = if response.status == "ok" {
        json!({
            "status": "ok",
            "visible": response.visible.as_ref().map(|v| &v.text),
            "accounting": response.accounting,
            "mode": response.mode,
            "content_type": response.content_type,
        })
    } else {
        json!({
            "status": response.status,
            "error": response.error,
        })
    };
    let mut domain = DomainResult::new(response.tool.clone(), value).with_refs(refs);
    if let Some(telem) = &response.telemetry {
        domain = domain.with_telemetry(telem.clone());
    }
    domain
}

fn map_tool_error_to_domain(op: &str, code: &str, message: &str) -> DomainError {
    let kind = match code {
        "path_not_allowed" | "path_outside_allowed_roots" => DomainErrorKind::Policy,
        "invalid_pattern" => DomainErrorKind::InvalidPattern,
        "invalid_ref" | "zeroref_malformed" => DomainErrorKind::InvalidRef,
        "invalid_url" => DomainErrorKind::InvalidUrl,
        "hunk_not_found" => DomainErrorKind::HunkNotFound,
        "ambiguous_hunk" => DomainErrorKind::AmbiguousHunk,
        "no_op_hunk" => DomainErrorKind::NoOpHunk,
        "not_found" => DomainErrorKind::NotFound,
        "unauthorized" => DomainErrorKind::Unauthorized,
        "cancelled" | "hard_max_wall_ms" => DomainErrorKind::Cancelled,
        "deadline_exceeded" => DomainErrorKind::DeadlineExceeded,
        "busy" => DomainErrorKind::Busy,
        "validation" | "invalid_argument" | "missing_argument" => DomainErrorKind::Validation,
        other if other.contains("policy") => DomainErrorKind::Policy,
        _ => DomainErrorKind::Runtime,
    };
    DomainError::new(kind, message.to_string()).with_op(op.to_string())
}

fn domain_dispatch_error_to_domain(err: DomainDispatchError) -> DomainError {
    match err {
        DomainDispatchError::UnknownTool(name) => {
            DomainError::new(DomainErrorKind::Validation, format!("unknown tool: {name}"))
                .with_op(name)
        }
        DomainDispatchError::InvalidArgs { op, message } => {
            DomainError::new(DomainErrorKind::Validation, message).with_op(op)
        }
        DomainDispatchError::TransportOnly(name) => DomainError::new(
            DomainErrorKind::Validation,
            format!("{name} is transport-control only; not a domain engine op"),
        )
        .with_op(name),
    }
}

/// Typed domain dispatch by canonical name / alias / CodeMode binding + JSON args.
pub fn dispatch_operation(
    engine: &TokenZeroEngine,
    surface: DispatchSurface,
    op_name: &str,
    args: &Value,
) -> DispatchOutcome {
    let wall_start = Instant::now();
    let resolved = resolve_operation(op_name)
        .map(|op| op.name)
        .unwrap_or(op_name);

    let pre_kernel = wall_start.elapsed().as_nanos() as u64;
    let kernel_start = Instant::now();
    let kernel = tools::execute_domain_op(engine, resolved, args);
    let kernel_ns = kernel_start.elapsed().as_nanos() as u64;
    let wall_ns = wall_start.elapsed().as_nanos() as u64;
    let overhead_ns = wall_ns.saturating_sub(kernel_ns).max(pre_kernel);

    match kernel {
        Ok(response) => {
            record_profile(surface, overhead_ns, wall_ns, kernel_ns);
            let result = tool_response_to_domain(&response);
            DispatchOutcome {
                result,
                tool_response: Some(response),
                domain_error: None,
                dispatcher_overhead_ns: overhead_ns,
                wall_ns,
                kernel_ns,
                surface,
                op: resolved.to_string(),
            }
        }
        Err(err) => {
            record_profile(surface, overhead_ns, wall_ns, kernel_ns);
            let domain_error = domain_dispatch_error_to_domain(err);
            let op = domain_error.op.clone().unwrap_or_else(|| resolved.to_string());
            DispatchOutcome {
                result: DomainResult::new(
                    op.clone(),
                    json!({
                        "status": "error",
                        "error": {
                            "kind": domain_error.kind.as_str(),
                            "message": domain_error.message,
                            "retryable": domain_error.retryable,
                        }
                    }),
                ),
                tool_response: None,
                domain_error: Some(domain_error),
                dispatcher_overhead_ns: overhead_ns,
                wall_ns,
                kernel_ns,
                surface,
                op,
            }
        }
    }
}

/// Private raw worker entry: typed op id + args, no transport framing.
pub fn dispatch_raw_worker(
    engine: &TokenZeroEngine,
    op_name: &str,
    args: &Value,
) -> DispatchOutcome {
    dispatch_operation(engine, DispatchSurface::RawWorker, op_name, args)
}

/// MCP / FastMCP tool name + args → domain dispatch (no MCP framing).
pub fn dispatch_mcp_tool(
    engine: &TokenZeroEngine,
    name: &str,
    args: &Value,
) -> Result<DispatchOutcome, DomainError> {
    // Transport-only CodeMode control tools are not domain engine ops.
    let bare = name.strip_prefix("tz_").unwrap_or(name);
    if matches!(
        bare,
        "execute_code" | "codemode_search" | "codemode_describe"
    ) {
        return Err(DomainError::new(
            DomainErrorKind::Validation,
            format!("{name} is a transport control tool, not a domain dispatch target"),
        )
        .with_op(name));
    }
    Ok(dispatch_operation(
        engine,
        DispatchSurface::Mcp,
        name,
        args,
    ))
}

/// CodeMode method path → domain dispatch (no sandbox / plan runtime).
pub fn dispatch_codemode_method(
    engine: &TokenZeroEngine,
    method: &str,
    args: &Value,
) -> Result<DispatchOutcome, DomainError> {
    let op = resolve_operation(method).ok_or_else(|| {
        DomainError::new(
            DomainErrorKind::Validation,
            format!("unknown codemode method for domain dispatch: {method}"),
        )
        .with_op(method)
    })?;
    // CodeMode-only control methods stay in the plan executor.
    if matches!(
        op.migration,
        tokenzero_core::operation_abi::MigrationStatus::CodemodeControl
    ) || op.exposure.resource_uri.is_some()
        || op.name.starts_with("resource.")
        || op.name.starts_with("codemode.journal")
        || op.name == "codemode.limits"
        || op.name == "tz_execute_code"
        || op.name == "tz_codemode_search"
        || op.name == "tz_codemode_describe"
    {
        return Err(DomainError::new(
            DomainErrorKind::Validation,
            format!("{} is not a domain engine op", op.name),
        )
        .with_op(op.name));
    }
    Ok(dispatch_operation(
        engine,
        DispatchSurface::CodeMode,
        op.name,
        args,
    ))
}

/// CLI compatibility path through the shared dispatcher.
pub fn dispatch_cli(
    engine: &TokenZeroEngine,
    op_name: &str,
    args: &Value,
) -> DispatchOutcome {
    dispatch_operation(engine, DispatchSurface::Cli, op_name, args)
}

/// Whether `op_name` is a domain engine operation (not transport control).
pub fn is_domain_operation(op_name: &str) -> bool {
    let Some(op) = resolve_operation(op_name) else {
        return false;
    };
    if op.exposure.resource_uri.is_some() {
        return false;
    }
    if matches!(
        op.migration,
        tokenzero_core::operation_abi::MigrationStatus::CodemodeControl
    ) {
        return false;
    }
    !matches!(
        op.name,
        "tz_execute_code"
            | "tz_codemode_search"
            | "tz_codemode_describe"
            | "codemode.limits"
            | "codemode.journalDoctor"
            | "codemode.journalInspect"
            | "codemode.journalResume"
            | "codemode.journalRollback"
    ) && !op.name.starts_with("resource.")
        && !op.name.starts_with("zero.pipe")
        && !op.name.starts_with("zero.pick")
        && !op.name.starts_with("zero.filter")
        && !op.name.starts_with("zero.count")
        && !op.name.starts_with("zero.first")
        && !op.name.starts_with("zero.verdict")
        && !op.name.starts_with("zero.raw")
        && !op.name.starts_with("zero.assert")
        && !op.name.starts_with("zero.token.compact")
        && !op.name.starts_with("zero.token.expandMany")
        && !op.name.starts_with("zero.token.dedupe")
        && !op.name.starts_with("zero.compact_max")
        && op.name != "zero.count_tokens"
}

/// Canonical FastMCP domain ops that participate in cross-surface identity tests.
pub fn domain_fastmcp_ops() -> Vec<&'static str> {
    tokenzero_core::operation_abi::all_operations()
        .iter()
        .filter(|op| op.exposure.fastmcp_tool && is_domain_operation(op.name))
        .map(|op| op.name)
        .collect()
}
