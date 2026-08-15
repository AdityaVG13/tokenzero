//! Profiling-only measurement hooks (Pass 6 INSTRUMENT).
//!
//! **Not a product feature.** Enable with `TOKENZERO_PERF_PROFILE=1` (also
//! `true` / `yes` / `on`). When the flag is off, hot paths take a single
//! atomic load and no allocations, timers, or stderr I/O.
//!
//! When on, emits one JSON object per line on **stderr** (never stdout, so
//! CLI JSON tools stay machine-parseable):
//!
//! - `perf.profile.run_start` — once per process
//! - `perf.profile.sample_collected` — per domain dispatch (wall/kernel/overhead)
//! - `perf.profile.span_summary` — per instrumented stage
//!
//! Flamegraph sentinels (`_profile_*_on`) are `#[inline(never)]` and only enter
//! when the flag is on, so release baselines stay free of extra frames.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::Instant;

/// Env flag: set to `1` / `true` / `yes` / `on` to enable structured profile events.
pub const PERF_PROFILE_ENV: &str = "TOKENZERO_PERF_PROFILE";

// 0 = unset, 1 = off, 2 = on
static ENABLED: AtomicU8 = AtomicU8::new(0);
static RUN_START_EMITTED: AtomicBool = AtomicBool::new(false);
static HOT_EXPAND: AtomicU64 = AtomicU64::new(0);
static HOT_READ: AtomicU64 = AtomicU64::new(0);
static HOT_CAPSULE: AtomicU64 = AtomicU64::new(0);

/// Process-wide enter counts for expand/read/capsule hot paths (PERF-H-001/004).
/// Cheap atomics; independent of `TOKENZERO_PERF_PROFILE` stderr spans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HotPathProfileSnapshot {
    pub expand: u64,
    pub read: u64,
    pub capsule: u64,
}

pub fn hot_path_snapshot() -> HotPathProfileSnapshot {
    HotPathProfileSnapshot {
        expand: HOT_EXPAND.load(Ordering::Relaxed),
        read: HOT_READ.load(Ordering::Relaxed),
        capsule: HOT_CAPSULE.load(Ordering::Relaxed),
    }
}

pub fn note_hot_path_expand() {
    HOT_EXPAND.fetch_add(1, Ordering::Relaxed);
}

pub fn note_hot_path_read() {
    HOT_READ.fetch_add(1, Ordering::Relaxed);
}

pub fn note_hot_path_capsule() {
    HOT_CAPSULE.fetch_add(1, Ordering::Relaxed);
}

pub fn note_dispatch_hot_path(op: &str) {
    match op {
        "tz_expand" | "expand" => note_hot_path_expand(),
        "tz_read" | "read" => note_hot_path_read(),
        _ => {}
    }
}

#[inline]
fn parse_enabled(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Cached check of [`PERF_PROFILE_ENV`]. Safe to call on every hot path.
#[inline]
pub fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        2 => true,
        1 => false,
        _ => {
            let on = std::env::var(PERF_PROFILE_ENV)
                .ok()
                .as_deref()
                .map(parse_enabled)
                .unwrap_or(false);
            ENABLED.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

fn emit_line(payload: &str) {
    let mut err = io::stderr().lock();
    let _ = writeln!(err, "{payload}");
    let _ = err.flush();
}

fn note_run_start() {
    if RUN_START_EMITTED.swap(true, Ordering::Relaxed) {
        return;
    }
    emit_line(&format!(
        r#"{{"event":"perf.profile.run_start","pid":{},"flag":"{PERF_PROFILE_ENV}","note":"profiling-only; stderr JSON lines"}}"#,
        std::process::id()
    ));
}

/// Emit a per-dispatch sample (wall / kernel / dispatcher overhead).
pub fn sample_collected(
    op: &str,
    surface: &str,
    wall_ns: u64,
    kernel_ns: u64,
    overhead_ns: u64,
    ok: bool,
) {
    if !enabled() {
        return;
    }
    note_run_start();
    // Manual JSON: avoid allocating a serde Value on the measurement path.
    emit_line(&format!(
        r#"{{"event":"perf.profile.sample_collected","op":"{}","surface":"{}","wall_us":{},"kernel_us":{},"overhead_us":{},"ok":{},"evidence":"dispatcher Instant wall/kernel"}}"#,
        json_escape(op),
        json_escape(surface),
        wall_ns / 1000,
        kernel_ns / 1000,
        overhead_ns / 1000,
        ok
    ));
}

#[inline]
fn timed_span<R>(span: &'static str, category: &'static str, f: impl FnOnce() -> R) -> R {
    note_run_start();
    let t0 = Instant::now();
    let out = f();
    let us = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);
    emit_line(&format!(
        r#"{{"event":"perf.profile.span_summary","span":"{span}","category":"{category}","cumulative_us":{us},"count":1,"p50_us":{us},"p95_us":{us},"evidence":"wall-clock Instant when {PERF_PROFILE_ENV}=1"}}"#
    ));
    out
}

// --- Stage entry points: flag-off = direct call; flag-on = never-inline label + span ---

/// S4 expand: recovery store resolve / reload-on-miss.
#[inline]
pub fn _profile_expand_resolve<R>(f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    _profile_expand_resolve_on(f)
}

#[inline(never)]
fn _profile_expand_resolve_on<R>(f: impl FnOnce() -> R) -> R {
    timed_span("expand.resolve_slice", "expand", f)
}

/// S4 expand: secret masking of visible body.
#[inline]
pub fn _profile_expand_mask<R>(f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    _profile_expand_mask_on(f)
}

#[inline(never)]
fn _profile_expand_mask_on<R>(f: impl FnOnce() -> R) -> R {
    timed_span("expand.mask_secrets", "expand", f)
}

/// S4 expand: session seen-set apply (+ optional disk persist).
#[inline]
pub fn _profile_expand_session_apply<R>(f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    _profile_expand_session_apply_on(f)
}

#[inline(never)]
fn _profile_expand_session_apply_on<R>(f: impl FnOnce() -> R) -> R {
    timed_span("expand.session_apply", "expand", f)
}

/// S2 read: full `read_with_options_inner` body.
#[inline]
pub fn _profile_read_inner<R>(f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    _profile_read_inner_on(f)
}

#[inline(never)]
fn _profile_read_inner_on<R>(f: impl FnOnce() -> R) -> R {
    timed_span("read.inner", "read", f)
}

/// S3 find: `search` body via `find_with_options`.
#[inline]
pub fn _profile_find_search<R>(f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    _profile_find_search_on(f)
}

#[inline(never)]
fn _profile_find_search_on<R>(f: impl FnOnce() -> R) -> R {
    timed_span("find.search", "find", f)
}

/// Session memory disk journal write path.
#[inline]
pub fn _profile_session_persist<R>(f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    _profile_session_persist_on(f)
}

#[inline(never)]
fn _profile_session_persist_on<R>(f: impl FnOnce() -> R) -> R {
    timed_span("session.persist", "persist", f)
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "../../../tests/engine/inline/perf_profile__tests.rs"]
mod tests;
