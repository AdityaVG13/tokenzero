#![forbid(unsafe_code)]

use fs4::{FileExt, TryLockError};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokenzero_core::{ContentType, count_tokens, error_block, id_for, sha256_hex, symbol_block};

use crate::shared_cas::{SharedCas, SharedCasError};
use crate::telemetry::CrossEngineTelemetry;

pub mod telemetry;

pub mod boot;
pub mod context_view;
pub mod dst;
pub mod embedded_store;
pub mod entity_novelty;
pub mod migration;
pub mod prefix_stability;
pub mod segment_store;
#[cfg(test)]
mod segment_store_tests;
pub mod session_aliases;
pub mod shared_cas;
pub mod transparency;
pub use entity_novelty::{
    ENTITY_NOVELTY_RECORD_TYPE, ENTITY_NOVELTY_REL_DIR, ENTITY_NOVELTY_SCHEMA_VERSION,
    EntityNoveltyRecord, NoveltyError, entity_novelty_path, merge_entity_novelty, parse_entity_ref,
    read_entity_novelty, scope_digest, write_entity_novelty,
};

pub mod working_set;

pub use session_aliases::{
    SESSION_ALIAS_HEX_LEN, canonical_full_blob_ref, is_full_hash_blob_bare, is_session_alias_bare,
    is_session_ordinal_bare, parse_session_ordinal_bare, rewrite_full_hash_blob_refs_in_text,
    rewrite_full_hash_blob_refs_in_value, session_ordinal_ref, session_visible_blob_alias,
    split_ref_fragment,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DurableCommitFailPoint {
    BeforePersist,
    BeforeFileSync,
    BeforeDirectorySync,
}

#[cfg(test)]
thread_local! {
    static DURABLE_COMMIT_FAIL_POINT: std::cell::Cell<Option<DurableCommitFailPoint>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn fail_durable_commit_at(point: DurableCommitFailPoint) -> Result<(), RecoveryError> {
    if DURABLE_COMMIT_FAIL_POINT.with(|configured| configured.get() == Some(point)) {
        return Err(io::Error::other("durable commit fault injected").into());
    }
    Ok(())
}

const LOCK_RETRIES: usize = 240;
const MAX_SHELL_OUTCOMES: usize = 256;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const TMP_RETRIES: usize = 16;
const REF_INDEX_MAX_BYTES: u64 = 1_048_576;
const REF_INDEX_DISABLE_ENV: &str = "TOKENZERO_REF_INDEX";
#[cfg(not(test))]
const REF_INDEX_PATH_ENV: &str = "TOKENZERO_REF_INDEX_PATH";
const JOURNAL_MAX_SEALED_SEGMENTS: usize = 4;

/// ZeroStack schemes accepted by expand. Full-hash portable blob refs first use the
/// configured canonical shared CAS. Legacy short blob refs retain a clearly separated
/// same-store alias tier when no shared object is available.
pub const EXPAND_REF_SCHEMES: &[&str] = &["tz://", "fz://", "gz://"];
const BLOB_REF_PREFIXES: &[(&str, &str)] = &[
    ("tz", "tz://blob/"),
    ("fz", "fz://blob/"),
    ("gz", "gz://blob/"),
];
fn blob_ref_scheme_hash(bare: &str) -> Option<(&str, &str)> {
    BLOB_REF_PREFIXES
        .iter()
        .find_map(|(scheme, prefix)| bare.strip_prefix(prefix).map(|hash| (*scheme, hash)))
}
fn blob_ref_hash(bare: &str) -> Option<&str> {
    blob_ref_scheme_hash(bare).map(|(_, hash)| hash)
}
fn is_foreign_blob_ref(ref_id: &str) -> bool {
    ref_id.starts_with("fz://blob/") || ref_id.starts_with("gz://blob/")
}
fn is_foreign_non_blob_ref(ref_id: &str) -> bool {
    (ref_id.starts_with("fz://") || ref_id.starts_with("gz://")) && blob_ref_hash(ref_id).is_none()
}

/// True when `ref_id` starts with a scheme expand can recover (`tz://`, `fz://`, `gz://`).
pub fn is_expandable_ref(ref_id: &str) -> bool {
    EXPAND_REF_SCHEMES
        .iter()
        .any(|scheme| ref_id.starts_with(scheme))
}

/// Rewrite portable `fz://blob/` / `gz://blob/` refs to `tz://blob/` for the
/// legacy same-store alias tier. Foreign non-blob refs remain engine-owned and
/// are rejected instead of being reinterpreted as TokenZero keys.
pub fn canonicalize_expand_ref(ref_id: &str) -> Option<String> {
    if ref_id.starts_with("tz://") {
        return Some(ref_id.to_string());
    }
    blob_ref_hash(ref_id).map(|hash| format!("tz://blob/{hash}"))
}

fn is_legacy_same_store_blob_ref(ref_id: &str) -> bool {
    let bare = ref_id.split_once('#').map_or(ref_id, |(bare, _)| bare);
    blob_ref_hash(bare).is_some_and(|hash| {
        hash.len() == 17
            && hash.starts_with('b')
            && hash[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Lazily compiled `file:line[:col]` matcher for search ingestion.
static SEARCH_PATH_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<path>(?:[\w.@+-]+/)+[\w.@+-]+):(?P<line>\d+):?(?P<col>\d+)?")
        .expect("SEARCH_PATH_LINE is a valid compile-time regex literal")
});
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

macro_rules! recovery_maps {
    (contains $s:expr, $id:expr) => {
        $s.blobs.contains_key($id)
            || $s.files.contains_key($id)
            || $s.units.contains_key($id)
            || $s.search_hits.contains_key($id)
    };
    (remove $s:expr, $id:expr) => {{
        $s.blobs.remove($id);
        $s.files.remove($id);
        $s.units.remove($id);
        $s.search_hits.remove($id);
    }};
    (keys $s:expr) => {
        $s.blobs
            .keys()
            .chain($s.files.keys())
            .chain($s.units.keys())
            .chain($s.search_hits.keys())
    };
    (copy $d:expr, $s:expr, $id:expr) => {{
        copy_map_entry(&mut $d.blobs, &$s.blobs, $id);
        copy_map_entry(&mut $d.files, &$s.files, $id);
        copy_map_entry(&mut $d.units, &$s.units, $id);
        copy_map_entry(&mut $d.search_hits, &$s.search_hits, $id);
    }};
    (merge $session:expr, $m:expr, $c:expr) => {{
        merge_map_entries($session, &mut $m.blobs, $c.blobs);
        merge_map_entries($session, &mut $m.files, $c.files);
        merge_map_entries($session, &mut $m.units, $c.units);
        merge_map_entries($session, &mut $m.search_hits, $c.search_hits);
    }};
    (evict $slf:expr) => {{
        evict_prefix(
            &mut $slf.state.blobs,
            &mut $slf.state.order,
            "tz://blob/",
            $slf.config.max_blobs,
        );
        evict_prefix(
            &mut $slf.state.files,
            &mut $slf.state.order,
            "tz://file/",
            $slf.config.max_files,
        );
        evict_prefix(
            &mut $slf.state.units,
            &mut $slf.state.order,
            "tz://unit/",
            $slf.config.max_units,
        );
        evict_prefix(
            &mut $slf.state.search_hits,
            &mut $slf.state.order,
            "tz://search/",
            $slf.config.max_search_hits,
        );
    }};
}

macro_rules! persist_after_deferred {
    ($name:ident, $deferred:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty) => {
        pub fn $name(&mut self, $($arg : $ty),*) -> Result<$ret, RecoveryError> {
            let value = self.$deferred($($arg),*);
            self.persist_value(value)
        }
    };
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

macro_rules! labeled_errors {
    ($name:ident { $($var:ident => $label:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $(
                #[error($label)]
                $var,
            )+
        }
    };
}

labeled_errors! { ZeroRefError {
    Malformed => "malformed", Unsupported => "unsupported", Missing => "missing", Io => "io",
    Corruption => "corruption", Policy => "policy", IncompatibleVersion => "incompatible_version",
    LegacyAmbiguity => "legacy_ambiguity",
}}

labeled_errors! { FragmentError {
    Malformed => "malformed", Reversed => "reversed", OutOfRange => "out_of_range",
    NonUtf8Line => "non_utf8_line_fragment", UnknownKind => "unknown_kind",
    DuplicateFragment => "duplicate_fragment",
}}

// Parsed byte or line fragment.
enum FragmentSpec {
    /// Zero-based half-open byte range `start..end`.
    Byte { start: usize, end: usize },
    /// One-based inclusive line range `start..=end`.
    Line { start: usize, end: usize },
}

// Parse validated byte and line fragment bounds.
fn parse_fragment_bounds_core(
    value: &str,
    repeated_kind: char,
    allow_single: bool,
    require_nonzero_start: bool,
) -> Result<(usize, usize), bool> {
    if value.starts_with(repeated_kind) {
        return Err(false);
    }
    let separated = value.split_once(',').or_else(|| value.split_once('-'));
    let (start, end) = match separated {
        Some((start, end)) => (start, end),
        None if allow_single => (value, value),
        None => return Err(false),
    };
    let start = start
        .trim_start_matches(repeated_kind)
        .parse::<usize>()
        .map_err(|_| false)?;
    let end = end
        .trim_start_matches(repeated_kind)
        .parse::<usize>()
        .map_err(|_| false)?;
    if require_nonzero_start && start == 0 {
        return Err(false);
    }
    if start > end {
        return Err(true);
    }
    Ok((start, end))
}

fn parse_fragment_spec(fragment: &str) -> Result<FragmentSpec, FragmentError> {
    if fragment.is_empty() {
        return Err(FragmentError::Malformed);
    }
    if fragment.contains('#') {
        return Err(FragmentError::DuplicateFragment);
    }
    let kind = fragment.as_bytes()[0] as char;
    let map_err = |reversed| {
        if reversed {
            FragmentError::Reversed
        } else {
            FragmentError::Malformed
        }
    };
    let (start, end) =
        parse_fragment_bounds_core(&fragment[1..], kind, true, kind == 'L').map_err(map_err)?;
    match kind {
        'B' => Ok(FragmentSpec::Byte { start, end }),
        'L' => Ok(FragmentSpec::Line { start, end }),
        _ => Err(FragmentError::UnknownKind),
    }
}

/// Stable reason string for a [`FragmentError`] used in `ExpansionResult::reason`.
fn shared_cas_error_reason(err: SharedCasError) -> &'static str {
    match err {
        SharedCasError::Corruption => "shared-cas-corruption",
        SharedCasError::Policy => "shared-cas-policy",
        SharedCasError::Io(_) => "shared-cas-io",
        SharedCasError::InvalidHash(_) => "zeroref-malformed",
        SharedCasError::NotFound => "shared-cas-missing",
    }
}

/// `Err(None)` = non-UTF8 object bytes; `Err(Some(_))` = CAS error.
fn shared_cas_utf8(cas: &SharedCas, hash: &str) -> Result<String, Option<SharedCasError>> {
    match cas.resolve(hash) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| None),
        Err(err) => Err(Some(err)),
    }
}

fn fragment_error_reason(err: FragmentError) -> &'static str {
    match err {
        FragmentError::Malformed => "fragment-malformed",
        FragmentError::Reversed => "fragment-reversed",
        FragmentError::OutOfRange => "fragment-out-of-range",
        FragmentError::NonUtf8Line => "non_utf8_line_fragment",
        FragmentError::UnknownKind => "fragment-unknown-kind",
        FragmentError::DuplicateFragment => "fragment-duplicate",
    }
}

/// Parsed components of a ZeroRef v1 portable blob ref.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZeroRefV1Blob {
    pub scheme: String,
    pub hash: String,
    pub fragment: Option<ZeroRefFragment>,
}

/// Fragment selector for a ZeroRef v1 blob ref.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZeroRefFragment {
    /// Zero-based half-open byte range `start..end`. `start == end` is allowed.
    Byte { start: usize, end: usize },
    /// One-based inclusive line range `start..=end`. Exact newline retention.
    Line { start: usize, end: usize },
}

/// Parse a portable `(tz|fz|gz)://blob/<sha256>` ZeroRef v1 reference.
/// Only full lowercase SHA-256 identities are accepted. `#Bstart-end` is a
/// zero-based half-open byte range; `#Lstart-end` is a one-based inclusive
/// line range. Legacy short IDs return [`ZeroRefError::LegacyAmbiguity`].
pub fn parse_zeroref_v1_blob(
    ref_id: &str,
    byte_length: Option<usize>,
) -> Result<ZeroRefV1Blob, ZeroRefError> {
    let (bare, fragment) = ref_id
        .split_once('#')
        .map_or((ref_id, None), |(bare, fragment)| (bare, Some(fragment)));
    let (scheme, hash) = blob_ref_scheme_hash(bare).ok_or(ZeroRefError::Unsupported)?;
    if hash.is_empty() || hash.contains('/') {
        return Err(ZeroRefError::Malformed);
    }
    if hash.len() != 64 {
        return Err(ZeroRefError::LegacyAmbiguity);
    }
    if hash
        .bytes()
        .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(ZeroRefError::Malformed);
    }
    let fragment = fragment
        .map(|fragment| {
            let (kind, value) = match fragment.as_bytes().first() {
                Some(&b'B') => ('B', &fragment[1..]),
                Some(&b'L') => ('L', &fragment[1..]),
                _ => return Err(ZeroRefError::Malformed),
            };
            let (start, end) = parse_fragment_bounds_core(value, kind, kind == 'L', kind == 'L')
                .map_err(|_| ZeroRefError::Malformed)?;
            match kind {
                'B' if byte_length.is_none_or(|len| end <= len) => {
                    Ok(ZeroRefFragment::Byte { start, end })
                }
                'L' => Ok(ZeroRefFragment::Line { start, end }),
                _ => Err(ZeroRefError::Malformed),
            }
        })
        .transpose()?;
    Ok(ZeroRefV1Blob {
        scheme: scheme.to_string(),
        hash: hash.to_string(),
        fragment,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    pub max_blobs: usize,
    pub max_files: usize,
    pub max_units: usize,
    pub max_search_hits: usize,
    pub max_bytes: usize,
    pub max_load_bytes: usize,
    /// When true, legacy short-ref lookups resolve through the alias tier.
    /// When false, legacy short refs fail with a typed "legacy-ref-disabled" reason.
    #[serde(default = "default_legacy_compat")]
    pub legacy_compat: bool,
    /// Optional Unix timestamp after which legacy compatibility may be removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_compat_deadline: Option<u64>,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_blobs: 128,
            max_files: 256,
            max_units: 2048,
            max_search_hits: 1024,
            max_bytes: 8_000_000,
            max_load_bytes: 16_000_000,
            legacy_compat: true,
            legacy_compat_deadline: None,
        }
    }
}

fn default_legacy_compat() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFile {
    pub ref_id: String,
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_identity: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub source_backed: bool,
    pub text: String,
    pub content_type: String,
    pub source_fingerprint: Option<SourceFingerprint>,
    pub source_start_line: Option<usize>,
    pub source_end_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredUnit {
    pub ref_id: String,
    pub text: String,
    pub content_type: String,
    pub source_ref: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub size: u64,
    pub mtime_ns: u128,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPayload {
    pub blob_ref: String,
    pub file_ref: String,
    pub unit_refs: Vec<String>,
    pub raw_tokens: usize,
    pub source_start_line: Option<usize>,
    pub source_end_line: Option<usize>,
}

#[derive(Debug, Clone)]
struct PayloadMemo {
    text: String,
    content_type: ContentType,
    path: Option<PathBuf>,
    source_start_line: Option<usize>,
    source_end_line: Option<usize>,
    source_backed: bool,
    stored: StoredPayload,
}

impl PayloadMemo {
    fn matches(
        &self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
        source_backed: bool,
    ) -> bool {
        self.text == text
            && self.content_type == content_type
            && self.path.as_deref() == path
            && self.source_start_line == source_start_line
            && self.source_end_line == source_end_line
            && self.source_backed == source_backed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpansionResult {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub selector: Option<String>,
    pub content: String,
    pub tokens: usize,
    pub found: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clamped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned_start_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned_end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<usize>,
}

impl ExpansionResult {
    pub fn ok(ref_id: String, selector: Option<String>, content: String) -> Self {
        let tokens = count_tokens(&content);
        Self {
            ref_id,
            selector,
            content,
            tokens,
            found: true,
            reason: "ok".to_string(),
            clamped: false,
            returned_start_line: None,
            returned_end_line: None,
            line_count: None,
        }
    }

    pub fn missing(ref_id: String, selector: Option<String>, reason: impl Into<String>) -> Self {
        Self {
            ref_id,
            selector,
            content: String::new(),
            tokens: 0,
            found: false,
            reason: reason.into(),
            clamped: false,
            returned_start_line: None,
            returned_end_line: None,
            line_count: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BlobEntry {
    /// Full text stored directly in recovery state. Legacy string-valued caches
    /// deserialize into this variant and serialize back to the same JSON shape.
    Inline(String),
    /// Pointer to an exact, one-based inclusive source line range.
    FileRef {
        path: PathBuf,
        source_start_line: usize,
        source_end_line: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryState {
    pub version: u32,
    pub max_blobs: usize,
    pub max_files: usize,
    pub max_units: usize,
    pub max_search_hits: usize,
    pub max_bytes: usize,
    pub blobs: BTreeMap<String, BlobEntry>,
    pub files: BTreeMap<String, StoredFile>,
    pub units: BTreeMap<String, StoredUnit>,
    pub search_hits: BTreeMap<String, StoredUnit>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(default = "initial_ordinal_generation")]
    pub ordinal_generation: u64,
    #[serde(default = "initial_next_ordinal")]
    pub next_ordinal: u64,
    pub order: Vec<String>,
    #[serde(default)]
    pub shell_outcomes: BTreeMap<String, ShellOutcome>,
    #[serde(default)]
    pub shell_outcome_seq: u64,
    /// Short refs whose 16-hex prefix maps to multiple distinct full hashes.
    #[serde(default)]
    pub ambiguous_aliases: BTreeSet<String>,
    /// Append-only audit commitment for acknowledged mint and alias-CAS mutations.
    #[serde(default)]
    pub transparency: crate::transparency::MmrLog,
}

// Capped shell-result index; blob payloads remain content-addressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ShellOutcome {
    pub combined_sha: String,
    pub exit_code: Option<i32>,
    pub seen: u32,
    pub seq: u64,
}

/// Repeat verdict for the command just recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRepeat {
    pub unchanged: bool,
    pub seen: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrdinalRange {
    pub generation: u64,
    pub start: u64,
    pub end_exclusive: u64,
}

impl OrdinalRange {
    pub fn len(self) -> u64 {
        self.end_exclusive.saturating_sub(self.start)
    }
    pub fn is_empty(self) -> bool {
        self.start == self.end_exclusive
    }
    pub fn ref_for(self, offset: u64) -> Option<String> {
        (offset < self.len()).then(|| session_ordinal_ref(self.generation, self.start + offset))
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "PascalCase")]
pub enum ContentClass {
    SourceFile,
    Diff,
    ShellOutput,
    SearchHits,
    Doc,
    BinaryPreview,
    #[default]
    Unknown,
}

// Infer the ref-index content class.
fn classify_ref(ref_id: &str, content_type: Option<ContentType>) -> ContentClass {
    let Some(parsed) = parse_ref(ref_id) else {
        return ContentClass::Unknown;
    };
    match parsed.kind {
        "file" => ContentClass::SourceFile,
        "search" => ContentClass::SearchHits,
        "unit" => match content_type {
            Some(ContentType::Diff) => ContentClass::Diff,
            Some(ContentType::ShellOutput) => ContentClass::ShellOutput,
            _ => ContentClass::Unknown,
        },
        "blob" => match content_type {
            Some(ContentType::Code) => ContentClass::SourceFile,
            Some(ContentType::Diff) => ContentClass::Diff,
            Some(ContentType::ShellOutput) => ContentClass::ShellOutput,
            Some(ContentType::Markdown)
            | Some(ContentType::Logs)
            | Some(ContentType::Tree)
            | Some(ContentType::JsonConfig) => ContentClass::Doc,
            Some(ContentType::SearchResult) => ContentClass::SearchHits,
            _ => ContentClass::BinaryPreview,
        },
        _ => ContentClass::Unknown,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefIndexEntry {
    ref_id: String,
    store_path: String,
    ts: u128,
    #[serde(default)]
    content_class: ContentClass,
    #[serde(default)]
    expanded: bool,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    expansion_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_expanded_ts: Option<u128>,
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

#[cfg(test)]
thread_local! {
    static REF_INDEX_TEST_OVERRIDE: std::cell::RefCell<Option<(bool, PathBuf)>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_ref_index_test_override(value: Option<(bool, PathBuf)>) -> Option<(bool, PathBuf)> {
    REF_INDEX_TEST_OVERRIDE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), value))
}

#[cfg(test)]
fn ref_index_test_override() -> Option<(bool, PathBuf)> {
    REF_INDEX_TEST_OVERRIDE.with(|slot| slot.borrow().clone())
}

const fn initial_ordinal_generation() -> u64 {
    1
}
const fn initial_next_ordinal() -> u64 {
    1
}

impl RecoveryState {
    fn empty(config: &RecoveryConfig) -> Self {
        Self {
            version: 1,
            max_blobs: config.max_blobs,
            max_files: config.max_files,
            max_units: config.max_units,
            max_search_hits: config.max_search_hits,
            max_bytes: config.max_bytes,
            blobs: BTreeMap::new(),
            files: BTreeMap::new(),
            units: BTreeMap::new(),
            search_hits: BTreeMap::new(),
            aliases: BTreeMap::new(),
            ordinal_generation: initial_ordinal_generation(),
            next_ordinal: initial_next_ordinal(),
            order: Vec::new(),
            shell_outcomes: BTreeMap::new(),
            shell_outcome_seq: 0,
            ambiguous_aliases: BTreeSet::new(),
            transparency: crate::transparency::MmrLog::default(),
        }
    }

    fn configure(&mut self, config: &RecoveryConfig) {
        self.max_blobs = config.max_blobs;
        self.max_files = config.max_files;
        self.max_units = config.max_units;
        self.max_search_hits = config.max_search_hits;
        self.max_bytes = config.max_bytes;
    }
}

#[derive(Debug)]
pub struct RecoveryStore {
    pub config: RecoveryConfig,
    pub persistence_path: Option<PathBuf>,
    state: RecoveryState,
    session_refs: Vec<String>,
    /// Transient mapping from ref id to the content class inferred at store time.
    /// Used only to seed ref-index entries with a class before the state is persisted;
    /// it is not itself persisted and re-derives from `classify_ref` when absent.
    ref_classes: BTreeMap<String, ContentClass>,
    /// Identity of the cache file as last written by this store, captured
    /// while still holding the persist lock. `None` until the first persist;
    /// also reset to `None` whenever a write fails, so the next persist must
    /// take the full reload+merge path.
    disk_identity: Option<DiskIdentity>,
    /// Identity of the journal sibling at our last write (`None` = we left no
    /// journal). Checked together with `disk_identity`: a foreign append to
    /// the journal must force the reload+merge path just like a foreign
    /// snapshot rewrite.
    journal_identity: Option<DiskIdentity>,
    /// Canonical immutable store shared with FSZero/GraphZero. Attached only
    /// for unified `<store-root>/tokenzero/...` cache paths whose `blobs/`
    /// directory already exists.
    shared_cas: Option<SharedCas>,
    pub recovery_count: usize,
    pub recovery_tokens: usize,
    /// Count of legacy short-ref lookups resolved via alias this session.
    pub legacy_read_count: usize,
    pub telemetry: CrossEngineTelemetry,
    /// Transient set of blob refs pending deletion. Applied by persist() and
    /// cleared only after successful authoritative snapshot write.
    pending_blob_deletions: BTreeSet<String>,
    /// Transient set of alias short refs pending deletion. Applied by
    /// persist() and cleared only after successful authoritative snapshot write.
    pending_alias_deletions: BTreeSet<String>,
    /// Last exact payload admitted by this engine. One entry bounds retained
    /// memory while covering the repeated MCP read/find hot path.
    payload_memo: Option<PayloadMemo>,
    /// A memo hit can make the immediately following persist provably empty.
    /// Any real ref mutation clears this flag through `remember_ref`.
    skip_empty_persist: bool,
}

// Snapshot identity used to detect foreign atomic replacements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiskIdentity {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl DiskIdentity {
    fn capture(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let modified = meta.modified().ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(Self {
                len: meta.len(),
                modified,
                dev: meta.dev(),
                ino: meta.ino(),
            })
        }
        #[cfg(not(unix))]
        Some(Self {
            len: meta.len(),
            modified,
        })
    }
}

fn cache_identities(path: &Path) -> (Option<DiskIdentity>, Option<DiskIdentity>) {
    (
        DiskIdentity::capture(path),
        DiskIdentity::capture(&journal_path(path)),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RefResolve {
    Found(String),
    NotFound,
    Stale,
    DecodeFailed,
}

impl RecoveryStore {
    pub fn new(persistence_path: Option<PathBuf>) -> Self {
        Self::with_config(persistence_path, RecoveryConfig::default())
    }

    pub fn with_config(persistence_path: Option<PathBuf>, config: RecoveryConfig) -> Self {
        let loaded = persistence_path
            .as_ref()
            .and_then(|path| load_state(path, &config).ok().flatten());
        let (disk_identity, journal_identity) = loaded
            .as_ref()
            .and(persistence_path.as_deref())
            .map(cache_identities)
            .unwrap_or_default();
        let state = loaded.unwrap_or_else(|| RecoveryState::empty(&config));
        let shared_cas = persistence_path
            .as_deref()
            .and_then(SharedCas::detect_from_cache_path)
            .or_else(|| {
                persistence_path
                    .is_some()
                    .then(ref_index_root)
                    .flatten()
                    .map(SharedCas::new)
            });
        Self {
            config,
            persistence_path,
            state,
            session_refs: Vec::new(),
            ref_classes: BTreeMap::new(),
            disk_identity,
            journal_identity,
            shared_cas,
            recovery_count: 0,
            recovery_tokens: 0,
            legacy_read_count: 0,
            telemetry: CrossEngineTelemetry::default(),
            pending_blob_deletions: BTreeSet::new(),
            pending_alias_deletions: BTreeSet::new(),
            payload_memo: None,
            skip_empty_persist: false,
        }
    }

    /// Persist exact bytes as a durable content-addressed blob without
    /// creating file/unit index entries. Used when prompt spans are paged out.
    pub fn store_blob(
        &mut self,
        text: &str,
        content_type: ContentType,
    ) -> Result<String, RecoveryError> {
        let ref_id = self.put_blob(text, content_type);
        self.persist_evicted(ref_id)
    }

    /// Persist a blob as a pointer to an exact source-file line range.
    ///
    /// This is an explicit opt-in path; ordinary blob writers remain inline so
    /// ephemeral stdin, shell, and slice content survives source deletion.
    pub fn store_file_backed_blob(
        &mut self,
        path: &Path,
        source_start_line: usize,
        source_end_line: usize,
        content_type: ContentType,
    ) -> Result<String, RecoveryError> {
        let source = fs::read_to_string(path)?;
        let line_count = content_line_count(&source);
        if line_range_out_of_bounds(source_start_line, source_end_line, line_count) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid source line range {source_start_line}..={source_end_line}; file has {line_count} lines"),
            )
            .into());
        }
        let text = line_slice_exact(&source, source_start_line, source_end_line);
        let ref_id = self.put_file_backed_blob(
            &text,
            path,
            source_start_line,
            source_end_line,
            content_type,
        );
        self.persist_evicted(ref_id)
    }

    persist_after_deferred!(
        store_payload,
        store_payload_deferred(
            text: &str,
            content_type: ContentType,
            path: Option<&Path>,
            source_start_line: Option<usize>,
            source_end_line: Option<usize>,
        ) -> StoredPayload
    );

    pub fn store_payload_deferred(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> StoredPayload {
        let stored = self.store_payload_deferred_batch(
            text,
            content_type,
            path,
            source_start_line,
            source_end_line,
        );
        if !self.skip_empty_persist {
            self.evict();
        }
        stored
    }

    pub fn store_payload_deferred_batch(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> StoredPayload {
        if let Some(stored) = self.admit_memoized_payload(
            text,
            content_type,
            path,
            source_start_line,
            source_end_line,
            false,
        ) {
            return stored;
        }
        let blob_ref = self.put_blob(text, content_type);
        let file_ref = self.put_file(text, content_type, path, source_start_line, source_end_line);
        let stored = self.finish_payload(
            blob_ref,
            file_ref,
            text,
            content_type,
            source_start_line,
            source_end_line,
        );
        self.memoize_payload(
            text,
            content_type,
            path,
            (source_start_line, source_end_line),
            false,
            &stored,
        );
        stored
    }

    /// Admit an already-read complete source file without duplicating its payload.
    pub fn store_source_backed_payload_deferred_batch(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: &Path,
    ) -> StoredPayload {
        if let Some(stored) =
            self.admit_memoized_payload(text, content_type, Some(path), None, None, true)
        {
            return stored;
        }
        let source_sha256 = sha256_hex(text);
        let blob_ref = self.put_file_backed_blob_hashed(
            path,
            1,
            content_line_count(text),
            content_type,
            &source_sha256,
        );
        let file_ref = self.put_source_backed_file(text, content_type, path, &source_sha256);
        let stored = self.finish_payload(blob_ref, file_ref, text, content_type, None, None);
        self.memoize_payload(text, content_type, Some(path), (None, None), true, &stored);
        stored
    }

    fn memoized_payload(
        &self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
        source_backed: bool,
    ) -> Option<StoredPayload> {
        let memo = self
            .payload_memo
            .as_ref()?
            .matches(
                text,
                content_type,
                path,
                source_start_line,
                source_end_line,
                source_backed,
            )
            .then_some(self.payload_memo.as_ref()?)?;
        let refs_live = self.state.files.contains_key(&memo.stored.file_ref)
            && memo
                .stored
                .unit_refs
                .iter()
                .all(|ref_id| self.state.units.contains_key(ref_id))
            && self.has_ref(&memo.stored.blob_ref);
        refs_live.then(|| memo.stored.clone())
    }

    fn admit_memoized_payload(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
        source_backed: bool,
    ) -> Option<StoredPayload> {
        let stored = self.memoized_payload(
            text,
            content_type,
            path,
            source_start_line,
            source_end_line,
            source_backed,
        )?;
        self.skip_empty_persist = true;
        Some(stored)
    }

    fn memoize_payload(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_lines: (Option<usize>, Option<usize>),
        source_backed: bool,
        stored: &StoredPayload,
    ) {
        let (source_start_line, source_end_line) = source_lines;
        self.payload_memo = Some(PayloadMemo {
            text: text.to_owned(),
            content_type,
            path: path.map(Path::to_path_buf),
            source_start_line,
            source_end_line,
            source_backed,
            stored: stored.clone(),
        });
    }

    fn finish_payload(
        &mut self,
        blob_ref: String,
        file_ref: String,
        text: &str,
        content_type: ContentType,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> StoredPayload {
        let unit_refs = self.index_units(text, content_type, &file_ref);
        StoredPayload {
            blob_ref,
            file_ref,
            unit_refs,
            raw_tokens: count_tokens(text),
            source_start_line,
            source_end_line,
        }
    }

    fn persist_value<T>(&mut self, value: T) -> Result<T, RecoveryError> {
        self.persist()?;
        Ok(value)
    }

    fn persist_evicted<T>(&mut self, value: T) -> Result<T, RecoveryError> {
        self.evict();
        self.persist_value(value)
    }

    pub fn persist_pending(&mut self) -> Result<(), RecoveryError> {
        self.persist()
    }

    /// Publish all deferred mutations as one recovery entry and make that
    /// publication durable before returning to a caller that will acknowledge it.
    pub fn persist_pending_durable(&mut self) -> Result<(), RecoveryError> {
        #[cfg(test)]
        fail_durable_commit_at(DurableCommitFailPoint::BeforePersist)?;
        self.persist()?;
        let Some(path) = self.persistence_path.as_deref() else {
            return Ok(());
        };
        let journal = journal_path(path);
        let published = if journal.exists() { &journal } else { path };
        #[cfg(test)]
        fail_durable_commit_at(DurableCommitFailPoint::BeforeFileSync)?;
        if published.exists() {
            fs::File::open(published)?.sync_all()?;
        }
        #[cfg(test)]
        fail_durable_commit_at(DurableCommitFailPoint::BeforeDirectorySync)?;
        #[cfg(unix)]
        {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    pub fn reserve_ordinal_range(&mut self, count: u64) -> Result<OrdinalRange, RecoveryError> {
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ordinal range must be non-empty",
            )
            .into());
        }
        let Some(path) = self.persistence_path.clone() else {
            let start = self.state.next_ordinal;
            let end_exclusive = start
                .checked_add(count)
                .ok_or_else(|| io::Error::other("ordinal counter overflow"))?;
            self.state.next_ordinal = end_exclusive;
            return Ok(OrdinalRange {
                generation: self.state.ordinal_generation,
                start,
                end_exclusive,
            });
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = PersistLock::acquire(recovery_lock_path(&path))?;
        let existing =
            load_state(&path, &self.config)?.unwrap_or_else(|| RecoveryState::empty(&self.config));
        let current = std::mem::replace(&mut self.state, RecoveryState::empty(&self.config));
        self.state = merge_states(existing, current, &self.session_refs, &self.config);
        let start = self.state.next_ordinal;
        let end_exclusive = start
            .checked_add(count)
            .ok_or_else(|| io::Error::other("ordinal counter overflow"))?;
        let range = OrdinalRange {
            generation: self.state.ordinal_generation,
            start,
            end_exclusive,
        };
        self.state.next_ordinal = end_exclusive;
        self.publish_snapshot(&path)?;
        Ok(range)
    }

    pub fn store_ordinal_alias_deferred(
        &mut self,
        range: OrdinalRange,
        offset: u64,
        target_ref: &str,
    ) -> Result<String, RecoveryError> {
        let alias = range.ref_for(offset).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "ordinal offset outside reserved range",
            )
        })?;
        let target =
            canonical_full_blob_ref(split_ref_fragment(target_ref).0).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ordinal target must be a full-hash blob ref",
                )
            })?;
        if self
            .state
            .aliases
            .get(&alias)
            .is_some_and(|existing| existing != &target)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "ordinal alias already targets another ref",
            )
            .into());
        }
        self.store_alias_deferred(&alias, &target);
        Ok(alias)
    }

    pub fn store_alias(&mut self, alias: &str, target_ref: &str) -> Result<(), RecoveryError> {
        self.store_alias_deferred(alias, target_ref);
        self.persist()
    }

    /// Store an alias without persisting. Caller must call `persist_pending()`.
    pub fn store_alias_deferred(&mut self, alias: &str, target_ref: &str) {
        self.skip_empty_persist = false;
        if self
            .state
            .aliases
            .get(alias)
            .is_none_or(|current| current != target_ref)
        {
            self.state
                .transparency
                .append(format!("alias-cas\0{alias}\0{target_ref}").as_bytes());
        }
        self.state
            .aliases
            .insert(alias.to_string(), target_ref.to_string());
    }

    /// Current MMR transparency commitment for recovery mutations.
    pub fn transparency_root(&self) -> String {
        self.state.transparency.root()
    }
    pub fn transparency_len(&self) -> usize {
        self.state.transparency.len()
    }
    pub fn transparency_inclusion_proof(
        &self,
        leaf_index: usize,
    ) -> Option<crate::transparency::InclusionProof> {
        self.state.transparency.inclusion_proof(leaf_index)
    }
    pub fn transparency_consistency_proof(
        &self,
        old_size: usize,
    ) -> Option<crate::transparency::ConsistencyProof> {
        self.state.transparency.consistency_proof(old_size)
    }

    /// Remove an alias after the next authoritative persist.
    pub(crate) fn remove_alias(&mut self, alias: &str) {
        self.skip_empty_persist = false;
        self.state.aliases.remove(alias);
        self.state.ambiguous_aliases.remove(alias);
        self.pending_alias_deletions.insert(alias.to_string());
    }

    /// Remove a blob after the next authoritative persist.
    pub(crate) fn remove_blob(&mut self, ref_id: &str) {
        self.skip_empty_persist = false;
        self.state.blobs.remove(ref_id);
        self.pending_blob_deletions.insert(ref_id.to_string());
    }

    /// Mark a short ref as ambiguous (maps to multiple full hashes).
    pub fn mark_ambiguous(&mut self, short_ref: &str) {
        self.skip_empty_persist = false;
        self.state.ambiguous_aliases.insert(short_ref.to_string());
    }

    /// Check whether a short ref has been marked as ambiguous.
    pub fn is_alias_ambiguous(&self, short_ref: &str) -> bool {
        self.state.ambiguous_aliases.contains(short_ref)
    }

    /// Return the target ref for an existing alias, if any.
    pub fn alias_target(&self, alias: &str) -> Option<String> {
        self.state.aliases.get(alias).cloned()
    }

    /// Return all blob ref IDs currently in the store (for migration scanning).
    pub fn blob_ref_ids(&self) -> Vec<String> {
        self.state.blobs.keys().cloned().collect()
    }

    /// Resolve a blob's content by its full ref ID.
    /// Returns None if not found or if the stored value cannot be resolved.
    pub(crate) fn resolve_blob_content(&self, ref_id: &str) -> Option<String> {
        self.state.blobs.get(ref_id).and_then(|value| {
            match resolve_blob_value(self.persistence_path.as_deref(), ref_id, value) {
                RefResolve::Found(content) => Some(content),
                RefResolve::NotFound | RefResolve::Stale | RefResolve::DecodeFailed => None,
            }
        })
    }

    // Resolve foreign blobs from a sibling engine store under the same root.
    fn expand_in_sibling_engine_store(
        &self,
        requested_ref: &str,
        selector: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        anchor_kind: Option<&str>,
        symbol: Option<&str>,
    ) -> Option<ExpansionResult> {
        let engine = match requested_ref.split_once("://")?.0 {
            "fz" => "fszero",
            "gz" => "graphzero",
            _ => return None,
        };
        let self_cache = self.persistence_path.as_deref()?;
        let sibling_cache = SharedCas::sibling_engine_cache_path(self_cache, engine)?;
        if sibling_cache == self_cache || !sibling_cache.is_file() {
            return None;
        }
        let canonical = canonicalize_expand_ref(requested_ref)?;
        let mut sibling_store = RecoveryStore::new(Some(sibling_cache));
        let result = sibling_store.expand(
            &canonical,
            selector,
            start_line,
            end_line,
            anchor_kind,
            symbol,
        );
        result.found.then(|| {
            ExpansionResult::ok(requested_ref.to_string(), result.selector, result.content)
        })
    }

    /// Return migration/compatibility state for doctor JSON output.
    /// Contains no payload content or filesystem paths.
    pub fn migration_state(&self) -> serde_json::Value {
        serde_json::json!({
            "legacy_compat_enabled": self.config.legacy_compat,
            "legacy_compat_deadline": self.config.legacy_compat_deadline,
            "legacy_compat_supported_until": "tokenzero-v2.0",
            "legacy_blob_count": self.state.blobs.keys()
                .filter(|k| crate::migration::is_legacy_blob_ref(k))
                .count(),
            "canonical_blob_count": self.state.blobs.keys()
                .filter(|k| k.starts_with("tz://blob/") && k.len() == 74)
                .count(),
            "alias_count": self.state.aliases.len(),
            "ambiguous_alias_count": self.state.ambiguous_aliases.len(),
            "shared_cas_attached": self.shared_cas.is_some(),
            "legacy_read_count_session": self.legacy_read_count,
        })
    }
    pub fn expected_refs(text: &str, path: Option<&Path>) -> (String, String) {
        let blob_ref = format!("tz://blob/{}", sha256_hex(text));
        let file_ref = recovery_file_ref(text, path);
        (blob_ref, file_ref)
    }

    persist_after_deferred!(
        store_search_output,
        store_search_output_deferred(output: &str, query: Option<&str>) -> Vec<String>
    );

    pub fn store_search_output_deferred(
        &mut self,
        output: &str,
        query: Option<&str>,
    ) -> Vec<String> {
        let path_line = &*SEARCH_PATH_LINE;
        let mut refs = Vec::new();
        for (idx, line) in output.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if query.is_some_and(|q| !line.contains(q)) && !path_line.is_match(line) {
                continue;
            }
            let hit_id = id_for('h', &format!("search:{idx}:{line}"));
            let ref_id = format!("tz://search/{hit_id}");
            refs.push(self.insert_stored_unit(
                true,
                ref_id,
                line,
                ContentType::SearchResult,
                None,
                (Some(idx + 1), Some(idx + 1)),
            ));
        }
        self.evict();
        refs
    }

    pub fn expand(
        // Validate routing and fragments before resolving CAS/local content and selectors.
        &mut self,
        ref_id: &str,
        selector: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        anchor_kind: Option<&str>,
        symbol: Option<&str>,
    ) -> ExpansionResult {
        self.recovery_count += 1;
        let requested_ref = ref_id.to_string();
        let selector_owned = selector.map(str::to_string);
        macro_rules! miss {
            ($reason:expr) => {
                ExpansionResult::missing(requested_ref.clone(), selector_owned.clone(), $reason)
            };
        }
        let early_fragment = ref_id.split_once('#').map(|(_, fragment)| fragment);
        let early_fragment_spec = match early_fragment.map(parse_fragment_spec).transpose() {
            Err(err) => return miss!(fragment_error_reason(err)),
            Ok(spec) => spec,
        };
        let portable = match parse_expand_portable(ref_id) {
            Ok(parsed) => parsed,
            Err(reason) => return miss!(reason),
        };
        if portable.is_none() && is_foreign_non_blob_ref(ref_id) {
            return miss!("unsupported-ref-kind");
        }
        let Some(lookup_ref) = canonicalize_expand_ref(ref_id) else {
            return miss!("invalid-ref");
        };
        if let Some(reason) = self.note_legacy_expand(&lookup_ref) {
            return miss!(reason);
        }
        let resolved_ref = self.resolve_alias_chain(&lookup_ref).unwrap_or(lookup_ref);
        let portable_resolved = parse_zeroref_v1_blob(&resolved_ref, None).ok();
        let shared_content = match (&portable_resolved, &self.shared_cas) {
            (Some(portable), Some(cas)) => match shared_cas_utf8(cas, &portable.hash) {
                Ok(content) => Some(content),
                Err(None) => return miss!("shared-cas-non-utf8"),
                Err(Some(SharedCasError::NotFound)) if requested_ref.starts_with("tz://") => None,
                Err(Some(SharedCasError::NotFound)) => {
                    if let Some(result) = self.expand_in_sibling_engine_store(
                        &requested_ref,
                        selector,
                        start_line,
                        end_line,
                        anchor_kind,
                        symbol,
                    ) {
                        return result;
                    }
                    return miss!("shared-cas-missing");
                }
                Err(Some(err)) => return miss!(shared_cas_error_reason(err)),
            },
            _ => None,
        };
        let ref_id = resolved_ref;
        let Some(parsed) = parse_ref(&ref_id) else {
            return miss!("invalid-ref");
        };
        let mut selected_start = start_line;
        let mut selected_end = end_line;
        // Reuse the early parse when the resolved ref kept the same fragment text.
        let fragment_spec = match (early_fragment, early_fragment_spec, parsed.fragment) {
            (Some(early), Some(spec), Some(pf)) if early == pf => Some(Ok(spec)),
            (_, _, Some(pf)) => Some(parse_fragment_spec(pf)),
            _ => None,
        };
        if let Some(Err(err)) = &fragment_spec {
            return miss!(fragment_error_reason(*err));
        }
        if let Some(Ok(FragmentSpec::Line { start, end })) = &fragment_spec {
            selected_start = Some(*start);
            selected_end = Some(*end);
        }
        resolve_selector_line_window(selector, &mut selected_start, &mut selected_end);
        let content = if let Some(content) = shared_content {
            content
        } else {
            match resolve_to_expand_content(
                self.resolve_ref_with_index(parsed.kind, parsed.bare),
                &requested_ref,
                parsed.kind,
            ) {
                Ok(content) => content,
                Err(reason) => return miss!(reason),
            }
        };
        if portable
            .as_ref()
            .is_some_and(|portable| sha256_hex(&content) != portable.hash)
        {
            return miss!("zeroref-corruption");
        }
        if parsed.kind == "file" && self.file_ref_is_stale(parsed.bare) {
            return miss!("stale-ref");
        }
        let line_window = if matches!(fragment_spec, Some(Ok(FragmentSpec::Byte { .. }))) {
            None
        } else {
            match clamp_line_window(&content, selected_start, &mut selected_end) {
                Ok(window) => window,
                Err(reason) => return miss!(reason),
            }
        };
        match expand_selected_content(
            content,
            &fragment_spec,
            selector,
            selected_start,
            selected_end,
            anchor_kind,
            symbol,
        ) {
            Ok(selected) => {
                let mut result = self.expand_ok(requested_ref, selector_owned, &ref_id, selected);
                if let Some((clamped, start, end, line_count)) = line_window {
                    result.clamped = clamped;
                    result.returned_start_line = Some(start);
                    result.returned_end_line = Some(end);
                    result.line_count = Some(line_count);
                }
                result
            }
            Err(reason) => miss!(reason),
        }
    }

    fn note_legacy_expand(&mut self, lookup_ref: &str) -> Option<&'static str> {
        if !is_legacy_same_store_blob_ref(lookup_ref) {
            return None;
        }
        if !self.config.legacy_compat {
            return Some("legacy-ref-disabled");
        }
        if self.state.ambiguous_aliases.contains(lookup_ref) {
            return Some("legacy-ambiguous");
        }
        self.legacy_read_count += 1;
        None
    }

    fn expand_ok(
        &mut self,
        requested_ref: String,
        selector: Option<String>,
        ref_id: &str,
        content: String,
    ) -> ExpansionResult {
        self.note_expand(ref_id, &content);
        ExpansionResult::ok(requested_ref, selector, content)
    }

    fn note_expand(&mut self, ref_id: &str, content: &str) {
        self.recovery_tokens += count_tokens(content);
        if let Some(store_path) = self.persistence_path.as_ref() {
            let content_class = self
                .ref_classes
                .get(ref_id)
                .copied()
                .unwrap_or_else(|| classify_ref(ref_id, None));
            record_ref_index_expanded(store_path, ref_id, content_class);
        }
    }

    fn resolve_alias_chain(&self, ref_id: &str) -> Option<String> {
        let (bare, frag) = split_ref_fragment(ref_id);
        let mut current = bare;
        let mut advanced = false;
        for _ in 0..8 {
            let Some(next) = self.state.aliases.get(current) else {
                if !advanced {
                    return None;
                }
                return Some(match frag {
                    Some(f) => format!("{current}#{f}"),
                    None => current.to_string(),
                });
            };
            current = next;
            advanced = true;
        }
        None
    }

    /// Register `tz://s/<16hex>` → full-hash blob alias and return the short form
    /// for visible capsules. Non-full-hash refs pass through unchanged.
    /// Register a session-visible short alias without flushing to disk.
    /// Callers that batch many aliases should finish with `persist_pending`.
    pub fn register_session_visible_alias(&mut self, ref_id: &str) -> String {
        let Some(short) = session_visible_blob_alias(ref_id) else {
            return ref_id.to_string();
        };
        let (short_bare, _) = split_ref_fragment(&short);
        if let Some(full_bare) = canonical_full_blob_ref(split_ref_fragment(ref_id).0) {
            if self.alias_target(short_bare).as_deref() != Some(full_bare.as_str()) {
                self.store_alias_deferred(short_bare, &full_bare);
            }
        }
        short
    }

    /// Ensure a full-hash blob ref has a durable session-visible short alias.
    /// Persists immediately so a subsequent process restart can expand the short form.
    pub fn ensure_session_visible_alias(&mut self, ref_id: &str) -> String {
        let short = self.register_session_visible_alias(ref_id);
        let _ = self.persist_pending();
        short
    }

    /// Rewrite full-hash blob refs in text to session-visible short aliases,
    /// registering each short → full mapping in the alias table (deferred).
    pub fn apply_session_visible_aliases_in_text(&mut self, text: &str) -> String {
        // Skip the char-by-char scan when the payload has no full-hash blob refs.
        if !text.contains("tz://blob/")
            && !text.contains("fz://blob/")
            && !text.contains("gz://blob/")
        {
            return text.to_string();
        }
        let mut cursor = 0usize;
        while cursor < text.len() {
            if let Some((end, full)) = crate::session_aliases::take_full_hash_blob_at(text, cursor)
            {
                let _ = self.register_session_visible_alias(&full);
                cursor = end;
            } else {
                cursor += 1;
            }
        }
        rewrite_full_hash_blob_refs_in_text(text)
    }

    /// Shorten full-hash blob ref strings inside a JSON value.
    pub fn apply_session_visible_aliases_in_value(&mut self, value: &mut serde_json::Value) {
        fn walk(store: &mut RecoveryStore, value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(text) => {
                    if session_visible_blob_alias(text).is_some() {
                        *text = store.register_session_visible_alias(text);
                    } else if text.contains("://blob/") {
                        *text = store.apply_session_visible_aliases_in_text(text);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        walk(store, item);
                    }
                }
                serde_json::Value::Object(map) => {
                    for item in map.values_mut() {
                        walk(store, item);
                    }
                }
                _ => {}
            }
        }
        walk(self, value);
    }

    pub fn has_ref(&self, ref_id: &str) -> bool {
        let Some(lookup) = canonicalize_expand_ref(ref_id) else {
            return false;
        };
        let lookup = self.resolve_alias_chain(&lookup).unwrap_or(lookup);
        let Some(parsed) = parse_ref(&lookup) else {
            return false;
        };
        match parsed.kind {
            "blob" => self.blob_reachable(parsed.bare),
            "file" => self.state.files.contains_key(parsed.bare),
            "unit" | "search" => recovery_unit_map(&self.state, parsed.kind)
                .is_some_and(|m| m.contains_key(parsed.bare)),
            _ => false,
        }
    }

    /// Local/CAS reachability only — never opens sibling stores via ref-index.
    ///
    /// Session resume must use this: calling [`Self::has_ref`] per persisted
    /// record can reload multi-MB journals thousands of times and peg a core.
    pub fn has_ref_local(&self, ref_id: &str) -> bool {
        let Some(lookup) = canonicalize_expand_ref(ref_id) else {
            return false;
        };
        let lookup = self.resolve_alias_chain(&lookup).unwrap_or(lookup);
        let Some(parsed) = parse_ref(&lookup) else {
            return false;
        };
        match parsed.kind {
            "blob" => {
                self.state.blobs.contains_key(parsed.bare)
                    || self
                        .shared_cas
                        .as_ref()
                        .and_then(|cas| {
                            ref_index_id_part(parsed.bare).map(|hash| cas.contains(hash))
                        })
                        .unwrap_or(false)
            }
            "file" => self.state.files.contains_key(parsed.bare),
            "unit" | "search" => recovery_unit_map(&self.state, parsed.kind)
                .is_some_and(|m| m.contains_key(parsed.bare)),
            _ => false,
        }
    }

    fn blob_reachable(&self, bare: &str) -> bool {
        self.state.blobs.contains_key(bare)
            || self
                .shared_cas
                .as_ref()
                .and_then(|cas| ref_index_id_part(bare).map(|hash| cas.contains(hash)))
                .unwrap_or(false)
            // Skip reloading this store via ref-index: `self.state` was already
            // loaded (and journal-applied) in `new`. Reloading the same multi-MB
            // cache+journal per has_ref pegs CPU on large session resumes.
            || blob_reachable_in_ref_index(bare, &self.config, self.persistence_path.as_deref())
    }

    pub fn export_status(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": "tokenzero.recovery.v1",
            "blobs": self.state.blobs.len(),
            "files": self.state.files.len(),
            "units": self.state.units.len(),
            "search_hits": self.state.search_hits.len(),
            "max_blobs": self.config.max_blobs,
            "max_files": self.config.max_files,
            "max_units": self.config.max_units,
            "max_search_hits": self.config.max_search_hits,
            "approx_bytes": self.approx_bytes(),
            "max_bytes": self.config.max_bytes,
            "recovery_count": self.recovery_count,
            "recovery_tokens": self.recovery_tokens,
            "persistent": self.persistence_path.is_some(),
            "persistence_path": self.persistence_path.as_ref().map(|p| p.display().to_string()),
        })
    }

    pub fn prune_stale(&mut self, dry_run: bool) -> Result<serde_json::Value, RecoveryError> {
        let stale: Vec<String> = self
            .state
            .files
            .keys()
            .filter(|ref_id| self.file_ref_is_stale(ref_id))
            .cloned()
            .collect();
        if !dry_run {
            for ref_id in &stale {
                self.drop_ref(ref_id);
            }
            self.persist()?;
        }
        Ok(serde_json::json!({
            "schema_version": "tokenzero.cache.v1",
            "status": "ok",
            "dry_run": dry_run,
            "candidates": stale.iter().map(|ref_id| {
                serde_json::json!({"category": "exact", "ref": ref_id, "reason": "stale-source"})
            }).collect::<Vec<_>>(),
            "reclaimed_bytes": if dry_run { 0 } else { stale.len() },
        }))
    }

    // Record the outcome of a shell command and report whether it repeated
    // the previous run byte-for-byte (same combined output, same exit code).
    // Callers may render verified-unchanged successes as a tiny delta
    // envelope; the content-addressed blob ref still recovers exact bytes.
    persist_after_deferred!(
        record_shell_outcome,
        record_shell_outcome_deferred(
            scope: Option<&str>,
            command: &str,
            combined: &str,
            exit_code: Option<i32>,
        ) -> ShellRepeat
    );

    pub fn record_shell_outcome_deferred(
        &mut self,
        scope: Option<&str>,
        command: &str,
        combined: &str,
        exit_code: Option<i32>,
    ) -> ShellRepeat {
        self.skip_empty_persist = false;
        let key = id_for('s', &format!("{}\u{0}{command}", scope.unwrap_or("")));
        let combined_sha = sha256_hex(combined);
        let seq = self.state.shell_outcome_seq.wrapping_add(1);
        self.state.shell_outcome_seq = seq;
        let (unchanged, seen) = match self.state.shell_outcomes.get(&key) {
            Some(prev) if prev.combined_sha == combined_sha && prev.exit_code == exit_code => {
                (true, prev.seen.saturating_add(1))
            }
            _ => (false, 1),
        };
        self.state.shell_outcomes.insert(
            key,
            ShellOutcome {
                combined_sha,
                exit_code,
                seen,
                seq,
            },
        );
        trim_shell_outcomes(&mut self.state.shell_outcomes);
        ShellRepeat { unchanged, seen }
    }

    fn put_file_backed_blob(
        &mut self,
        text: &str,
        path: &Path,
        source_start_line: usize,
        source_end_line: usize,
        content_type: ContentType,
    ) -> String {
        self.put_file_backed_blob_hashed(
            path,
            source_start_line,
            source_end_line,
            content_type,
            &sha256_hex(text),
        )
    }

    fn put_file_backed_blob_hashed(
        &mut self,
        path: &Path,
        source_start_line: usize,
        source_end_line: usize,
        content_type: ContentType,
        full_hash: &str,
    ) -> String {
        self.register_blob(
            full_hash,
            format!("tz://blob/b{}", &full_hash[..16]),
            content_type,
            Some(BlobEntry::FileRef {
                path: path.to_path_buf(),
                source_start_line,
                source_end_line,
            }),
        )
    }

    fn track_ref_class(&mut self, ref_id: &str, content_type: ContentType) {
        self.ref_classes
            .insert(ref_id.to_string(), classify_ref(ref_id, Some(content_type)));
    }

    fn register_blob(
        &mut self,
        full_hash: &str,
        legacy_ref: String,
        content_type: ContentType,
        value: Option<BlobEntry>,
    ) -> String {
        let ref_id = format!("tz://blob/{full_hash}");
        self.track_ref_class(&ref_id, content_type);
        if legacy_ref != ref_id {
            self.state.aliases.insert(legacy_ref, ref_id.clone());
        }
        if let Some(value) = value {
            self.state.blobs.insert(ref_id.clone(), value);
        }
        self.remember_ref(&ref_id);
        ref_id
    }

    fn put_blob(&mut self, text: &str, content_type: ContentType) -> String {
        let full_hash = sha256_hex(text);
        let canonical_ref = format!("tz://blob/{full_hash}");
        if !self.state.blobs.contains_key(&canonical_ref) {
            self.state
                .transparency
                .append(format!("mint\0{canonical_ref}").as_bytes());
        }
        let published = self
            .shared_cas
            .as_ref()
            .is_some_and(|cas| cas.publish(text.as_bytes()).is_ok());
        let value = if published {
            None
        } else {
            let text = self
                .persistence_path
                .as_deref()
                .and_then(|cache| externalize_blob_value(cache, text, &full_hash))
                .unwrap_or_else(|| text.to_string());
            Some(BlobEntry::Inline(text))
        };
        self.register_blob(
            &full_hash,
            format!("tz://blob/{}", id_for('b', text)),
            content_type,
            value,
        )
    }

    fn put_file(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> String {
        self.put_file_entry(
            text,
            content_type,
            path,
            false,
            || fingerprint_for_stored_payload(path, source_start_line, source_end_line),
            source_start_line,
            source_end_line,
        )
    }

    fn put_source_backed_file(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: &Path,
        source_sha256: &str,
    ) -> String {
        self.put_file_entry(
            text,
            content_type,
            Some(path),
            true,
            || source_fingerprint_from_sha256(path, source_sha256),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn put_file_entry<F: FnOnce() -> Option<SourceFingerprint>>(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_backed: bool,
        source_fingerprint: F,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> String {
        let ref_id = recovery_file_ref(text, path);
        self.track_ref_class(&ref_id, content_type);
        self.state.files.insert(
            ref_id.clone(),
            StoredFile {
                ref_id: ref_id.clone(),
                path: path.map(|path| path.to_string_lossy().into_owned()),
                path_identity: path.map(path_identity_text),
                source_backed,
                text: if source_backed {
                    String::new()
                } else {
                    text.to_string()
                },
                content_type: content_type.to_string(),
                source_fingerprint: source_fingerprint(),
                source_start_line,
                source_end_line,
            },
        );
        self.remember_ref(&ref_id);
        ref_id
    }

    fn index_units(
        &mut self,
        text: &str,
        content_type: ContentType,
        source_ref: &str,
    ) -> Vec<String> {
        let mut refs = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            let stripped = line.trim();
            if stripped.len() >= 12 {
                refs.push(self.put_unit(
                    stripped,
                    content_type,
                    Some(source_ref),
                    Some(idx + 1),
                    Some(idx + 1),
                ));
            }
            if refs.len() >= 64 {
                break;
            }
        }
        refs
    }

    fn put_unit(
        &mut self,
        text: &str,
        content_type: ContentType,
        source_ref: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> String {
        self.insert_stored_unit(
            false,
            format!("tz://unit/{}", id_for('u', text)),
            text,
            content_type,
            source_ref,
            (start_line, end_line),
        )
    }

    fn insert_stored_unit(
        &mut self,
        search_hit: bool,
        ref_id: String,
        text: &str,
        content_type: ContentType,
        source_ref: Option<&str>,
        source_lines: (Option<usize>, Option<usize>),
    ) -> String {
        let (start_line, end_line) = source_lines;
        self.track_ref_class(&ref_id, content_type);
        let unit = StoredUnit {
            ref_id: ref_id.clone(),
            text: text.to_string(),
            content_type: content_type.to_string(),
            source_ref: source_ref.map(str::to_string),
            start_line,
            end_line,
        };
        if search_hit {
            self.state.search_hits.insert(ref_id.clone(), unit);
        } else {
            self.state.units.entry(ref_id.clone()).or_insert(unit);
        }
        self.remember_ref(&ref_id);
        ref_id
    }

    fn resolve_ref(&self, kind: &str, bare: &str) -> RefResolve {
        match kind {
            "blob" => self
                .state
                .blobs
                .get(bare)
                .map_or(RefResolve::NotFound, |value| {
                    resolve_blob_value(self.persistence_path.as_deref(), bare, value)
                }),
            "file" => self
                .state
                .files
                .get(bare)
                .map(resolve_file_value)
                .unwrap_or(RefResolve::NotFound),
            "unit" | "search" => recovery_unit_map(&self.state, kind)
                .and_then(|units| units.get(bare))
                .map_or(RefResolve::NotFound, |u| RefResolve::Found(u.text.clone())),
            _ => RefResolve::NotFound,
        }
    }

    fn resolve_ref_with_index(&self, kind: &str, bare: &str) -> RefResolve {
        match self.resolve_ref(kind, bare) {
            RefResolve::NotFound if kind == "blob" => {
                resolve_blob_from_ref_index(bare, &self.config)
            }
            other => other,
        }
    }

    fn file_ref_is_stale(&self, bare: &str) -> bool {
        let Some(stored) = self.state.files.get(bare) else {
            return false;
        };
        if is_ephemeral_source_path(stored.path.as_deref().unwrap_or_default()) {
            return false;
        }
        let Some(expected) = stored.source_fingerprint.as_ref() else {
            return false;
        };
        let Some(source_path) = stored_source_path(stored) else {
            return false;
        };
        source_fingerprint(&source_path).is_none_or(|actual| actual != *expected)
    }

    fn remember_ref(&mut self, ref_id: &str) {
        self.skip_empty_persist = false;
        self.state.order.push(ref_id.to_string());
        self.session_refs.push(ref_id.to_string());
    }

    fn evict(&mut self) {
        recovery_maps!(evict self);
        while self.approx_bytes() > self.config.max_bytes {
            // CAS reachability must not pin a local eviction victim.
            let Some(victim) = self
                .state
                .order
                .iter()
                .find(|ref_id| self.local_entry_present(ref_id))
                .cloned()
            else {
                break;
            };
            self.drop_ref(&victim);
        }
        self.compact_order();
    }

    fn local_entry_present(&self, ref_id: &str) -> bool {
        state_entry_present(&self.state, ref_id)
    }

    fn drop_ref(&mut self, ref_id: &str) {
        self.skip_empty_persist = false;
        recovery_maps!(remove self.state, ref_id);
    }

    fn compact_order(&mut self) {
        let live: HashSet<String> = recovery_maps!(keys self.state).cloned().collect();
        let mut seen = HashSet::new();
        self.state
            .order
            .retain(|ref_id| live.contains(ref_id) && seen.insert(ref_id.clone()));
    }

    fn approx_bytes(&self) -> usize {
        // Externalized blob markers account at their original payload size so
        // eviction pressure reflects real content, not marker bytes.
        self.state.blobs.values().map(blob_value_len).sum::<usize>()
            + self
                .state
                .files
                .values()
                .map(|v| v.text.len() + v.path.as_deref().unwrap_or_default().len())
                .sum::<usize>()
            + self
                .state
                .units
                .values()
                .chain(self.state.search_hits.values())
                .map(|v| v.text.len())
                .sum::<usize>()
    }

    fn persist(&mut self) -> Result<(), RecoveryError> {
        let storage_unchanged = self.persistence_path.as_deref().is_none_or(|path| {
            let (disk_identity, journal_identity) = cache_identities(path);
            disk_identity == self.disk_identity && journal_identity == self.journal_identity
        });
        if self.persist_skip_empty(storage_unchanged) {
            self.skip_empty_persist = false;
            return Ok(());
        }
        self.skip_empty_persist = false;
        // The persist lock covers identity checks, merge, journal append, and snapshot publication.
        let Some(path) = self.persistence_path.clone() else {
            self.evict();
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = PersistLock::acquire(recovery_lock_path(&path))?;
        let snap_unchanged = self
            .disk_identity
            .is_some_and(|identity| DiskIdentity::capture(&path) == Some(identity));
        let journal_unchanged =
            self.journal_identity == DiskIdentity::capture(&journal_path(&path));
        let unchanged_since_last_write = snap_unchanged && journal_unchanged;
        if !unchanged_since_last_write {
            let existing = load_state(&path, &self.config)?
                .unwrap_or_else(|| RecoveryState::empty(&self.config));
            let current = std::mem::replace(&mut self.state, RecoveryState::empty(&self.config));
            self.state = merge_states(existing, current, &self.session_refs, &self.config);
        }
        self.evict();
        let has_pending_deletions =
            !self.pending_alias_deletions.is_empty() || !self.pending_blob_deletions.is_empty();
        apply_deletions(
            &mut self.state,
            self.pending_blob_deletions.iter().map(String::as_str),
            self.pending_alias_deletions.iter().map(String::as_str),
        );
        if self.try_append_session_journal(&path, unchanged_since_last_write, has_pending_deletions)
        {
            return Ok(());
        }
        self.publish_snapshot(&path)
    }

    fn persist_skip_empty(&self, storage_unchanged: bool) -> bool {
        self.skip_empty_persist
            && self.session_refs.is_empty()
            && self.pending_blob_deletions.is_empty()
            && self.pending_alias_deletions.is_empty()
            && storage_unchanged
    }

    // True when journal append published the delta. Restores session_refs on compaction/append failure.
    fn try_append_session_journal(
        &mut self,
        path: &Path,
        unchanged_since_last_write: bool,
        has_pending_deletions: bool,
    ) -> bool {
        if !(unchanged_since_last_write && !has_pending_deletions) {
            return false;
        }
        let delta = session_delta(&self.state, &self.session_refs, &self.config);
        let entry = JournalEntry {
            refs: std::mem::take(&mut self.session_refs),
            state: delta,
            deleted_blob_refs: self.pending_blob_deletions.iter().cloned().collect(),
            deleted_aliases: self.pending_alias_deletions.iter().cloned().collect(),
        };
        let segment_limit =
            journal_compact_threshold(self.disk_identity.map_or(0, |identity| identity.len));
        match append_journal(path, &entry, segment_limit) {
            Ok(JournalAppend::Appended) => {
                self.journal_identity = DiskIdentity::capture(&journal_path(path));
                append_blob_refs_to_ref_index(path, &entry.refs, Some(&self.ref_classes));
                self.clear_pending_deletions();
                true
            }
            Ok(JournalAppend::NeedsCompaction) | Err(_) => {
                self.session_refs = entry.refs;
                false
            }
        }
    }

    fn publish_snapshot(&mut self, path: &Path) -> Result<(), RecoveryError> {
        self.disk_identity = None;
        atomic_write_json(path, &self.state)?;
        remove_journal_segments(path);
        self.journal_identity = None;
        self.disk_identity = DiskIdentity::capture(path);
        append_blob_refs_to_ref_index(path, &self.session_refs, Some(&self.ref_classes));
        self.session_refs.clear();
        self.clear_pending_deletions();
        Ok(())
    }

    fn clear_pending_deletions(&mut self) {
        self.pending_blob_deletions.clear();
        self.pending_alias_deletions.clear();
    }
}

#[derive(Debug)]
struct ParsedRef<'a> {
    kind: &'a str,
    bare: &'a str,
    fragment: Option<&'a str>,
}

fn parse_ref(ref_id: &str) -> Option<ParsedRef<'_>> {
    let (bare, fragment) = ref_id
        .split_once('#')
        .map_or((ref_id, None), |(bare, fragment)| (bare, Some(fragment)));
    let rest = bare.strip_prefix("tz://")?;
    let (kind, id) = rest.split_once('/')?;
    if id.is_empty() || !matches!(kind, "blob" | "file" | "unit" | "search" | "codemode" | "s") {
        return None;
    }
    if kind == "codemode" {
        let mut parts = id.split('/');
        if parts.next() != Some("execution")
            || parts.next().is_none()
            || !matches!(
                parts.next(),
                Some("code" | "steps" | "telemetry" | "result" | "error")
            )
            || parts.next().is_some()
        {
            return None;
        }
    }
    if kind == "s" && !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(ParsedRef {
        kind,
        bare,
        fragment,
    })
}

fn parse_line_fragment(fragment: &str) -> (Option<usize>, Option<usize>) {
    let value = fragment.trim().trim_start_matches('L');
    let (start, end) = value.split_once('-').unwrap_or((value, value));
    (
        start.trim_start_matches('L').parse().ok(),
        end.trim_start_matches('L').parse().ok(),
    )
}

fn parse_around_selector(value: &str) -> (Option<usize>, Option<usize>) {
    let (line_text, radius_text) = value
        .split_once(':')
        .or_else(|| value.split_once(','))
        .unwrap_or((value, "3"));
    let line = line_text
        .trim()
        .trim_start_matches('L')
        .parse::<usize>()
        .unwrap_or(1);
    let radius = radius_text.trim().parse::<usize>().unwrap_or(3);
    (
        Some(line.saturating_sub(radius).max(1)),
        Some(line.saturating_add(radius)),
    )
}

// Line count matching exact split-inclusive slicing.
fn content_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split_inclusive('\n').count()
    }
}

// Return an exact one-based inclusive line slice.
fn line_slice_exact(text: &str, start: usize, end: usize) -> String {
    let start = start.max(1);
    text.split_inclusive('\n')
        .skip(start - 1)
        .take(end.max(start) - start + 1)
        .collect()
}

// Resolve a selector line window in place.
fn resolve_selector_line_window(
    selector: Option<&str>,
    selected_start: &mut Option<usize>,
    selected_end: &mut Option<usize>,
) {
    let Some(selector) = selector else { return };
    let window = ["range:", "lines:", "line:"]
        .into_iter()
        .find_map(|prefix| selector.strip_prefix(prefix).map(parse_line_fragment))
        .or_else(|| selector.strip_prefix("around:").map(parse_around_selector));
    if let Some((start, end)) = window {
        (*selected_start, *selected_end) = (start, end);
    }
}

fn select_content<'a>(
    content: String,
    selector: Option<&'a str>,
    start_line: Option<usize>,
    end_line: Option<usize>,
    anchor_kind: Option<&str>,
    symbol: Option<&'a str>,
) -> String {
    match selector {
        Some("error_block") => return error_block(&content, 3),
        Some("summary") => return tokenzero_core::summarize_lines(&content, 12, 8, ""),
        _ => {}
    }
    let (mut selected_start, mut selected_end) = (start_line, end_line);
    resolve_selector_line_window(selector, &mut selected_start, &mut selected_end);
    if let Some(start) = selected_start {
        return line_slice_exact(&content, start, selected_end.unwrap_or(start));
    }
    if let Some(symbol) = selector
        .and_then(|value| value.strip_prefix("symbol:"))
        .or(symbol)
    {
        return symbol_block(&content, symbol);
    }
    if anchor_kind.is_some() || selector.is_some_and(|value| value.starts_with("anchor:")) {
        return content
            .lines()
            .filter(|line| {
                let line = line.trim_start();
                [
                    "fn ", "def ", "class ", "struct ", "impl ", "use ", "import ",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content
}
fn ref_not_found_reason(kind: &str) -> String {
    if kind == "blob" && ref_index_enabled() {
        "ref-not-found; tiers tried: explicit/env cache, current-root store, per-user ref-index"
            .to_string()
    } else if kind == "blob" {
        "ref-not-found; tiers tried: explicit/env cache, current-root store (per-user ref-index disabled)".to_string()
    } else {
        "ref-not-found; tiers tried: explicit/env cache, current-root store".to_string()
    }
}

fn resolve_to_expand_content(
    resolve: RefResolve,
    requested_ref: &str,
    kind: &str,
) -> Result<String, String> {
    match resolve {
        RefResolve::Found(content) => Ok(content),
        RefResolve::Stale => Err("stale-ref".into()),
        RefResolve::DecodeFailed => Err("decode-failed".into()),
        RefResolve::NotFound if is_foreign_blob_ref(requested_ref) => Err("ref-not-found".into()),
        RefResolve::NotFound => Err(ref_not_found_reason(kind)),
    }
}
fn parse_expand_portable(ref_id: &str) -> Result<Option<ZeroRefV1Blob>, String> {
    match parse_zeroref_v1_blob(ref_id, None) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(ZeroRefError::Unsupported) => Ok(None),
        Err(ZeroRefError::LegacyAmbiguity) if is_legacy_same_store_blob_ref(ref_id) => Ok(None),
        Err(err) => Err(format!("zeroref-{err}")),
    }
}
fn expand_selected_content(
    content: String,
    fragment_spec: &Option<Result<FragmentSpec, FragmentError>>,
    selector: Option<&str>,
    selected_start: Option<usize>,
    selected_end: Option<usize>,
    anchor_kind: Option<&str>,
    symbol: Option<&str>,
) -> Result<String, String> {
    if let Some(Ok(FragmentSpec::Byte { start, end })) = fragment_spec {
        let bytes = content.as_bytes();
        if *end > bytes.len() {
            return Err(format!(
                "fragment-out-of-range; start={start} end={end} len={}",
                bytes.len()
            ));
        }
        return Ok(String::from_utf8_lossy(&bytes[*start..*end]).into_owned());
    }
    Ok(select_content(
        content,
        selector,
        selected_start,
        selected_end,
        anchor_kind,
        symbol,
    ))
}

fn clamp_line_window(
    content: &str,
    selected_start: Option<usize>,
    selected_end: &mut Option<usize>,
) -> Result<Option<(bool, usize, usize, usize)>, String> {
    let Some(start) = selected_start else {
        return Ok(None);
    };
    let requested_end = selected_end.unwrap_or(start);
    let line_count = content_line_count(content);
    if start == 0 || start > requested_end || start > line_count {
        return Err(format!(
            "window-out-of-range; start={start} end={requested_end} line_count={line_count}"
        ));
    }
    let returned_end = requested_end.min(line_count);
    *selected_end = Some(returned_end);
    Ok(Some((
        returned_end != requested_end,
        start,
        returned_end,
        line_count,
    )))
}

fn ref_index_enabled() -> bool {
    #[cfg(test)]
    if let Some((enabled, _)) = ref_index_test_override() {
        return enabled;
    }
    env::var(REF_INDEX_DISABLE_ENV)
        .map(|value| value.trim() != "0")
        .unwrap_or(true)
}

use std::sync::OnceLock;

/// Test-only hook: override the ref index root directory on the current thread.
/// Call with `Some(path)` to redirect, `None` to clear.
#[doc(hidden)]
pub fn set_ref_index_root_override(path: Option<PathBuf>) {
    REF_INDEX_ROOT_OVERRIDE.with(|root| root.replace(path));
}

std::thread_local! {
    static REF_INDEX_ROOT_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}
static REF_INDEX_DISABLED_OVERRIDE: OnceLock<std::sync::atomic::AtomicBool> = OnceLock::new();

/// Test-only hook: disable the per-user ref-index/shared-CAS fallback entirely
/// so stores exercise the local snapshot path regardless of ambient state.
#[doc(hidden)]
pub fn set_ref_index_disabled_override(disabled: bool) {
    REF_INDEX_DISABLED_OVERRIDE
        .get_or_init(|| std::sync::atomic::AtomicBool::new(false))
        .store(disabled, std::sync::atomic::Ordering::SeqCst);
}

fn ref_index_root() -> Option<PathBuf> {
    if let Some(flag) = REF_INDEX_DISABLED_OVERRIDE.get() {
        if flag.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
    }
    if let Some(path) = REF_INDEX_ROOT_OVERRIDE.with(|root| root.borrow().clone()) {
        return Some(path);
    }
    #[cfg(test)]
    if let Some((enabled, path)) = ref_index_test_override() {
        return enabled.then_some(path);
    }
    #[cfg(test)]
    return None;
    #[cfg(not(test))]
    {
        if !ref_index_enabled() {
            return None;
        }
        if let Some(path) = env::var_os(REF_INDEX_PATH_ENV).filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(path));
        }
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(|home| PathBuf::from(home).join(".tokenzero").join("ref-index"))
    }
}

fn create_ref_index_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn ref_index_id_part(ref_id: &str) -> Option<&str> {
    ref_id
        .rsplit_once('/')
        .map(|(_, id)| id)
        .filter(|id| !id.is_empty())
}

fn ref_index_shard_path(root: &Path, ref_id: &str) -> PathBuf {
    let id = ref_index_id_part(ref_id).unwrap_or(ref_id);
    let mut chars = id.chars();
    root.join(format!(
        "{}{}.ndjson",
        chars.next().unwrap_or('x'),
        chars.next().unwrap_or('x')
    ))
}

fn ref_index_lock_path(shard: &Path) -> PathBuf {
    append_file_name_suffix(shard, ".lock")
}

fn ref_index_timestamp() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn ref_index_text(path: &Path) -> Option<String> {
    read_limited_utf8(
        fs::File::open(path).ok()?,
        (REF_INDEX_MAX_BYTES as usize).saturating_mul(4),
    )
    .ok()
    .flatten()
}

fn parsed_ref_index_entries(text: &str) -> impl Iterator<Item = RefIndexEntry> + '_ {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map_while(|line| serde_json::from_str(line).ok())
}

fn ref_index_store_path(store_path: &Path) -> Option<PathBuf> {
    store_path
        .canonicalize()
        .or_else(|_| Ok::<_, std::io::Error>(store_path.to_path_buf()))
        .ok()
}

fn ref_index_root_store(store_path: &Path) -> Option<(PathBuf, PathBuf)> {
    Some((ref_index_root()?, ref_index_store_path(store_path)?))
}

fn locked_ref_index_shard(root: &Path, ref_id: &str) -> Option<(PathBuf, PersistLock)> {
    let shard = ref_index_shard_path(root, ref_id);
    PersistLock::acquire_with_retries(ref_index_lock_path(&shard), LOCK_RETRIES)
        .ok()
        .map(|lock| (shard, lock))
}

fn compact_ref_index_if_needed(shard: &Path) {
    if fs::metadata(shard).is_ok_and(|meta| meta.len() > REF_INDEX_MAX_BYTES) {
        let _ = compact_ref_index_shard(shard);
    }
}

fn append_blob_refs_to_ref_index(
    store_path: &Path,
    refs: &[String],
    classes: Option<&BTreeMap<String, ContentClass>>,
) {
    let Some((root, store_path)) = ref_index_root_store(store_path) else {
        return;
    };
    let mut refs = refs
        .iter()
        .filter(|ref_id| ref_id.starts_with("tz://blob/"))
        .peekable();
    if refs.peek().is_none() || create_ref_index_dir(&root).is_err() {
        return;
    }
    let ts = ref_index_timestamp();
    for ref_id in refs {
        let Some((shard, _lock)) = locked_ref_index_shard(&root, ref_id) else {
            continue;
        };
        if newest_ref_index_store_path(&shard, ref_id).as_deref()
            == Some(store_path.to_string_lossy().as_ref())
        {
            continue;
        }
        let class = classes
            .and_then(|classes| classes.get(ref_id))
            .copied()
            .unwrap_or_else(|| classify_ref(ref_id, None));
        if append_ref_index_line(&shard, ref_id, &store_path, ts, class, false, 0, None).is_ok() {
            compact_ref_index_if_needed(&shard);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn append_ref_index_line(
    shard: &Path,
    ref_id: &str,
    store_path: &Path,
    ts: u128,
    content_class: ContentClass,
    expanded: bool,
    expansion_count: u64,
    last_expanded_ts: Option<u128>,
) -> Result<(), RecoveryError> {
    let Some(parent) = shard.parent() else {
        return Ok(());
    };
    create_ref_index_dir(parent)?;
    let entry = RefIndexEntry {
        ref_id: ref_id.to_string(),
        store_path: store_path.to_string_lossy().into_owned(),
        ts,
        content_class,
        expanded,
        expansion_count,
        last_expanded_ts,
    };
    let mut line = serde_json::to_string(&entry)?;
    line.push('\n');
    private_open_options()
        .create(true)
        .append(true)
        .open(shard)?
        .write_all(line.as_bytes())?;
    Ok(())
}

fn open_optional_file(path: &Path) -> Result<Option<fs::File>, RecoveryError> {
    match fs::File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn compact_ref_index_shard(shard: &Path) -> Result<(), RecoveryError> {
    let Some(file) = open_optional_file(shard)? else {
        return Ok(());
    };
    let Some(text) = read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))?
    else {
        return Ok(());
    };
    write_ref_index_entries(shard, newest_ref_index_entries(&text, None).values())
}

fn prune_ref_index_stale_entries(ref_id: &str, stale: &HashSet<String>) {
    if stale.is_empty() {
        return;
    }
    let Some(root) = ref_index_root() else { return };
    let Some((shard, _lock)) = locked_ref_index_shard(&root, ref_id) else {
        return;
    };
    let Some(text) = ref_index_text(&shard) else {
        return;
    };
    let entries: Vec<_> = parsed_ref_index_entries(&text)
        .filter(|entry| entry.ref_id != ref_id || !stale.contains(&entry.store_path))
        .collect();
    let _ = write_ref_index_entries(&shard, &entries);
}

fn newest_ref_index_store_path(shard: &Path, ref_id: &str) -> Option<String> {
    parsed_ref_index_entries(&ref_index_text(shard)?)
        .filter(|entry| entry.ref_id == ref_id)
        .fold(None, |newest, entry| {
            if newest
                .as_ref()
                .is_none_or(|current: &RefIndexEntry| entry.ts > current.ts)
            {
                Some(entry)
            } else {
                newest
            }
        })
        .map(|entry| entry.store_path)
}

fn ref_index_entries_for_ref(text: &str, ref_id: &str) -> Vec<RefIndexEntry> {
    let mut entries: Vec<_> = parsed_ref_index_entries(text)
        .filter(|entry| entry.ref_id == ref_id)
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.ts));
    entries
}

fn newest_ref_index_entries(text: &str, skip_ref: Option<&str>) -> BTreeMap<String, RefIndexEntry> {
    let mut entries = BTreeMap::new();
    for mut entry in
        parsed_ref_index_entries(text).filter(|entry| skip_ref != Some(entry.ref_id.as_str()))
    {
        match entries.entry(entry.ref_id.clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(entry);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let existing = slot.get_mut();
                if entry.ts >= existing.ts {
                    entry.expanded |= existing.expanded;
                    entry.expansion_count = entry.expansion_count.max(existing.expansion_count);
                    entry.last_expanded_ts = entry.last_expanded_ts.max(existing.last_expanded_ts);
                    slot.insert(entry);
                } else {
                    existing.expanded |= entry.expanded;
                    existing.expansion_count = existing.expansion_count.max(entry.expansion_count);
                    existing.last_expanded_ts =
                        existing.last_expanded_ts.max(entry.last_expanded_ts);
                }
            }
        }
    }
    entries
}

fn write_ref_index_entries<'a>(
    shard: &Path,
    entries: impl IntoIterator<Item = &'a RefIndexEntry>,
) -> Result<(), RecoveryError> {
    create_ref_index_dir(shard.parent().unwrap_or_else(|| Path::new(".")))?;
    let tmp = recovery_tmp_path(shard);
    {
        let mut file = create_private_new(&tmp)?;
        for entry in entries {
            serde_json::to_writer(&mut file, entry)?;
            file.write_all(b"\n")?;
        }
    }
    fs::rename(&tmp, shard).map_err(RecoveryError::from)
}

fn ref_index_blob_entries(ref_id: &str) -> Option<(PathBuf, Vec<RefIndexEntry>)> {
    let root = ref_index_root()?;
    let text = ref_index_text(&ref_index_shard_path(&root, ref_id))?;
    let entries = ref_index_entries_for_ref(&text, ref_id);
    Some((root, entries))
}

fn blob_reachable_in_ref_index(
    ref_id: &str,
    config: &RecoveryConfig,
    skip_store: Option<&Path>,
) -> bool {
    let Some((root, entries)) = ref_index_blob_entries(ref_id) else {
        return false;
    };
    if entries.is_empty() {
        return false;
    }
    if let Some(hash) = ref_index_id_part(ref_id) {
        if SharedCas::new(root).contains(hash) {
            return true;
        }
    }
    // One load_state per unique sibling store path for this lookup. Without
    // memoization, duplicate ref-index rows re-parse the same journal.
    let mut loaded: HashMap<PathBuf, bool> = HashMap::new();
    entries.iter().any(|entry| {
        let store_path = PathBuf::from(&entry.store_path);
        if skip_store.is_some_and(|skip| skip == store_path.as_path()) {
            return false;
        }
        *loaded.entry(store_path.clone()).or_insert_with(|| {
            store_path.is_file()
                && load_state(&store_path, config)
                    .ok()
                    .flatten()
                    .is_some_and(|state| state.blobs.contains_key(ref_id))
        })
    })
}

fn resolve_blob_from_ref_index(ref_id: &str, config: &RecoveryConfig) -> RefResolve {
    let Some((root, entries)) = ref_index_blob_entries(ref_id) else {
        return RefResolve::NotFound;
    };
    if !entries.is_empty() {
        if let Some(hash) = ref_index_id_part(ref_id) {
            match shared_cas_utf8(&SharedCas::new(root), hash) {
                Ok(content) => return RefResolve::Found(content),
                Err(None) | Err(Some(SharedCasError::Corruption)) => {
                    return RefResolve::DecodeFailed;
                }
                Err(_) => {}
            }
        }
    }
    let mut stale = HashSet::new();
    let mut loaded: HashMap<PathBuf, Option<RecoveryState>> = HashMap::new();
    for entry in entries {
        let store_path = PathBuf::from(&entry.store_path);
        if !store_path.is_file() {
            stale.insert(entry.store_path);
            continue;
        }
        let state = loaded
            .entry(store_path.clone())
            .or_insert_with(|| load_state(&store_path, config).ok().flatten());
        let resolved = state
            .as_ref()
            .and_then(|state| state.blobs.get(ref_id).cloned())
            .map(|value| resolve_blob_value(Some(&store_path), ref_id, &value));
        match resolved {
            Some(result @ (RefResolve::Found(_) | RefResolve::DecodeFailed)) => {
                prune_ref_index_stale_entries(ref_id, &stale);
                return result;
            }
            Some(RefResolve::Stale) => return RefResolve::Stale,
            Some(RefResolve::NotFound) | None => {
                stale.insert(entry.store_path);
            }
        }
    }
    prune_ref_index_stale_entries(ref_id, &stale);
    RefResolve::NotFound
}

fn record_ref_index_expanded(store_path: &Path, ref_id: &str, fallback: ContentClass) {
    let Some((root, store_path)) = ref_index_root_store(store_path) else {
        return;
    };
    let Some((shard, _lock)) = locked_ref_index_shard(&root, ref_id) else {
        return;
    };
    let existing = ref_index_text(&shard)
        .and_then(|text| ref_index_entries_for_ref(&text, ref_id).into_iter().next());
    let class = existing
        .as_ref()
        .map_or(fallback, |entry| entry.content_class);
    let expansion_count = existing.as_ref().map_or(1, |entry| {
        entry
            .expansion_count
            .max(u64::from(entry.expanded))
            .saturating_add(1)
    });
    let now = ref_index_timestamp();
    let _ = append_ref_index_line(
        &shard,
        ref_id,
        &store_path,
        now,
        class,
        true,
        expansion_count,
        Some(now),
    );
    compact_ref_index_if_needed(&shard);
}

/// Export per-content-class expansion rates from the per-user ref index.
/// Returns a JSON summary with total refs, expanded refs, and the expansion
/// rate for each content class. The `expanded` flag is sticky across sessions.
pub fn export_class_stats() -> serde_json::Value {
    const SCHEMA: &str = "tokenzero.recovery.class-stats.v1";
    let empty = || {
        serde_json::json!({
            "schema_version": SCHEMA,
            "classes": Vec::<serde_json::Value>::new(),
            "total_refs": 0,
            "total_expanded": 0,
        })
    };
    let Some(root) = ref_index_root() else {
        return empty();
    };
    let Ok(shards) = fs::read_dir(root) else {
        return empty();
    };
    let mut per_ref: BTreeMap<String, (u128, ContentClass, bool)> = BTreeMap::new();
    for shard in shards
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ndjson"))
    {
        let Some(text) = ref_index_text(&shard) else {
            continue;
        };
        for entry in parsed_ref_index_entries(&text) {
            let current = per_ref
                .entry(entry.ref_id)
                .or_insert((0, entry.content_class, false));
            current.2 |= entry.expanded;
            if entry.ts > current.0 {
                current.0 = entry.ts;
                current.1 = entry.content_class;
            }
        }
    }
    let mut totals: BTreeMap<ContentClass, (usize, usize)> = BTreeMap::new();
    for (_, class, expanded) in per_ref.values() {
        let counts = totals.entry(*class).or_default();
        counts.0 += 1;
        counts.1 += usize::from(*expanded);
    }
    let mut total_refs = 0usize;
    let mut total_expanded = 0usize;
    let classes = [
        ContentClass::SourceFile,
        ContentClass::Diff,
        ContentClass::ShellOutput,
        ContentClass::SearchHits,
        ContentClass::Doc,
        ContentClass::BinaryPreview,
        ContentClass::Unknown,
    ]
    .into_iter()
    .map(|class| {
        let (total, expanded) = totals.remove(&class).unwrap_or_default();
        total_refs += total;
        total_expanded += expanded;
        let rate = if total == 0 {
            0.0
        } else {
            expanded as f64 / total as f64
        };
        serde_json::json!({
            "content_class": class,
            "total": total,
            "expanded": expanded,
            "rate": rate,
        })
    })
    .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": SCHEMA,
        "classes": classes,
        "total_refs": total_refs,
        "total_expanded": total_expanded,
    })
}

fn load_state(
    path: &Path,
    config: &RecoveryConfig,
) -> Result<Option<RecoveryState>, RecoveryError> {
    let Some(file) = open_optional_file(path)? else {
        return Ok(None);
    };
    let meta = file.metadata()?;
    // Compare as u64 so a file larger than usize can't truncate and slip past
    // the load-size guard on 32-bit targets (which would risk an OOM on read).
    if !meta.is_file() || meta.len() > config.max_load_bytes as u64 {
        return Ok(None);
    }
    let Some(text) = read_limited_utf8(file, config.max_load_bytes)? else {
        return Ok(None);
    };
    let Ok(mut state) = serde_json::from_str::<RecoveryState>(&text) else {
        return Ok(None);
    };
    state.configure(config);
    Ok(Some(apply_journal(state, path, config)))
}

// Large blobs use verified content-addressed sidecars.
const BLOB_EXTERNALIZE_MIN_BYTES: usize = 64 * 1024;
const STREAM_READ_BUFFER_BYTES: usize = 64 * 1024;
const BLOB_MARKER_PREFIX: &str = "\u{0}tzx:v1:";

fn blob_sidecar_dir(cache_path: &Path) -> PathBuf {
    append_file_name_suffix(cache_path, ".blobs")
}

fn externalize_blob_value(cache_path: &Path, text: &str, hash: &str) -> Option<String> {
    if text.len() < BLOB_EXTERNALIZE_MIN_BYTES {
        return None;
    }
    let dir = blob_sidecar_dir(cache_path);
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{hash}.txt"));
    // Content-addressed: an existing file already holds these exact bytes.
    if !path.exists() {
        fs::write(&path, text).ok()?;
    }
    Some(format!("{BLOB_MARKER_PREFIX}{hash}:{}:", text.len()))
}

fn parse_blob_marker(value: &str) -> Option<(&str, usize)> {
    let rest = value.strip_prefix(BLOB_MARKER_PREFIX)?;
    let (hash, rest) = rest.split_at_checked(64)?;
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let len: usize = rest.strip_prefix(':')?.strip_suffix(':')?.parse().ok()?;
    Some((hash, len))
}

fn blob_value_len(value: &BlobEntry) -> usize {
    match value {
        BlobEntry::Inline(text) => parse_blob_marker(text).map_or(text.len(), |(_, len)| len),
        BlobEntry::FileRef { path, .. } => {
            std::mem::size_of::<BlobEntry>() + path.as_os_str().len()
        }
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    crate::shared_cas::lower_hex(bytes)
}

fn digest_hex(hasher: Sha256) -> String {
    encode_hex(hasher.finalize().as_ref())
}

fn invalid_data(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into())
}

fn finalize_utf8_digest(bytes: Vec<u8>, hasher: Sha256) -> std::io::Result<(String, String)> {
    Ok((
        String::from_utf8(bytes).map_err(|err| invalid_data(err.to_string()))?,
        digest_hex(hasher),
    ))
}

fn line_range_out_of_bounds(start: usize, end: usize, line_count: usize) -> bool {
    start == 0 || start > end || end > line_count
}

fn read_file_chunks<R: Read>(
    reader: &mut R,
    mut on_chunk: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut buffer = [0_u8; STREAM_READ_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        on_chunk(&buffer[..read])?;
    }
    Ok(())
}

fn read_utf8_hashed(path: &Path, expected_len: Option<usize>) -> std::io::Result<(String, String)> {
    let mut file = fs::File::open(path)?;
    let capacity = expected_len
        .or_else(|| file.metadata().ok()?.len().try_into().ok())
        .unwrap_or(STREAM_READ_BUFFER_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    let mut hasher = Sha256::new();
    read_file_chunks(&mut file, |chunk| {
        if expected_len.is_some_and(|len| bytes.len().saturating_add(chunk.len()) > len) {
            return Err(invalid_data("streamed payload exceeds its recorded length"));
        }
        hasher.update(chunk);
        bytes.extend_from_slice(chunk);
        Ok(())
    })?;
    if expected_len.is_some_and(|len| bytes.len() != len) {
        return Err(invalid_data(
            "streamed payload does not match its recorded length",
        ));
    }
    finalize_utf8_digest(bytes, hasher)
}

fn read_utf8_line_range_hashed(
    path: &Path,
    start_line: usize,
    end_line: usize,
) -> std::io::Result<(String, String)> {
    let mut file = fs::File::open(path)?;
    let mut selected = Vec::new();
    let mut hasher = Sha256::new();
    let mut line = 1_usize;
    let mut bytes_seen = 0_usize;
    let mut newline_count = 0_usize;
    let mut last_byte = None;
    read_file_chunks(&mut file, |chunk| {
        bytes_seen += chunk.len();
        let mut selected_from = None;
        for (index, byte) in chunk.iter().copied().enumerate() {
            if line >= start_line && line <= end_line && selected_from.is_none() {
                selected_from = Some(index);
            }
            if byte == b'\n' {
                newline_count += 1;
                if line == end_line {
                    if let Some(from) = selected_from.take() {
                        hasher.update(&chunk[from..=index]);
                        selected.extend_from_slice(&chunk[from..=index]);
                    }
                }
                line += 1;
            }
            last_byte = Some(byte);
        }
        if let Some(from) = selected_from {
            hasher.update(&chunk[from..]);
            selected.extend_from_slice(&chunk[from..]);
        }
        Ok(())
    })?;
    let line_count = if bytes_seen == 0 {
        0
    } else {
        newline_count + usize::from(last_byte != Some(b'\n'))
    };
    if line_range_out_of_bounds(start_line, end_line, line_count) {
        return Err(invalid_data("streamed line range is outside the source"));
    }
    finalize_utf8_digest(selected, hasher)
}

fn stored_source_path(stored: &StoredFile) -> Option<PathBuf> {
    let path_text = stored.path.as_deref()?;
    Some(
        stored
            .path_identity
            .as_deref()
            .and_then(path_from_identity_text)
            .unwrap_or_else(|| PathBuf::from(path_text)),
    )
}

fn resolve_found_if(ok: bool, text: String, fail: RefResolve) -> RefResolve {
    if ok { RefResolve::Found(text) } else { fail }
}

fn blob_ref_digest_matches(ref_id: &str, text: &str, sha256: &str) -> bool {
    ref_id.strip_prefix("tz://blob/").is_some_and(|hash| {
        if hash.len() == 64 {
            sha256 == hash
        } else {
            id_for('b', text) == hash
        }
    })
}

fn resolve_file_value(stored: &StoredFile) -> RefResolve {
    if !stored.source_backed {
        return RefResolve::Found(stored.text.clone());
    }
    let Some(path) = stored_source_path(stored) else {
        return RefResolve::DecodeFailed;
    };
    let Some(expected) = stored.source_fingerprint.as_ref() else {
        return RefResolve::DecodeFailed;
    };
    let Ok(expected_len) = usize::try_from(expected.size) else {
        return RefResolve::Stale;
    };
    let Ok((text, sha256)) = read_utf8_hashed(&path, Some(expected_len)) else {
        return RefResolve::Stale;
    };
    resolve_found_if(
        source_fingerprint_from_sha256(&path, &sha256).as_ref() == Some(expected),
        text,
        RefResolve::Stale,
    )
}

fn resolve_blob_value(cache_path: Option<&Path>, ref_id: &str, value: &BlobEntry) -> RefResolve {
    match value {
        BlobEntry::Inline(value) => {
            let Some((hash, expected_len)) = parse_blob_marker(value) else {
                return RefResolve::Found(value.clone());
            };
            let Some(cache_path) = cache_path else {
                return RefResolve::DecodeFailed;
            };
            let path = blob_sidecar_dir(cache_path).join(format!("{hash}.txt"));
            let Ok((text, actual_hash)) = read_utf8_hashed(&path, Some(expected_len)) else {
                return RefResolve::DecodeFailed;
            };
            resolve_found_if(actual_hash == hash, text, RefResolve::DecodeFailed)
        }
        BlobEntry::FileRef {
            path,
            source_start_line,
            source_end_line,
        } => {
            let Ok((text, sha256)) =
                read_utf8_line_range_hashed(path, *source_start_line, *source_end_line)
            else {
                return RefResolve::Stale;
            };
            resolve_found_if(
                blob_ref_digest_matches(ref_id, &text, &sha256),
                text,
                RefResolve::Stale,
            )
        }
    }
}

// Active journal sibling path.
fn journal_path(path: &Path) -> PathBuf {
    append_file_name_suffix(path, ".journal")
}

// Persisted session delta.
#[derive(Debug, Serialize, Deserialize)]
struct JournalEntry {
    refs: Vec<String>,
    state: RecoveryState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deleted_blob_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deleted_aliases: Vec<String>,
}

fn apply_deletions<'a>(
    state: &mut RecoveryState,
    blob_refs: impl IntoIterator<Item = &'a str>,
    aliases: impl IntoIterator<Item = &'a str>,
) {
    for alias in aliases {
        state.aliases.remove(alias);
        state.ambiguous_aliases.remove(alias);
    }
    for ref_id in blob_refs {
        state.blobs.remove(ref_id);
    }
}

fn recovery_unit_map<'a>(
    state: &'a RecoveryState,
    kind: &str,
) -> Option<&'a BTreeMap<String, StoredUnit>> {
    match kind {
        "unit" => Some(&state.units),
        "search" => Some(&state.search_hits),
        _ => None,
    }
}

fn state_entry_present(state: &RecoveryState, ref_id: &str) -> bool {
    recovery_maps!(contains state, ref_id)
}

fn session_delta(
    state: &RecoveryState,
    session_refs: &[String],
    config: &RecoveryConfig,
) -> RecoveryState {
    let mut delta = RecoveryState::empty(config);
    for ref_id in session_refs {
        recovery_maps!(copy delta, state, ref_id);
    }
    // These capped indexes are merged wholesale because they can change after
    // the persist that carried their target and have no session-ref identity.
    delta.aliases = state.aliases.clone();
    delta.shell_outcomes = state.shell_outcomes.clone();
    delta.shell_outcome_seq = state.shell_outcome_seq;
    delta.ambiguous_aliases = state.ambiguous_aliases.clone();
    delta.transparency = state.transparency.clone();
    delta.order = session_refs
        .iter()
        .filter(|ref_id| state_entry_present(&delta, ref_id))
        .cloned()
        .collect();
    delta
}

fn copy_map_entry<T: Clone>(
    destination: &mut BTreeMap<String, T>,
    source: &BTreeMap<String, T>,
    ref_id: &str,
) {
    if let Some(value) = source.get(ref_id) {
        destination.insert(ref_id.to_string(), value.clone());
    }
}

// Bound journal segments relative to the snapshot.
fn journal_compact_threshold(snapshot_len: u64) -> u64 {
    snapshot_len.max(64 * 1024)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalAppend {
    Appended,
    NeedsCompaction,
}

fn journal_segment_path(path: &Path, generation: usize) -> PathBuf {
    let mut os: OsString = journal_path(path).into_os_string();
    os.push(format!(".{generation}"));
    PathBuf::from(os)
}

fn remove_journal_segments(path: &Path) {
    let _ = fs::remove_file(journal_path(path));
    for generation in 1..=JOURNAL_MAX_SEALED_SEGMENTS {
        let _ = fs::remove_file(journal_segment_path(path, generation));
    }
}

fn append_journal(
    snapshot_path: &Path,
    entry: &JournalEntry,
    segment_limit: u64,
) -> Result<JournalAppend, RecoveryError> {
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let line_len = line.len() as u64;
    if line_len > segment_limit {
        return Ok(JournalAppend::NeedsCompaction);
    }

    let active = journal_path(snapshot_path);
    let active_len = fs::metadata(&active).map(|meta| meta.len()).unwrap_or(0);
    if active_len > 0 && active_len.saturating_add(line_len) > segment_limit {
        if journal_segment_path(snapshot_path, JOURNAL_MAX_SEALED_SEGMENTS).exists() {
            return Ok(JournalAppend::NeedsCompaction);
        }
        for generation in (1..JOURNAL_MAX_SEALED_SEGMENTS).rev() {
            let from = journal_segment_path(snapshot_path, generation);
            if from.exists() {
                fs::rename(from, journal_segment_path(snapshot_path, generation + 1))?;
            }
        }
        fs::rename(&active, journal_segment_path(snapshot_path, 1))?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(active)?;
    file.write_all(line.as_bytes())?;
    Ok(JournalAppend::Appended)
}

/// Fail-open capped journal segment read: `None` stops replay; empty means missing segment.
/// Fail-open capped journal segment read: `None` stops replay; `Some(None)` skips a missing segment.
fn read_capped_journal_text(path: &Path, remaining: &mut u64) -> Option<Option<String>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    let Ok(meta) = file.metadata() else {
        return None;
    };
    if !meta.is_file() || meta.len() > *remaining {
        return None;
    }
    *remaining -= meta.len();
    match read_limited_utf8(file, meta.len() as usize) {
        Ok(Some(text)) => Some(Some(text)),
        _ => None,
    }
}

// Replay complete journal entries; any bad tail fails open to recovered state.
fn apply_journal(mut state: RecoveryState, path: &Path, config: &RecoveryConfig) -> RecoveryState {
    let journals = (1..=JOURNAL_MAX_SEALED_SEGMENTS)
        .rev()
        .map(|generation| journal_segment_path(path, generation))
        .chain(std::iter::once(journal_path(path)));
    let mut remaining = config.max_load_bytes as u64;
    for journal in journals {
        let Some(maybe_text) = read_capped_journal_text(&journal, &mut remaining) else {
            return state;
        };
        let Some(text) = maybe_text else { continue };
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let Ok(entry) = serde_json::from_str::<JournalEntry>(line) else {
                return state;
            };
            let JournalEntry {
                refs,
                state: delta,
                deleted_blob_refs,
                deleted_aliases,
            } = entry;
            let accumulated = std::mem::replace(&mut state, RecoveryState::empty(config));
            state = merge_states(accumulated, delta, &refs, config);
            apply_deletions(
                &mut state,
                deleted_blob_refs.iter().map(String::as_str),
                deleted_aliases.iter().map(String::as_str),
            );
        }
    }
    state
}

fn read_limited_utf8<R: Read>(
    reader: R,
    max_load_bytes: usize,
) -> Result<Option<String>, RecoveryError> {
    let mut limited = reader.take((max_load_bytes as u64).saturating_add(1));
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes)?;
    if bytes.len() > max_load_bytes {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

fn merge_map_entries<T>(
    session: &HashSet<&str>,
    dst: &mut BTreeMap<String, T>,
    src: BTreeMap<String, T>,
) {
    for (ref_id, value) in src {
        if session.contains(ref_id.as_str()) || dst.contains_key(&ref_id) {
            dst.insert(ref_id, value);
        }
    }
}

fn trim_shell_outcomes(outcomes: &mut BTreeMap<String, ShellOutcome>) {
    while outcomes.len() > MAX_SHELL_OUTCOMES {
        let Some(victim) = outcomes
            .iter()
            .min_by_key(|(_, outcome)| outcome.seq)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        outcomes.remove(&victim);
    }
}

fn merge_states(
    existing: RecoveryState,
    current: RecoveryState,
    session_refs: &[String],
    config: &RecoveryConfig,
) -> RecoveryState {
    let session: HashSet<&str> = session_refs.iter().map(String::as_str).collect();
    let mut merged = existing;
    recovery_maps!(merge & session, merged, current);
    merged.aliases.extend(current.aliases);
    if current.ordinal_generation > merged.ordinal_generation {
        merged.ordinal_generation = current.ordinal_generation;
        merged.next_ordinal = current.next_ordinal;
    } else if current.ordinal_generation == merged.ordinal_generation {
        merged.next_ordinal = merged.next_ordinal.max(current.next_ordinal);
    }
    merged.order.extend(session_refs.iter().cloned());
    let mut seen = HashSet::new();
    merged.order.retain(|ref_id| seen.insert(ref_id.clone()));
    merged.shell_outcome_seq = merged.shell_outcome_seq.max(current.shell_outcome_seq);
    merged.shell_outcomes.extend(current.shell_outcomes);
    trim_shell_outcomes(&mut merged.shell_outcomes);
    merged.ambiguous_aliases.extend(current.ambiguous_aliases);
    merged.transparency.merge_concurrent(&current.transparency);
    merged.configure(config);
    merged
}

fn evict_prefix<T>(
    items: &mut BTreeMap<String, T>,
    order: &mut Vec<String>,
    prefix: &str,
    limit: usize,
) {
    let excess = items.len().saturating_sub(limit);
    if excess == 0 {
        return;
    }
    let mut victims = HashSet::with_capacity(excess);
    for ref_id in order
        .iter()
        .filter(|ref_id| ref_id.starts_with(prefix) && items.contains_key(*ref_id))
    {
        victims.insert(ref_id.clone());
        if victims.len() == excess {
            break;
        }
    }
    if victims.len() < excess {
        for ref_id in items.keys() {
            victims.insert(ref_id.clone());
            if victims.len() == excess {
                break;
            }
        }
    }
    items.retain(|ref_id, _| !victims.contains(ref_id));
    order.retain(|ref_id| !victims.contains(ref_id));
}
fn private_open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn create_private_new(path: &Path) -> std::io::Result<fs::File> {
    private_open_options()
        .write(true)
        .create_new(true)
        .open(path)
}

fn atomic_write_json(path: &Path, state: &RecoveryState) -> Result<(), RecoveryError> {
    // A same-directory rename publishes either the old or complete new cache.
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut last_collision = None;
    for _ in 0..TMP_RETRIES {
        let tmp = recovery_tmp_path(path);
        match write_json_to_tmp(&tmp, state) {
            Ok(()) => {
                if let Err(err) = fs::rename(&tmp, path) {
                    let _ = fs::remove_file(&tmp);
                    return Err(err.into());
                }
                return Ok(());
            }
            Err(RecoveryError::Io(err)) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(err);
            }
            Err(err) => {
                let _ = fs::remove_file(&tmp);
                return Err(err);
            }
        }
    }
    Err(last_collision
        .unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "could not allocate recovery temp file for {} after {TMP_RETRIES} attempts",
                    path.display()
                ),
            )
        })
        .into())
}

fn write_json_to_tmp(tmp: &Path, state: &RecoveryState) -> Result<(), RecoveryError> {
    // Buffered serialization avoids per-fragment writes; the cache is reconstructible.
    let file = create_private_new(tmp)?;
    let mut writer = std::io::BufWriter::with_capacity(1 << 20, file);
    serde_json::to_writer(&mut writer, state)?;
    writer.write_all(b"\n")?;
    let _file = writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?;
    Ok(())
}

fn recovery_file_ref(text: &str, path: Option<&Path>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.map(path_identity_text).unwrap_or_default());
    hasher.update(b":");
    hasher.update(text.as_bytes());
    format!("tz://file/f{}", &digest_hex(hasher)[..16])
}

macro_rules! path_identity_platform {
    (unix) => {
        fn path_identity_text(path: &Path) -> String {
            use std::os::unix::ffi::OsStrExt;
            format!("unix:{}", encode_hex(path.as_os_str().as_bytes()))
        }
        fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
            use std::os::unix::ffi::OsStringExt;
            Some(PathBuf::from(OsString::from_vec(decode_hex_bytes(
                identity.strip_prefix("unix:")?,
            )?)))
        }
    };
    (windows) => {
        fn path_identity_text(path: &Path) -> String {
            use std::os::windows::ffi::OsStrExt;
            let mut bytes = Vec::new();
            for unit in path.as_os_str().encode_wide() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
            format!("windows:{}", encode_hex(&bytes))
        }
        fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
            use std::os::windows::ffi::OsStringExt;
            let bytes = decode_hex_bytes(identity.strip_prefix("windows:")?)?;
            let mut chunks = bytes.chunks_exact(2);
            let units: Vec<u16> = chunks
                .by_ref()
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect();
            if !chunks.remainder().is_empty() {
                return None;
            }
            Some(PathBuf::from(OsString::from_wide(&units)))
        }
    };
    (other) => {
        fn path_identity_text(path: &Path) -> String {
            format!("display:{}", path.to_string_lossy())
        }
        fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
            Some(PathBuf::from(identity.strip_prefix("display:")?))
        }
    };
}
#[cfg(unix)]
path_identity_platform!(unix);
#[cfg(windows)]
path_identity_platform!(windows);
#[cfg(not(any(unix, windows)))]
path_identity_platform!(other);

fn decode_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chunks = hex.as_bytes().chunks_exact(2);
    for pair in chunks.by_ref() {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        bytes.push((high << 4) | low);
    }
    if !chunks.remainder().is_empty() {
        return None;
    }
    Some(bytes)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn recovery_lock_path(path: &Path) -> PathBuf {
    append_file_name_suffix(path, ".lock")
}

fn recovery_tmp_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_name = OsString::from(".");
    tmp_name.push(
        path.file_name()
            .map(OsString::from)
            .unwrap_or_else(|| OsString::from("recovery")),
    );
    let nonce = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    tmp_name.push(format!(".{}.{nonce}.tmp", std::process::id()));
    parent.join(tmp_name)
}

/// Age after which an abandoned atomic-write temp file is reclaimable. A
/// live persist holds its temp file for milliseconds; an hour-old one belongs
/// to a process that died mid-write.
pub const STALE_TMP_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// Outcome of a stale temp-file sweep. With `dry_run`, `removed*` counts what
/// would be reclaimed without unlinking anything.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TmpSweepReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub removed: usize,
    pub removed_bytes: u64,
    pub failed: usize,
}

/// Remove matching recovery temp files older than `max_age`.
/// Dry runs report candidates; per-file failures are counted and fail open.
pub fn sweep_stale_tmp_files(
    cache_path: &Path,
    max_age: Duration,
    dry_run: bool,
) -> TmpSweepReport {
    let mut report = TmpSweepReport {
        dry_run,
        ..TmpSweepReport::default()
    };
    let Some((parent, cache_name)) = cache_path
        .parent()
        .zip(cache_path.file_name().and_then(|name| name.to_str()))
    else {
        return report;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return report;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".tmp") || !name.contains(cache_name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        report.scanned += 1;
        let expired = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > max_age);
        if !expired {
            continue;
        }
        if dry_run || fs::remove_file(path).is_ok() {
            report.removed += 1;
            report.removed_bytes += meta.len();
        } else {
            report.failed += 1;
        }
    }
    report
}

fn append_file_name_suffix(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("recovery"));
    file_name.push(suffix);
    parent.join(file_name)
}

struct PersistLock {
    file: fs::File,
}

impl PersistLock {
    fn acquire(path: PathBuf) -> Result<Self, RecoveryError> {
        Self::acquire_with_retries(path, LOCK_RETRIES)
    }

    fn acquire_with_retries(path: PathBuf, retries: usize) -> Result<Self, RecoveryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        for attempt in 0..retries {
            match FileExt::try_lock(&file) {
                Ok(()) => {
                    file.set_len(0)?;
                    writeln!(file, "{}", std::process::id())?;
                    return Ok(Self { file });
                }
                Err(TryLockError::Error(err)) if err.kind() != std::io::ErrorKind::WouldBlock => {
                    return Err(err.into());
                }
                Err(_) if attempt + 1 < retries => thread::sleep(LOCK_RETRY_DELAY),
                Err(_) => {}
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timed out waiting for lock {}", path.display()),
        )
        .into())
    }
}

impl Drop for PersistLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn is_ephemeral_source_path(path_text: &str) -> bool {
    path_text.starts_with("shell:") || path_text.starts_with("search:")
}

fn fingerprint_for_stored_payload(
    path: Option<&Path>,
    source_start_line: Option<usize>,
    source_end_line: Option<usize>,
) -> Option<SourceFingerprint> {
    if source_start_line.is_some() || source_end_line.is_some() {
        return None;
    }
    let path = path?;
    if is_ephemeral_source_path(&path.to_string_lossy()) {
        return None;
    }
    source_fingerprint(path)
}

fn fingerprint_from_meta(meta: &fs::Metadata, sha256: String) -> SourceFingerprint {
    SourceFingerprint {
        size: meta.len(),
        mtime_ns: mtime_ns(meta),
        sha256,
    }
}

fn source_fingerprint_from_sha256(path: &Path, sha256: &str) -> Option<SourceFingerprint> {
    Some(fingerprint_from_meta(&file_meta(path)?, sha256.to_string()))
}

fn hash_file_sha256(path: &Path) -> Option<(fs::Metadata, String)> {
    let meta = file_meta(path)?;
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    read_file_chunks(&mut file, |chunk| {
        hasher.update(chunk);
        Ok(())
    })
    .ok()?;
    Some((meta, digest_hex(hasher)))
}

fn source_fingerprint(path: &Path) -> Option<SourceFingerprint> {
    let (meta, sha256) = hash_file_sha256(path)?;
    Some(fingerprint_from_meta(&meta, sha256))
}

fn file_meta(path: &Path) -> Option<fs::Metadata> {
    let meta = fs::metadata(path).ok()?;
    meta.is_file().then_some(meta)
}

fn mtime_ns(meta: &fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod select_content_tests {
    use super::*;

    #[test]
    fn selector_line_windows_override_existing_line_args() {
        let content = "one\ntwo\nthree\nfour\nfive\n".to_string();

        assert_eq!(
            select_content(
                content.clone(),
                Some("range:2-3"),
                Some(5),
                Some(5),
                None,
                None
            ),
            "two\nthree\n"
        );
        assert_eq!(
            select_content(
                content.clone(),
                Some("lines:L3-L4"),
                Some(5),
                Some(5),
                None,
                None
            ),
            "three\nfour\n"
        );
        assert_eq!(
            select_content(
                content.clone(),
                Some("line:4"),
                Some(5),
                Some(5),
                None,
                None
            ),
            "four\n"
        );
        assert_eq!(
            select_content(content, Some("around:3:1"), Some(5), Some(5), None, None),
            "two\nthree\nfour\n"
        );
    }
}

/// Result of enforcing the legacy recovery sidecar byte budget.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobSidecarPruneReport {
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub removed_files: usize,
    pub retained_referenced: usize,
}

/// Remove oldest unreferenced legacy blob sidecars until the budget is met.
/// Sidecars referenced by the authoritative snapshot or journal are retained.
pub fn prune_blob_sidecars(
    cache_path: &Path,
    max_bytes: u64,
    dry_run: bool,
) -> Result<BlobSidecarPruneReport, RecoveryError> {
    let _lock = PersistLock::acquire(recovery_lock_path(cache_path))?;
    let config = RecoveryConfig::default();
    let state = load_state(cache_path, &config)?.unwrap_or_else(|| RecoveryState::empty(&config));
    let referenced: HashSet<String> = state
        .blobs
        .values()
        .filter_map(|entry| match entry {
            BlobEntry::Inline(value) => parse_blob_marker(value).map(|(hash, _)| hash.to_string()),
            BlobEntry::FileRef { .. } => None,
        })
        .collect();
    let directory = blob_sidecar_dir(cache_path);
    let mut files = Vec::new();
    let mut bytes_before = 0_u64;
    let mut retained_referenced = 0_usize;
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if !metadata.is_file() {
                    continue;
                }
                let path = entry.path();
                let Some(hash) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let len = metadata.len();
                bytes_before = bytes_before.saturating_add(len);
                if referenced.contains(hash) {
                    retained_referenced += 1;
                } else {
                    files.push((metadata.modified().unwrap_or(UNIX_EPOCH), path, len));
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut bytes_after = bytes_before;
    let mut removed_files = 0_usize;
    for (_, path, len) in files {
        if bytes_after <= max_bytes {
            break;
        }
        if !dry_run {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        bytes_after = bytes_after.saturating_sub(len);
        removed_files += 1;
    }
    Ok(BlobSidecarPruneReport {
        bytes_before,
        bytes_after,
        removed_files,
        retained_referenced,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryBlobPruneReport {
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub freed_bytes: u64,
    pub removed_files: usize,
    pub removed_referenced: usize,
    pub expired_files: usize,
    pub max_bytes: u64,
    pub max_age_seconds: u64,
    pub dry_run: bool,
}

/// Enforce byte and age bounds over the complete recovery sidecar store.
/// Never-expanded blobs are selected before expanded blobs; expanded blobs use
/// their durable ref-index last-expand timestamp as the LRU key.
pub fn prune_recovery_blobs(
    cache_path: &Path,
    max_bytes: u64,
    max_age: Duration,
    dry_run: bool,
) -> Result<RecoveryBlobPruneReport, RecoveryError> {
    let directory = blob_sidecar_dir(cache_path);
    let mut store = RecoveryStore::new(Some(cache_path.to_path_buf()));
    let mut files = Vec::new();
    let mut bytes_before = 0_u64;
    let now = SystemTime::now();
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if !metadata.is_file() {
                    continue;
                }
                let path = entry.path();
                let Some(hash) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let ref_id = format!("tz://blob/{hash}");
                let len = metadata.len();
                bytes_before = bytes_before.saturating_add(len);
                let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
                let expired = now.duration_since(modified).unwrap_or_default() >= max_age;
                let (expansion_count, last_expanded) = ref_index_blob_entries(&ref_id)
                    .map(|(_, entries)| {
                        entries.into_iter().fold((0_u64, 0_u128), |acc, item| {
                            (
                                acc.0
                                    .max(item.expansion_count.max(u64::from(item.expanded))),
                                acc.1.max(item.last_expanded_ts.unwrap_or(0)),
                            )
                        })
                    })
                    .unwrap_or_default();
                let referenced = store.state.blobs.contains_key(&ref_id);
                files.push((
                    !expired,
                    expansion_count > 0,
                    last_expanded,
                    modified,
                    path,
                    ref_id,
                    len,
                    referenced,
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    files.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
            .then(left.3.cmp(&right.3))
            .then(left.5.cmp(&right.5))
    });
    let mut bytes_after = bytes_before;
    let mut victims = Vec::new();
    let mut expired_files = 0_usize;
    for (not_expired, _, _, _, path, ref_id, len, referenced) in files {
        if not_expired && bytes_after <= max_bytes {
            continue;
        }
        if !not_expired {
            expired_files += 1;
        }
        bytes_after = bytes_after.saturating_sub(len);
        victims.push((path, ref_id, len, referenced));
    }
    let removed_referenced = victims.iter().filter(|item| item.3).count();
    if !dry_run {
        for (_, ref_id, _, referenced) in &victims {
            if *referenced {
                let aliases: Vec<_> = store
                    .state
                    .aliases
                    .iter()
                    .filter(|(_, target)| *target == ref_id)
                    .map(|(alias, _)| alias.clone())
                    .collect();
                for alias in aliases {
                    store.remove_alias(&alias);
                }
                store.remove_blob(ref_id);
            }
        }
        store.persist_pending()?;
        for (path, _, _, _) in &victims {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(RecoveryBlobPruneReport {
        bytes_before,
        bytes_after,
        freed_bytes: bytes_before.saturating_sub(bytes_after),
        removed_files: victims.len(),
        removed_referenced,
        expired_files,
        max_bytes,
        max_age_seconds: max_age.as_secs(),
        dry_run,
    })
}

pub fn recovery_blob_status(cache_path: &Path) -> serde_json::Value {
    let bytes = fs::read_dir(blob_sidecar_dir(cache_path))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .fold(0_u64, |total, metadata| {
            total.saturating_add(metadata.len())
        });
    serde_json::json!({"bytes": bytes, "freed_bytes": 0, "path": blob_sidecar_dir(cache_path)})
}
