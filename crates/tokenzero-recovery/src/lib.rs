#![forbid(unsafe_code)]

use fs4::{FileExt, TryLockError};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokenzero_core::{ContentType, count_tokens, error_block, id_for, sha256_hex, symbol_block};

use crate::shared_cas::{SharedCas, SharedCasError};

pub mod migration;
pub mod shared_cas;

const LOCK_RETRIES: usize = 240;
const MAX_SHELL_OUTCOMES: usize = 256;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const TMP_RETRIES: usize = 16;
const REF_INDEX_MAX_BYTES: u64 = 1_048_576;
const REF_INDEX_DISABLE_ENV: &str = "TOKENZERO_REF_INDEX";
const REF_INDEX_PATH_ENV: &str = "TOKENZERO_REF_INDEX_PATH";

/// ZeroStack schemes accepted by expand. Full-hash portable blob refs first use the
/// configured canonical shared CAS. Legacy short blob refs retain a clearly separated
/// same-store alias tier when no shared object is available.
pub const EXPAND_REF_SCHEMES: &[&str] = &["tz://", "fz://", "gz://"];

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
    if let Some(rest) = ref_id.strip_prefix("fz://blob/") {
        return Some(format!("tz://blob/{rest}"));
    }
    if let Some(rest) = ref_id.strip_prefix("gz://blob/") {
        return Some(format!("tz://blob/{rest}"));
    }
    None
}

fn is_legacy_same_store_blob_ref(ref_id: &str) -> bool {
    let bare = ref_id.split_once('#').map_or(ref_id, |(bare, _)| bare);
    let hash = ["tz://blob/", "fz://blob/", "gz://blob/"]
        .iter()
        .find_map(|prefix| bare.strip_prefix(prefix));
    hash.is_some_and(|hash| {
        hash.len() == 17
            && hash.starts_with('b')
            && hash[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// `file:line[:col]` matcher for search-output ingestion. Compiled once on first
/// use — the pattern is a compile-time literal, so `expect` can only fire on a
/// programmer typo (caught by the unit tests below), never on user input. The
/// previous code rebuilt this regex on every `store_search_output` call.
static SEARCH_PATH_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<path>(?:[\w.@+-]+/)+[\w.@+-]+):(?P<line>\d+):?(?P<col>\d+)?")
        .expect("SEARCH_PATH_LINE is a valid compile-time regex literal")
});
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// ZeroRef v1 contract error taxonomy for portable blob refs.
/// Stable error labels used by `parse_zeroref_v1_blob` and the v1 test suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZeroRefError {
    /// Ref string is not structurally a ZeroRef v1 blob ref.
    Malformed,
    /// Scope is not a portable blob ref (e.g. file, unit, session, execution, index).
    Unsupported,
    /// Object missing from the store (not found after a valid parse).
    Missing,
    /// Underlying storage or network read failed.
    Io,
    /// Complete-object digest verification failed.
    Corruption,
    /// Policy denied access/expansion.
    Policy,
    /// Ref version is incompatible with this consumer.
    IncompatibleVersion,
    /// Legacy short/prefix ID cannot be disambiguated under v1 rules.
    LegacyAmbiguity,
}

impl std::fmt::Display for ZeroRefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Malformed => "malformed",
            Self::Unsupported => "unsupported",
            Self::Missing => "missing",
            Self::Io => "io",
            Self::Corruption => "corruption",
            Self::Policy => "policy",
            Self::IncompatibleVersion => "incompatible_version",
            Self::LegacyAmbiguity => "legacy_ambiguity",
        };
        write!(f, "{label}")
    }
}

impl std::error::Error for ZeroRefError {}

/// Typed error for `#B`/`#L` fragment parsing and evaluation (cqr.5).
///
/// Used by `parse_fragment_spec` and the `expand` fragment path to surface
/// structured rejection instead of falling back to the full payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FragmentError {
    /// Fragment string is not structurally valid (non-numeric, missing `-`, empty).
    Malformed,
    /// `start > end` for a `#B` or `#L` range.
    Reversed,
    /// `end > content.len()` (`#B`) or `start/end > line_count` (`#L`).
    OutOfRange,
    /// `#L` requested on content whose line bytes are not valid UTF-8.
    /// The current String-based store cannot produce this, but the contract
    /// requires the variant so future raw-byte stores can signal it without
    /// changing the error type.
    NonUtf8Line,
    /// Fragment kind is not `B` or `L`.
    UnknownKind,
    /// Fragment string contains a second `#` (e.g. `#B0-5#L1-3`).
    DuplicateFragment,
}

impl std::fmt::Display for FragmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Malformed => "malformed",
            Self::Reversed => "reversed",
            Self::OutOfRange => "out_of_range",
            Self::NonUtf8Line => "non_utf8_line_fragment",
            Self::UnknownKind => "unknown_kind",
            Self::DuplicateFragment => "duplicate_fragment",
        };
        write!(f, "{label}")
    }
}

impl std::error::Error for FragmentError {}

/// Internal parsed fragment spec produced by `parse_fragment_spec`.
enum FragmentSpec {
    /// Zero-based half-open byte range `start..end`.
    Byte { start: usize, end: usize },
    /// One-based inclusive line range `start..=end`.
    Line { start: usize, end: usize },
}

/// Parse a `#B`/`#L` fragment string into a [`FragmentSpec`].
///
/// Fragment syntax (after the leading `#` stripped by `parse_ref`):
/// - `Bstart-end` — zero-based half-open byte range; `start == end` allowed.
/// - `Bn`         — single byte position `n..n` (empty).
/// - `Lstart-end` — one-based inclusive line range.
/// - `Ln`         — single line `n`.
///
/// Reversed (`start > end`), zero-line (`#L0`), duplicate (`#` inside
/// fragment), unknown kind, and non-numeric values are rejected with typed
/// [`FragmentError`]s. Out-of-range checks that need content length are
/// deferred to the caller.
fn parse_fragment_spec(fragment: &str) -> Result<FragmentSpec, FragmentError> {
    if fragment.is_empty() {
        return Err(FragmentError::Malformed);
    }
    if fragment.contains('#') {
        return Err(FragmentError::DuplicateFragment);
    }
    let kind = &fragment[..1];
    let rest = &fragment[1..];
    match kind {
        "B" => {
            let (start_str, end_str) = rest
                .split_once('-')
                .or_else(|| rest.split_once(','))
                .unwrap_or((rest, rest));
            let start = start_str
                .trim_start_matches('B')
                .parse::<usize>()
                .map_err(|_| FragmentError::Malformed)?;
            let end = end_str
                .trim_start_matches('B')
                .parse::<usize>()
                .map_err(|_| FragmentError::Malformed)?;
            if start > end {
                return Err(FragmentError::Reversed);
            }
            Ok(FragmentSpec::Byte { start, end })
        }
        "L" => {
            let (start_str, end_str) = rest.split_once('-').unwrap_or((rest, rest));
            let start = start_str
                .trim_start_matches('L')
                .parse::<usize>()
                .map_err(|_| FragmentError::Malformed)?;
            let end = end_str
                .trim_start_matches('L')
                .parse::<usize>()
                .map_err(|_| FragmentError::Malformed)?;
            if start == 0 {
                return Err(FragmentError::Malformed);
            }
            if start > end {
                return Err(FragmentError::Reversed);
            }
            Ok(FragmentSpec::Line { start, end })
        }
        _ => Err(FragmentError::UnknownKind),
    }
}

/// Stable reason string for a [`FragmentError`] used in `ExpansionResult::reason`.
fn fragment_error_reason(err: FragmentError) -> String {
    match err {
        FragmentError::Malformed => "fragment-malformed".to_string(),
        FragmentError::Reversed => "fragment-reversed".to_string(),
        FragmentError::OutOfRange => "fragment-out-of-range".to_string(),
        FragmentError::NonUtf8Line => "non_utf8_line_fragment".to_string(),
        FragmentError::UnknownKind => "fragment-unknown-kind".to_string(),
        FragmentError::DuplicateFragment => "fragment-duplicate".to_string(),
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

/// Parse and validate a ZeroRef v1 portable blob ref.
///
/// Portable scope is `(tz|fz|gz)://blob/<full-hash>` only. Execution/error/session/file/graph/index
/// refs remain engine-specific and are rejected with `ZeroRefError::Unsupported`.
///
/// Identity is the full lowercase 64-hex SHA-256 of the complete unfragmented bytes. The parser
/// emits the full hash and rejects short, prefix, uppercase, non-hex, or extra-segment IDs.
///
/// Fragments:
/// - `#Bstart-end`: zero-based half-open byte range, checked arithmetic; `start == end` allowed;
///   reversed (`start > end`) or `end > byte_length` rejected with `Malformed`.
/// - `#Lstart-end`: one-based inclusive line range, exact newline retention; `start == 0`,
///   reversed, or out-of-bounds rejected with `Malformed`.
///
/// Legacy 17-character short IDs (prefix + 8 hex bytes) are rejected by this v1 parser; callers
/// that need backward compatibility should detect `LegacyAmbiguity` and fall back to the existing
/// `parse_ref`/`canonicalize_expand_ref` path.
pub fn parse_zeroref_v1_blob(
    ref_id: &str,
    byte_length: Option<usize>,
) -> Result<ZeroRefV1Blob, ZeroRefError> {
    let (bare, fragment_str) = ref_id
        .split_once('#')
        .map_or((ref_id, None), |(b, f)| (b, Some(f)));

    // Scheme must be one of the three portable blob schemes and the path must be exactly `blob/<hash>`.
    let scheme = if bare.starts_with("tz://blob/") {
        "tz"
    } else if bare.starts_with("fz://blob/") {
        "fz"
    } else if bare.starts_with("gz://blob/") {
        "gz"
    } else {
        return Err(ZeroRefError::Unsupported);
    };
    let hash = bare
        .strip_prefix(&format!("{scheme}://blob/"))
        .expect("prefix checked above");

    if hash.is_empty() {
        return Err(ZeroRefError::Malformed);
    }
    if hash.contains('/') {
        return Err(ZeroRefError::Malformed);
    }
    if hash.len() != 64 {
        // 17-char short IDs (prefix + 8 hex bytes) are a legacy format, not v1.
        return Err(ZeroRefError::LegacyAmbiguity);
    }
    if hash
        .chars()
        .any(|c| !c.is_ascii_hexdigit() || c.is_ascii_uppercase())
    {
        return Err(ZeroRefError::Malformed);
    }

    let fragment = fragment_str
        .map(|f| parse_zeroref_v1_fragment(f, byte_length))
        .transpose()?;

    Ok(ZeroRefV1Blob {
        scheme: scheme.to_string(),
        hash: hash.to_string(),
        fragment,
    })
}

fn parse_zeroref_v1_fragment(
    fragment: &str,
    byte_length: Option<usize>,
) -> Result<ZeroRefFragment, ZeroRefError> {
    if fragment.is_empty() {
        return Err(ZeroRefError::Malformed);
    }
    match fragment.chars().next() {
        Some('B') => parse_zeroref_v1_byte_fragment(&fragment[1..], byte_length),
        Some('L') => parse_zeroref_v1_line_fragment(&fragment[1..]),
        _ => Err(ZeroRefError::Malformed),
    }
}

fn parse_zeroref_v1_byte_fragment(
    value: &str,
    byte_length: Option<usize>,
) -> Result<ZeroRefFragment, ZeroRefError> {
    let (start, end) = parse_zeroref_v1_fragment_bounds(value, 'B', false)?;
    if let Some(len) = byte_length {
        if end > len {
            return Err(ZeroRefError::Malformed);
        }
    }
    Ok(ZeroRefFragment::Byte { start, end })
}

fn parse_zeroref_v1_line_fragment(value: &str) -> Result<ZeroRefFragment, ZeroRefError> {
    let (start, end) = parse_zeroref_v1_fragment_bounds(value, 'L', true)?;
    Ok(ZeroRefFragment::Line { start, end })
}

fn parse_zeroref_v1_fragment_bounds(
    value: &str,
    repeated_kind: char,
    allow_single: bool,
) -> Result<(usize, usize), ZeroRefError> {
    let separated = value.split_once(',').or_else(|| value.split_once('-'));
    let (start, end) = match separated {
        Some((start, end)) => (start, end),
        None if allow_single => (value, value),
        None => return Err(ZeroRefError::Malformed),
    };
    let start = start
        .parse::<usize>()
        .map_err(|_| ZeroRefError::Malformed)?;
    let end = end
        .strip_prefix(repeated_kind)
        .unwrap_or(end)
        .parse::<usize>()
        .map_err(|_| ZeroRefError::Malformed)?;
    if (repeated_kind == 'L' && start == 0) || start > end {
        return Err(ZeroRefError::Malformed);
    }
    Ok((start, end))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpansionResult {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub selector: Option<String>,
    pub content: String,
    pub tokens: usize,
    pub found: bool,
    pub reason: String,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryState {
    pub version: u32,
    pub max_blobs: usize,
    pub max_files: usize,
    pub max_units: usize,
    pub max_search_hits: usize,
    pub max_bytes: usize,
    pub blobs: BTreeMap<String, String>,
    pub files: BTreeMap<String, StoredFile>,
    pub units: BTreeMap<String, StoredUnit>,
    pub search_hits: BTreeMap<String, StoredUnit>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    pub order: Vec<String>,
    #[serde(default)]
    pub shell_outcomes: BTreeMap<String, ShellOutcome>,
    #[serde(default)]
    pub shell_outcome_seq: u64,
    /// Short refs whose 16-hex prefix maps to multiple distinct full hashes.
    #[serde(default)]
    pub ambiguous_aliases: BTreeSet<String>,
}

/// Last observed result of a shell command, keyed by scope+command hash, so
/// repeat runs with byte-identical output can render as a delta instead of
/// re-paying for the full capsule. The combined blob itself lives in `blobs`
/// (content-addressed), so this index never duplicates payload bytes.
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

/// Infer a coarse content class from the ref kind and the original content type.
/// Used to tag ref-index entries so a predictor can learn per-class expansion rates.
fn classify_ref(ref_id: &str, content_type: Option<ContentType>) -> ContentClass {
    let Some(parsed) = parse_ref(ref_id) else {
        return ContentClass::Unknown;
    };
    match parsed.kind.as_str() {
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
            order: Vec::new(),
            shell_outcomes: BTreeMap::new(),
            shell_outcome_seq: 0,
            ambiguous_aliases: BTreeSet::new(),
        }
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
    /// Transient set of blob refs pending deletion. Applied by persist() and
    /// cleared only after successful authoritative snapshot write.
    pending_blob_deletions: BTreeSet<String>,
    /// Transient set of alias short refs pending deletion. Applied by
    /// persist() and cleared only after successful authoritative snapshot write.
    pending_alias_deletions: BTreeSet<String>,
}

/// Cache-file identity used to detect foreign writes between persists.
/// `atomic_write_json` always replaces the file via rename, so any cooperating
/// writer changes the inode — mtime alone is never trusted (its granularity
/// can be a full second on some filesystems). Captured only under the persist
/// lock; any uncertainty (missing file, unreadable metadata) must yield `None`
/// and force the full reload+merge path.
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum RefResolve {
    Found(String),
    NotFound,
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
        // Capture disk identity right after a successful load (read first,
        // stat second). This lets the FIRST persist of a fresh process take
        // the journal fast path — critical for CLI invocations, which are one
        // process per call and would otherwise rewrite the whole snapshot
        // every time. The capture runs without the persist lock, but that is
        // safe: persist re-captures under the lock and any mismatch falls
        // back to the full reload+merge. If a foreign write lands between our
        // read and stat, the identity matches the NEWER disk state and the
        // fast path appends only this session's refs — byte-for-byte the same
        // disk outcome merge_states would have produced.
        let (disk_identity, journal_identity) = match (&loaded, &persistence_path) {
            (Some(_), Some(path)) => (
                DiskIdentity::capture(path),
                DiskIdentity::capture(&journal_path(path)),
            ),
            _ => (None, None),
        };
        let state = loaded.unwrap_or_else(|| RecoveryState::empty(&config));
        let shared_cas = persistence_path
            .as_deref()
            .and_then(SharedCas::detect_from_cache_path);
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
            pending_blob_deletions: BTreeSet::new(),
            pending_alias_deletions: BTreeSet::new(),
        }
    }

    pub fn store_payload(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> Result<StoredPayload, RecoveryError> {
        let stored = self.store_payload_deferred(
            text,
            content_type,
            path,
            source_start_line,
            source_end_line,
        );
        self.persist()?;
        Ok(stored)
    }

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
        self.evict();
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
        let blob_ref = self.put_blob(text, content_type);
        let file_ref = self.put_file(text, content_type, path, source_start_line, source_end_line);
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

    pub fn persist_pending(&mut self) -> Result<(), RecoveryError> {
        self.persist()
    }

    pub fn store_alias(&mut self, alias: &str, target_ref: &str) -> Result<(), RecoveryError> {
        self.state
            .aliases
            .insert(alias.to_string(), target_ref.to_string());
        self.persist()
    }

    /// Store an alias without persisting. Caller must call `persist_pending()`.
    pub fn store_alias_deferred(&mut self, alias: &str, target_ref: &str) {
        self.state
            .aliases
            .insert(alias.to_string(), target_ref.to_string());
    }

    /// Remove an alias. Caller must call `persist_pending()`.
    /// Marks a tombstone; persist() applies the actual removal after merging
    /// concurrent disk state so external additions are preserved.
    pub(crate) fn remove_alias(&mut self, alias: &str) {
        self.state.aliases.remove(alias);
        self.state.ambiguous_aliases.remove(alias);
        self.pending_alias_deletions.insert(alias.to_string());
    }

    /// Remove a blob from the store. Caller must call `persist_pending()`.
    /// Marks a tombstone; persist() applies the actual removal after merging
    /// concurrent disk state so external additions are preserved.
    pub(crate) fn remove_blob(&mut self, ref_id: &str) {
        self.state.blobs.remove(ref_id);
        self.pending_blob_deletions.insert(ref_id.to_string());
    }

    /// Mark a short ref as ambiguous (maps to multiple full hashes).
    pub fn mark_ambiguous(&mut self, short_ref: &str) {
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

    /// Resolve a blob's content by its full ref ID (e.g. `tz://blob/b<hex>`).
    /// Returns `None` if not found or if the externalized sidecar cannot be resolved.
    pub(crate) fn resolve_blob_content(&self, ref_id: &str) -> Option<String> {
        self.state
            .blobs
            .get(ref_id)
            .and_then(|value| resolve_blob_value(self.persistence_path.as_deref(), value))
    }

    /// Insert a raw blob entry with a caller-specified ref ID (test only).
    #[cfg(test)]
    pub(crate) fn insert_test_blob(&mut self, ref_id: &str, content: &str) {
        self.state
            .blobs
            .insert(ref_id.to_string(), content.to_string());
        self.remember_ref(ref_id);
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

    pub fn store_search_output(
        &mut self,
        output: &str,
        query: Option<&str>,
    ) -> Result<Vec<String>, RecoveryError> {
        let refs = self.store_search_output_deferred(output, query);
        self.persist()?;
        Ok(refs)
    }

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
            self.ref_classes.insert(
                ref_id.clone(),
                classify_ref(&ref_id, Some(ContentType::SearchResult)),
            );
            self.state.search_hits.insert(
                ref_id.clone(),
                StoredUnit {
                    ref_id: ref_id.clone(),
                    text: line.to_string(),
                    content_type: "search_result".to_string(),
                    source_ref: None,
                    start_line: Some(idx + 1),
                    end_line: Some(idx + 1),
                },
            );
            self.remember_ref(&ref_id);
            refs.push(ref_id);
        }
        self.evict();
        refs
    }

    pub fn expand(
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
        // Validate fragments before the ZeroRef structural parser so fragment
        // failures retain their dedicated error taxonomy.
        if let Some((_, fragment)) = ref_id.split_once('#') {
            if let Err(err) = parse_fragment_spec(fragment) {
                return ExpansionResult::missing(
                    requested_ref,
                    selector.map(str::to_string),
                    fragment_error_reason(err),
                );
            }
        }
        // Full-hash portable blobs use the canonical shared CAS before the
        // legacy TokenZero JSON tier. The original scheme and complete digest
        // stay intact for routing, verification, and errors.
        let portable = match parse_zeroref_v1_blob(ref_id, None) {
            Ok(parsed) => Some(parsed),
            Err(ZeroRefError::Unsupported) => None,
            Err(ZeroRefError::LegacyAmbiguity) if is_legacy_same_store_blob_ref(ref_id) => None,
            Err(err) => {
                return ExpansionResult::missing(
                    requested_ref,
                    selector.map(str::to_string),
                    format!("zeroref-{err}"),
                );
            }
        };
        if portable.is_none()
            && (ref_id.starts_with("fz://") || ref_id.starts_with("gz://"))
            && !ref_id.starts_with("fz://blob/")
            && !ref_id.starts_with("gz://blob/")
        {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "unsupported-ref-kind",
            );
        }
        let Some(lookup_ref) = canonicalize_expand_ref(ref_id) else {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "invalid-ref",
            );
        };

        // Check legacy_compat and ambiguous-aliases gates on the
        // canonicalized requested ref BEFORE alias traversal. This ensures
        // disabled legacy refs and ambiguous short IDs fail immediately
        // regardless of whether an alias exists.
        if is_legacy_same_store_blob_ref(&lookup_ref) {
            if !self.config.legacy_compat {
                return ExpansionResult::missing(
                    requested_ref,
                    selector.map(str::to_string),
                    "legacy-ref-disabled",
                );
            }
            if self.state.ambiguous_aliases.contains(&lookup_ref) {
                return ExpansionResult::missing(
                    requested_ref,
                    selector.map(str::to_string),
                    "legacy-ambiguous",
                );
            }
            self.legacy_read_count += 1;
        }

        // Resolve alias chain AFTER the legacy gates so migrated short refs
        // route to their canonical full-hash target before CAS dispatch.
        let resolved_ref = self.resolve_alias_chain(&lookup_ref).unwrap_or(lookup_ref);

        // Re-parse the resolved (aliased) ref for shared CAS dispatch.
        let portable_resolved = match parse_zeroref_v1_blob(&resolved_ref, None) {
            Ok(parsed) => Some(parsed),
            Err(ZeroRefError::LegacyAmbiguity) | Err(ZeroRefError::Unsupported) => None,
            Err(_) => None,
        };

        let shared_content = match (&portable_resolved, &self.shared_cas) {
            (Some(portable), Some(cas)) => match cas.resolve(&portable.hash) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(content) => Some(content),
                    Err(_) => {
                        return ExpansionResult::missing(
                            requested_ref,
                            selector.map(str::to_string),
                            "shared-cas-non-utf8",
                        );
                    }
                },
                Err(SharedCasError::NotFound) if requested_ref.starts_with("tz://") => None,
                Err(SharedCasError::NotFound) => {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        "shared-cas-missing",
                    );
                }
                Err(SharedCasError::Corruption) => {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        "shared-cas-corruption",
                    );
                }
                Err(SharedCasError::Policy) => {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        "shared-cas-policy",
                    );
                }
                Err(SharedCasError::Io(_)) => {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        "shared-cas-io",
                    );
                }
                Err(SharedCasError::InvalidHash(_)) => {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        "zeroref-malformed",
                    );
                }
            },
            _ => None,
        };
        let ref_id = resolved_ref;        let Some(parsed) = parse_ref(&ref_id) else {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "invalid-ref",
            );
        };
        let mut selected_start = start_line;
        let mut selected_end = end_line;
        // Parse fragment spec early to catch malformed/duplicate/unknown
        // before store lookup (cqr.5). #B and #L are both supported; no
        // fallback to the full payload for a fragment request.
        let fragment_spec = parsed.fragment.as_deref().map(parse_fragment_spec);
        if let Some(Err(err)) = &fragment_spec {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                fragment_error_reason(*err),
            );
        }
        if let Some(Ok(FragmentSpec::Line { start, end })) = &fragment_spec {
            selected_start = Some(*start);
            selected_end = Some(*end);
        }
        // Resolve lines:/range:/around: before OOB so selector windows get the
        // same structured error as explicit start_line/end_line (zq9).
        resolve_selector_line_window(selector, &mut selected_start, &mut selected_end);
        let content = if let Some(content) = shared_content {
            content
        } else {
            match self.resolve_ref_with_index(&parsed.kind, &parsed.bare) {
                RefResolve::Found(content) => content,
                RefResolve::NotFound => {
                    let reason = if requested_ref.starts_with("fz://blob/")
                        || requested_ref.starts_with("gz://blob/")
                    {
                        "ref-not-found".to_string()
                    } else {
                        ref_not_found_reason(&parsed.kind)
                    };
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        reason,
                    );
                }
                RefResolve::DecodeFailed => {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        "decode-failed",
                    );
                }
            }
        };
        if parsed.kind == "file" && self.file_ref_is_stale(&parsed.bare) {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "stale-ref",
            );
        }
        // Apply #B byte-range fragment after content resolution (cqr.5).
        // Zero-based half-open: content[start..end]. start == end → empty.
        // Reversed was already caught by parse_fragment_spec; only OOB remains.
        if let Some(Ok(FragmentSpec::Byte { start, end })) = &fragment_spec {
            let bytes = content.as_bytes();
            if *end > bytes.len() {
                return ExpansionResult::missing(
                    requested_ref,
                    selector.map(str::to_string),
                    format!(
                        "fragment-out-of-range; start={start} end={end} len={}",
                        bytes.len()
                    ),
                );
            }
            let sliced = String::from_utf8_lossy(&bytes[*start..*end]).into_owned();
            self.recovery_tokens += count_tokens(&sliced);
            if let Some(store_path) = self.persistence_path.as_ref() {
                let content_class = self
                    .ref_classes
                    .get(&ref_id)
                    .copied()
                    .unwrap_or_else(|| classify_ref(&ref_id, None));
                record_ref_index_expanded(store_path, &ref_id, content_class);
            }
            return ExpansionResult::ok(requested_ref, selector.map(str::to_string), sliced);
        }
        // Explicit / selector line windows: OOB is a structured error, never
        // ref_not_found (zq9). 1-based inclusive; start past last line or end <
        // start fails.
        if let Some(start) = selected_start {
            let line_count = content_line_count(&content);
            if start == 0 || start > line_count {
                return ExpansionResult::missing(
                    requested_ref,
                    selector.map(str::to_string),
                    format!(
                        "window-out-of-range; start={start} end={} lines={line_count}",
                        selected_end
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| start.to_string())
                    ),
                );
            }
            if let Some(end) = selected_end {
                if end < start || end > line_count {
                    return ExpansionResult::missing(
                        requested_ref,
                        selector.map(str::to_string),
                        format!("window-out-of-range; start={start} end={end} lines={line_count}"),
                    );
                }
            }
        }
        let selected = select_content(
            &content,
            selector,
            selected_start,
            selected_end,
            anchor_kind,
            symbol,
        );
        self.recovery_tokens += count_tokens(&selected);
        if let Some(store_path) = self.persistence_path.as_ref() {
            let content_class = self
                .ref_classes
                .get(&ref_id)
                .copied()
                .unwrap_or_else(|| classify_ref(&ref_id, None));
            record_ref_index_expanded(store_path, &ref_id, content_class);
        }
        ExpansionResult::ok(requested_ref, selector.map(str::to_string), selected)
    }

    fn resolve_alias_chain(&self, ref_id: &str) -> Option<String> {
        let mut current = ref_id;
        for _ in 0..8 {
            let Some(next) = self.state.aliases.get(current) else {
                return (current != ref_id).then(|| current.to_string());
            };
            current = next;
        }
        None
    }

    pub fn has_ref(&self, ref_id: &str) -> bool {
        let Some(lookup) = canonicalize_expand_ref(ref_id) else {
            return false;
        };
        let lookup = self.resolve_alias_chain(&lookup).unwrap_or(lookup);
        let Some(parsed) = parse_ref(&lookup) else {
            return false;
        };
        match parsed.kind.as_str() {
            "blob" => self.state.blobs.contains_key(&parsed.bare),
            "file" => self.state.files.contains_key(&parsed.bare),
            "unit" => self.state.units.contains_key(&parsed.bare),
            "search" => self.state.search_hits.contains_key(&parsed.bare),
            _ => false,
        }
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
            "candidates": stale.iter().map(|ref_id| serde_json::json!({"category": "exact", "ref": ref_id, "reason": "stale-source"})).collect::<Vec<_>>(),
            "reclaimed_bytes": if dry_run { 0 } else { stale.len() },
        }))
    }

    /// Record the outcome of a shell command and report whether it repeated
    /// the previous run byte-for-byte (same combined output, same exit code).
    /// Callers may render verified-unchanged successes as a tiny delta
    /// envelope; the content-addressed blob ref still recovers exact bytes.
    pub fn record_shell_outcome(
        &mut self,
        scope: Option<&str>,
        command: &str,
        combined: &str,
        exit_code: Option<i32>,
    ) -> Result<ShellRepeat, RecoveryError> {
        let repeat = self.record_shell_outcome_deferred(scope, command, combined, exit_code);
        self.persist()?;
        Ok(repeat)
    }

    pub fn record_shell_outcome_deferred(
        &mut self,
        scope: Option<&str>,
        command: &str,
        combined: &str,
        exit_code: Option<i32>,
    ) -> ShellRepeat {
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
        while self.state.shell_outcomes.len() > MAX_SHELL_OUTCOMES {
            let victim = self
                .state
                .shell_outcomes
                .iter()
                .min_by_key(|(_, outcome)| outcome.seq)
                .map(|(key, _)| key.clone());
            match victim {
                Some(victim) => {
                    self.state.shell_outcomes.remove(&victim);
                }
                None => break,
            }
        }
        ShellRepeat { unchanged, seen }
    }

    fn put_blob(&mut self, text: &str, content_type: ContentType) -> String {
        let full_hash = sha256_hex(text);
        let canonical_ref = format!("tz://blob/{full_hash}");
        self.ref_classes
            .insert(canonical_ref.clone(), classify_ref(&canonical_ref, Some(content_type)));

        // Publish to the canonical shared CAS when attached. On success the
        // canonical ref is served from the immutable CAS and the payload is
        // not duplicated in local recovery state. On publish failure, store
        // the full-hash payload locally so the emitted ref remains readable.
        let cas_published = if let Some(cas) = &self.shared_cas {
            cas.publish(text.as_bytes()).is_ok()
        } else {
            false
        };

        // Always compute and store the legacy short ref as an alias so existing
        // callers using legacy refs can resolve through the alias chain.
        let legacy_ref = format!("tz://blob/{}", id_for('b', text));
        if legacy_ref != canonical_ref {
            self.state
                .aliases
                .insert(legacy_ref, canonical_ref.clone());
        }

        if !cas_published {
            // Multi-MB payloads (shell captures are the usual offender) stored
            // inline multiply every snapshot serialize, journal append, and load
            // parse, and sit in RAM for the process lifetime (bead tz8). Above
            // the threshold, durable stores divert the bytes to a content-
            // addressed sidecar file and keep a tiny marker as the value; the
            // marker travels through journal/merge/delta untouched and is only
            // resolved on expand. Fail-open: if the sidecar write fails, the
            // text stays inline.
            let value = self
                .persistence_path
                .as_deref()
                .and_then(|cache| externalize_blob_value(cache, text, &full_hash))
                .unwrap_or_else(|| text.to_string());
            self.state.blobs.insert(canonical_ref.clone(), value);
        }
        self.remember_ref(&canonical_ref);
        canonical_ref
    }

    fn put_file(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> String {
        let ref_id = recovery_file_ref(text, path);
        self.ref_classes
            .insert(ref_id.clone(), classify_ref(&ref_id, Some(content_type)));
        self.state.files.insert(
            ref_id.clone(),
            StoredFile {
                ref_id: ref_id.clone(),
                path: path.map(|p| p.to_string_lossy().to_string()),
                path_identity: path.map(path_identity_text),
                text: text.to_string(),
                content_type: content_type.to_string(),
                source_fingerprint: fingerprint_for_stored_payload(
                    path,
                    source_start_line,
                    source_end_line,
                ),
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
        let ref_id = format!("tz://unit/{}", id_for('u', text));
        self.ref_classes
            .insert(ref_id.clone(), classify_ref(&ref_id, Some(content_type)));
        self.state
            .units
            .entry(ref_id.clone())
            .or_insert_with(|| StoredUnit {
                ref_id: ref_id.clone(),
                text: text.to_string(),
                content_type: content_type.to_string(),
                source_ref: source_ref.map(str::to_string),
                start_line,
                end_line,
            });
        self.remember_ref(&ref_id);
        ref_id
    }

    fn resolve_ref(&self, kind: &str, bare: &str) -> RefResolve {
        match kind {
            "blob" => match self.state.blobs.get(bare) {
                Some(value) => resolve_blob_value(self.persistence_path.as_deref(), value)
                    .map(RefResolve::Found)
                    .unwrap_or(RefResolve::DecodeFailed),
                None => RefResolve::NotFound,
            },
            "file" => self
                .state
                .files
                .get(bare)
                .map(|f| RefResolve::Found(f.text.clone()))
                .unwrap_or(RefResolve::NotFound),
            "unit" => self
                .state
                .units
                .get(bare)
                .map(|u| RefResolve::Found(u.text.clone()))
                .unwrap_or(RefResolve::NotFound),
            "search" => self
                .state
                .search_hits
                .get(bare)
                .map(|u| RefResolve::Found(u.text.clone()))
                .unwrap_or(RefResolve::NotFound),
            _ => RefResolve::NotFound,
        }
    }

    fn resolve_ref_with_index(&self, kind: &str, bare: &str) -> RefResolve {
        match self.resolve_ref(kind, bare) {
            RefResolve::Found(content) => return RefResolve::Found(content),
            RefResolve::DecodeFailed => return RefResolve::DecodeFailed,
            RefResolve::NotFound => {}
        }
        if kind != "blob" {
            return RefResolve::NotFound;
        }
        resolve_blob_from_ref_index(bare, &self.config)
    }

    fn file_ref_is_stale(&self, bare: &str) -> bool {
        let Some(stored) = self.state.files.get(bare) else {
            return false;
        };
        let Some(path_text) = stored.path.as_deref() else {
            return false;
        };
        if path_text.starts_with("shell:") || path_text.starts_with("search:") {
            return false;
        }
        let Some(expected) = stored.source_fingerprint.as_ref() else {
            return false;
        };
        let source_path = stored
            .path_identity
            .as_deref()
            .and_then(path_from_identity_text)
            .unwrap_or_else(|| PathBuf::from(path_text));
        source_fingerprint(&source_path).is_none_or(|actual| actual != *expected)
    }

    fn remember_ref(&mut self, ref_id: &str) {
        self.state.order.push(ref_id.to_string());
        self.session_refs.push(ref_id.to_string());
    }

    fn evict(&mut self) {
        evict_prefix(
            &mut self.state.blobs,
            &mut self.state.order,
            "tz://blob/",
            self.config.max_blobs,
        );
        evict_prefix(
            &mut self.state.files,
            &mut self.state.order,
            "tz://file/",
            self.config.max_files,
        );
        evict_prefix(
            &mut self.state.units,
            &mut self.state.order,
            "tz://unit/",
            self.config.max_units,
        );
        evict_prefix(
            &mut self.state.search_hits,
            &mut self.state.order,
            "tz://search/",
            self.config.max_search_hits,
        );
        while self.approx_bytes() > self.config.max_bytes {
            let Some(victim) = self.state.order.iter().find(|r| self.has_ref(r)).cloned() else {
                break;
            };
            self.drop_ref(&victim);
        }
        self.compact_order();
    }

    fn drop_ref(&mut self, ref_id: &str) {
        self.state.blobs.remove(ref_id);
        self.state.files.remove(ref_id);
        self.state.units.remove(ref_id);
        self.state.search_hits.remove(ref_id);
    }

    fn compact_order(&mut self) {
        let live: HashSet<String> = self
            .state
            .blobs
            .keys()
            .chain(self.state.files.keys())
            .chain(self.state.units.keys())
            .chain(self.state.search_hits.keys())
            .cloned()
            .collect();
        let mut seen = HashSet::new();
        self.state
            .order
            .retain(|ref_id| live.contains(ref_id) && seen.insert(ref_id.clone()));
    }

    fn approx_bytes(&self) -> usize {
        // Externalized blob markers account at their original payload size so
        // eviction pressure reflects real content, not marker bytes.
        let blob_bytes: usize = self.state.blobs.values().map(|v| blob_value_len(v)).sum();
        let file_bytes: usize = self
            .state
            .files
            .values()
            .map(|v| v.text.len() + v.path.as_deref().unwrap_or_default().len())
            .sum();
        let unit_bytes: usize = self.state.units.values().map(|v| v.text.len()).sum();
        let search_bytes: usize = self.state.search_hits.values().map(|v| v.text.len()).sum();
        blob_bytes + file_bytes + unit_bytes + search_bytes
    }

    fn persist(&mut self) -> Result<(), RecoveryError> {
        let Some(path) = self.persistence_path.clone() else {
            self.evict();
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = PersistLock::acquire(recovery_lock_path(&path))?;
        // Skip the reload+merge only when the file is byte-identical to our
        // last write under this lock: in-memory state is then a superset of
        // disk and authoritative. Any mismatch — another process persisted,
        // the file vanished, metadata is unreadable — falls back to the full
        // merge so multi-process semantics are preserved exactly.
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
        // Apply pending deletions to in-memory state. Tombstones are kept
        // until the authoritative write succeeds so a crash-and-restart
        // retries the deletion. Drain to owned vecs to avoid borrow issues
        // with concurrent mutation of self.state and self.pending_*.
        let pending_aliases: Vec<String> = self.pending_alias_deletions.iter().cloned().collect();
        let pending_blobs: Vec<String> = self.pending_blob_deletions.iter().cloned().collect();
        for alias in &pending_aliases {
            self.state.aliases.remove(alias);
            self.state.ambiguous_aliases.remove(alias);
        }
        for ref_id in &pending_blobs {
            self.state.blobs.remove(ref_id);
        }
        let has_pending_deletions = !pending_aliases.is_empty() || !pending_blobs.is_empty();
        // Fast path: disk is byte-identical to our last write, so everything
        // new since then is exactly `session_refs`. Append that delta to the
        // journal sibling instead of rewriting the whole snapshot — persist
        // cost becomes O(new data this session), not O(entire store). The
        // delta line replays through `merge_states` at load, so merge
        // semantics are inherited, never re-implemented. Any append error or
        // an oversized journal falls through to the full snapshot rewrite.
        // Journal tombstones make deletions durable and replayable without
        // rewriting the snapshot; old journal entries cannot resurrect them.
        if unchanged_since_last_write && !has_pending_deletions {
            let delta = session_delta(&self.state, &self.session_refs, &self.config);
            let entry = JournalEntry {
                refs: std::mem::take(&mut self.session_refs),
                state: delta,
                deleted_blob_refs: self.pending_blob_deletions.iter().cloned().collect(),
                deleted_aliases: self.pending_alias_deletions.iter().cloned().collect(),
            };
            let snap_len = self.disk_identity.map_or(0, |identity| identity.len);
            if let Ok(journal_len) = append_journal(&journal_path(&path), &entry) {
                if journal_len <= journal_compact_threshold(snap_len) {
                    self.journal_identity = DiskIdentity::capture(&journal_path(&path));
                    append_blob_refs_to_ref_index(&path, &entry.refs, Some(&self.ref_classes));
                    self.pending_blob_deletions.clear();
                    self.pending_alias_deletions.clear();
                    return Ok(());
                }
            }
            // fall through: compact journal into a fresh snapshot
        }
        self.disk_identity = None;
        atomic_write_json(&path, &self.state)?;
        let _ = fs::remove_file(journal_path(&path));
        self.journal_identity = None;
        self.disk_identity = DiskIdentity::capture(&path);
        append_blob_refs_to_ref_index(&path, &self.session_refs, Some(&self.ref_classes));
        self.session_refs.clear();
        self.pending_blob_deletions.clear();
        self.pending_alias_deletions.clear();
        Ok(())
    }
}

#[derive(Debug)]
struct ParsedRef {
    kind: String,
    bare: String,
    fragment: Option<String>,
}

fn parse_ref(ref_id: &str) -> Option<ParsedRef> {
    // Callers canonicalize fz:// / gz:// → tz:// before parse_ref (same-store alias).
    // Accept only the store scheme here so scheme rewriting lives in one place.
    let (bare, fragment) = ref_id
        .split_once('#')
        .map_or((ref_id, None), |(b, f)| (b, Some(f.to_string())));
    let rest = bare.strip_prefix("tz://")?;
    let (kind, id) = rest.split_once('/')?;
    if id.is_empty() {
        return None;
    }
    if !matches!(kind, "blob" | "file" | "unit" | "search" | "codemode") {
        return None;
    }
    if kind == "codemode" {
        let mut parts = id.split('/');
        if parts.next() != Some("execution") {
            return None;
        }
        let _safe_id = parts.next()?;
        if !matches!(
            parts.next(),
            Some("code" | "steps" | "telemetry" | "result" | "error")
        ) || parts.next().is_some()
        {
            return None;
        }
    }
    Some(ParsedRef {
        kind: kind.to_string(),
        bare: format!("tz://{rest}"),
        fragment,
    })
}

fn parse_line_fragment(fragment: &str) -> (Option<usize>, Option<usize>) {
    let value = fragment.trim().trim_start_matches('L');
    if let Some((start, end)) = value.split_once('-') {
        (
            start.trim_start_matches('L').parse().ok(),
            end.trim_start_matches('L').parse().ok(),
        )
    } else {
        let line = value.parse().ok();
        (line, line)
    }
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

/// Line count for window validation (split_inclusive so a trailing newline
/// still counts as a line segment, matching `line_slice_exact`).
fn content_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split_inclusive('\n').count()
    }
}

/// Exact line slice for recovery: returns the verbatim bytes of lines
/// `start..=end` (1-based, inclusive), preserving each line's trailing
/// newline — including a trailing blank line — so `expand` is byte-exact.
/// Unlike `tokenzero_core::line_range` (display: drops trailing newlines),
/// this is the recovery path and must reproduce the original bytes.
/// Caller must validate OOB via `content_line_count` before calling when a
/// structured `window-out-of-range` error is required.
fn line_slice_exact(text: &str, start: usize, end: usize) -> String {
    let start = start.max(1);
    let end = end.max(start);
    let segments: Vec<&str> = text.split_inclusive('\n').collect();
    if start > segments.len() {
        return String::new();
    }
    let lo = start - 1;
    let hi = end.min(segments.len());
    segments[lo..hi].concat()
}

/// Parse line-window selectors into start/end. Non-window selectors leave the
/// existing start/end untouched.
fn resolve_selector_line_window(
    selector: Option<&str>,
    selected_start: &mut Option<usize>,
    selected_end: &mut Option<usize>,
) {
    match selector {
        Some(value)
            if value.starts_with("range:")
                || value.starts_with("lines:")
                || value.starts_with("line:") =>
        {
            let prefix_len = value.find(':').map_or(0, |n| n + 1);
            let (start, end) = parse_line_fragment(&value[prefix_len..]);
            *selected_start = start;
            *selected_end = end;
        }
        Some(value) if value.starts_with("around:") => {
            let (start, end) = parse_around_selector(&value["around:".len()..]);
            *selected_start = start;
            *selected_end = end;
        }
        _ => {}
    }
}

fn select_content(
    content: &str,
    selector: Option<&str>,
    start_line: Option<usize>,
    end_line: Option<usize>,
    anchor_kind: Option<&str>,
    symbol: Option<&str>,
) -> String {
    let mut selected_start = start_line;
    let mut selected_end = end_line;
    let mut selected_symbol = symbol.map(str::to_string);
    let mut selected_anchor = anchor_kind.map(str::to_string);
    match selector {
        Some("raw") | None => {}
        Some("error_block") => return error_block(content, 3),
        Some("summary") => return tokenzero_core::summarize_lines(content, 12, 8, ""),
        Some(value) if value.starts_with("anchor:") => {
            selected_anchor = Some(value["anchor:".len()..].to_string())
        }
        Some(value) if value.starts_with("symbol:") => {
            selected_symbol = Some(value["symbol:".len()..].to_string())
        }
        Some(value)
            if value.starts_with("range:")
                || value.starts_with("lines:")
                || value.starts_with("line:")
                || value.starts_with("around:") =>
        {
            // Already resolved by resolve_selector_line_window before OOB.
            resolve_selector_line_window(selector, &mut selected_start, &mut selected_end);
        }
        Some(_) => {}
    }
    if let Some(start) = selected_start {
        return line_slice_exact(content, start, selected_end.unwrap_or(start));
    }
    if let Some(symbol) = selected_symbol {
        return symbol_block(content, &symbol);
    }
    if selected_anchor.is_some() {
        return content
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("fn ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("class ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("impl ")
                    || trimmed.starts_with("use ")
                    || trimmed.starts_with("import ")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content.to_string()
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

fn ref_index_enabled() -> bool {
    #[cfg(test)]
    if let Some((enabled, _)) = ref_index_test_override() {
        return enabled;
    }
    env::var(REF_INDEX_DISABLE_ENV)
        .map(|value| value.trim() != "0")
        .unwrap_or(true)
}

fn ref_index_root() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some((enabled, path)) = ref_index_test_override() {
        return enabled.then_some(path);
    }
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

fn ref_index_shard_name(ref_id: &str) -> String {
    let id = ref_index_id_part(ref_id).unwrap_or(ref_id);
    let mut chars = id.chars();
    let first = chars.next().unwrap_or('x');
    let second = chars.next().unwrap_or('x');
    format!("{first}{second}.ndjson")
}

fn ref_index_shard_path(root: &Path, ref_id: &str) -> PathBuf {
    root.join(ref_index_shard_name(ref_id))
}

fn ref_index_lock_path(shard: &Path) -> PathBuf {
    append_file_name_suffix(shard, ".lock")
}

fn append_blob_refs_to_ref_index(
    store_path: &Path,
    refs: &[String],
    classes: Option<&BTreeMap<String, ContentClass>>,
) {
    let Some(root) = ref_index_root() else {
        return;
    };
    let Ok(store_path) = store_path
        .canonicalize()
        .or_else(|_| Ok::<_, std::io::Error>(store_path.to_path_buf()))
    else {
        return;
    };
    let blob_refs: Vec<&String> = refs
        .iter()
        .filter(|ref_id| ref_id.starts_with("tz://blob/"))
        .collect();
    if blob_refs.is_empty() || create_ref_index_dir(&root).is_err() {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for ref_id in blob_refs {
        let shard = ref_index_shard_path(&root, ref_id);
        let Ok(_lock) =
            PersistLock::acquire_with_retries(ref_index_lock_path(&shard), LOCK_RETRIES)
        else {
            continue;
        };
        if newest_ref_index_store_path(&shard, ref_id).as_deref()
            == Some(store_path.to_string_lossy().as_ref())
        {
            continue;
        }
        let content_class = classes
            .and_then(|m| m.get(ref_id.as_str()))
            .copied()
            .unwrap_or_else(|| classify_ref(ref_id, None));
        if append_ref_index_line(&shard, ref_id, &store_path, ts, content_class, false).is_ok()
            && fs::metadata(&shard)
                .map(|meta| meta.len() > REF_INDEX_MAX_BYTES)
                .unwrap_or(false)
        {
            let _ = compact_ref_index_shard(&shard);
        }
    }
}

fn append_ref_index_line(
    shard: &Path,
    ref_id: &str,
    store_path: &Path,
    ts: u128,
    content_class: ContentClass,
    expanded: bool,
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
    };
    let mut line = serde_json::to_string(&entry)?;
    line.push('\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(shard)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn compact_ref_index_shard(shard: &Path) -> Result<(), RecoveryError> {
    let file = match fs::File::open(shard) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let Some(text) = read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))?
    else {
        return Ok(());
    };
    let entries = newest_ref_index_entries(&text, None);
    write_ref_index_entries(shard, entries.values())
}

fn prune_ref_index_stale_entries(ref_id: &str, stale_store_paths: &HashSet<String>) {
    if stale_store_paths.is_empty() {
        return;
    }
    let Some(root) = ref_index_root() else {
        return;
    };
    let shard = ref_index_shard_path(&root, ref_id);
    let Ok(_lock) = PersistLock::acquire_with_retries(ref_index_lock_path(&shard), LOCK_RETRIES)
    else {
        return;
    };
    let Ok(file) = fs::File::open(&shard) else {
        return;
    };
    let Ok(Some(text)) = read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))
    else {
        return;
    };
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<RefIndexEntry>(line) else {
            break;
        };
        if entry.ref_id == ref_id && stale_store_paths.contains(&entry.store_path) {
            continue;
        }
        entries.push(entry);
    }
    let _ = write_ref_index_entries(&shard, entries.iter());
}

fn newest_ref_index_store_path(shard: &Path, ref_id: &str) -> Option<String> {
    let file = fs::File::open(shard).ok()?;
    let text = read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))
        .ok()
        .flatten()?;
    ref_index_entries_for_ref(&text, ref_id)
        .into_iter()
        .next()
        .map(|entry| entry.store_path)
}

fn ref_index_entries_for_ref(text: &str, ref_id: &str) -> Vec<RefIndexEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<RefIndexEntry>(line) else {
            break;
        };
        if entry.ref_id == ref_id {
            entries.push(entry);
        }
    }
    entries.sort_by_key(|b| std::cmp::Reverse(b.ts));
    entries
}

fn newest_ref_index_entries(text: &str, skip_ref: Option<&str>) -> BTreeMap<String, RefIndexEntry> {
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<RefIndexEntry>(line) else {
            break;
        };
        if skip_ref == Some(entry.ref_id.as_str()) {
            continue;
        }
        let replace = entries
            .get(&entry.ref_id)
            .map(|existing: &RefIndexEntry| entry.ts >= existing.ts)
            .unwrap_or(true);
        if replace {
            let mut entry = entry;
            if let Some(existing) = entries.get(&entry.ref_id) {
                entry.expanded |= existing.expanded;
            }
            entries.insert(entry.ref_id.clone(), entry);
        } else if let Some(existing) = entries.get_mut(&entry.ref_id) {
            existing.expanded |= entry.expanded;
        }
    }
    entries
}

fn write_ref_index_entries<'a>(
    shard: &Path,
    entries: impl IntoIterator<Item = &'a RefIndexEntry>,
) -> Result<(), RecoveryError> {
    let parent = shard.parent().unwrap_or_else(|| Path::new("."));
    create_ref_index_dir(parent)?;
    let tmp = recovery_tmp_path(shard);
    {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        for entry in entries {
            let mut line = serde_json::to_string(entry)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
        }
    }
    fs::rename(&tmp, shard).map_err(RecoveryError::from)
}

fn resolve_blob_from_ref_index(ref_id: &str, config: &RecoveryConfig) -> RefResolve {
    let Some(root) = ref_index_root() else {
        return RefResolve::NotFound;
    };
    let shard = ref_index_shard_path(&root, ref_id);
    let Ok(file) = fs::File::open(&shard) else {
        return RefResolve::NotFound;
    };
    let Ok(Some(text)) = read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))
    else {
        return RefResolve::NotFound;
    };
    let entries = ref_index_entries_for_ref(&text, ref_id);
    let mut stale_store_paths = HashSet::new();
    for entry in entries {
        let store_path = PathBuf::from(&entry.store_path);
        if !store_path.is_file() {
            stale_store_paths.insert(entry.store_path);
            continue;
        }
        let resolved = load_state(&store_path, config)
            .ok()
            .flatten()
            .and_then(|state| state.blobs.get(ref_id).cloned())
            .map(|value| {
                resolve_blob_value(Some(&store_path), &value)
                    .map(RefResolve::Found)
                    .unwrap_or(RefResolve::DecodeFailed)
            });
        match resolved {
            Some(RefResolve::Found(content)) => {
                if !stale_store_paths.is_empty() {
                    prune_ref_index_stale_entries(ref_id, &stale_store_paths);
                }
                return RefResolve::Found(content);
            }
            Some(RefResolve::DecodeFailed) => {
                if !stale_store_paths.is_empty() {
                    prune_ref_index_stale_entries(ref_id, &stale_store_paths);
                }
                return RefResolve::DecodeFailed;
            }
            Some(RefResolve::NotFound) | None => stale_store_paths.insert(entry.store_path),
        };
    }
    prune_ref_index_stale_entries(ref_id, &stale_store_paths);
    RefResolve::NotFound
}

/// Append an expansion outcome to the per-user ref index. Preserves the
/// content class from an existing entry for the same ref when available,
/// so a ref expanded in a later session keeps the class it was stored with.
fn record_ref_index_expanded(store_path: &Path, ref_id: &str, fallback_class: ContentClass) {
    let Some(root) = ref_index_root() else {
        return;
    };
    let Ok(store_path) = store_path
        .canonicalize()
        .or_else(|_| Ok::<_, std::io::Error>(store_path.to_path_buf()))
    else {
        return;
    };
    let shard = ref_index_shard_path(&root, ref_id);
    let Ok(_lock) = PersistLock::acquire_with_retries(ref_index_lock_path(&shard), LOCK_RETRIES)
    else {
        return;
    };
    let existing = if let Ok(file) = fs::File::open(&shard) {
        read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))
            .ok()
            .flatten()
            .and_then(|text| ref_index_entries_for_ref(&text, ref_id).into_iter().next())
    } else {
        None
    };
    let content_class = existing
        .as_ref()
        .map(|entry| entry.content_class)
        .unwrap_or(fallback_class);
    // Avoid rewriting the shard when the ref is already marked expanded.
    // The expanded flag is sticky across sessions.
    if existing
        .as_ref()
        .map(|entry| entry.expanded)
        .unwrap_or(false)
    {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let _ = append_ref_index_line(&shard, ref_id, &store_path, ts, content_class, true);
    if fs::metadata(&shard)
        .map(|meta| meta.len() > REF_INDEX_MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = compact_ref_index_shard(&shard);
    }
}

/// Export per-content-class expansion rates from the per-user ref index.
/// Returns a JSON summary with total refs, expanded refs, and the expansion
/// rate for each content class. The `expanded` flag is sticky across sessions.
pub fn export_class_stats() -> serde_json::Value {
    let empty = serde_json::json!({
        "schema_version": "tokenzero.recovery.class-stats.v1",
        "classes": Vec::<serde_json::Value>::new(),
        "total_refs": 0,
        "total_expanded": 0,
    });
    let Some(root) = ref_index_root() else {
        return empty.clone();
    };
    let mut all_entries = Vec::new();
    let Ok(entries) = fs::read_dir(&root) else {
        return empty;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ndjson") {
            continue;
        }
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let Ok(Some(text)) =
            read_limited_utf8(file, (REF_INDEX_MAX_BYTES as usize).saturating_mul(4))
        else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<RefIndexEntry>(line) {
                all_entries.push(entry);
            }
        }
    }
    let mut per_ref: BTreeMap<String, (u128, ContentClass, bool)> = BTreeMap::new();
    for entry in all_entries {
        match per_ref.entry(entry.ref_id.clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert((entry.ts, entry.content_class, entry.expanded));
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let (ts, class, expanded) = slot.get_mut();
                *expanded |= entry.expanded;
                if entry.ts > *ts {
                    *ts = entry.ts;
                    *class = entry.content_class;
                }
            }
        }
    }
    let mut totals: BTreeMap<ContentClass, (usize, usize)> = BTreeMap::new();
    for (_, class, expanded) in per_ref.values() {
        let (total, expanded_count) = totals.entry(*class).or_insert((0, 0));
        *total += 1;
        if *expanded {
            *expanded_count += 1;
        }
    }
    let mut classes = Vec::new();
    let mut total_refs = 0usize;
    let mut total_expanded = 0usize;
    for class in [
        ContentClass::SourceFile,
        ContentClass::Diff,
        ContentClass::ShellOutput,
        ContentClass::SearchHits,
        ContentClass::Doc,
        ContentClass::BinaryPreview,
        ContentClass::Unknown,
    ] {
        let (total, expanded) = totals.remove(&class).unwrap_or((0, 0));
        let rate = if total > 0 {
            expanded as f64 / total as f64
        } else {
            0.0
        };
        classes.push(serde_json::json!({
            "content_class": class,
            "total": total,
            "expanded": expanded,
            "rate": rate,
        }));
        total_refs += total;
        total_expanded += expanded;
    }
    serde_json::json!({
        "schema_version": "tokenzero.recovery.class-stats.v1",
        "classes": classes,
        "total_refs": total_refs,
        "total_expanded": total_expanded,
    })
}

fn load_state(
    path: &Path,
    config: &RecoveryConfig,
) -> Result<Option<RecoveryState>, RecoveryError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Ok(None);
    }
    // Compare as u64 so a file larger than usize can't truncate and slip past
    // the load-size guard on 32-bit targets (which would risk an OOM on read).
    if meta.len() > config.max_load_bytes as u64 {
        return Ok(None);
    }
    let Some(text) = read_limited_utf8(file, config.max_load_bytes)? else {
        return Ok(None);
    };
    let Ok(mut state) = serde_json::from_str::<RecoveryState>(&text) else {
        return Ok(None);
    };
    state.max_blobs = config.max_blobs;
    state.max_files = config.max_files;
    state.max_units = config.max_units;
    state.max_search_hits = config.max_search_hits;
    state.max_bytes = config.max_bytes;
    Ok(Some(apply_journal(state, path, config)))
}

/// Externalized-blob sidecar (bead tz8). Values >= this many bytes are
/// written to `<cache>.blobs/<sha256>.txt` and replaced by a marker string:
/// `\u{0}tzx:v1:<sha256hex>:<len>:`. Content-addressed: reads verify the
/// hash, so a torn or tampered sidecar is a cache miss, never bad bytes.
/// A leading NUL keeps collisions with real tool output implausible, and a
/// malformed marker is treated as literal text (fail-open both ways).
const BLOB_EXTERNALIZE_MIN_BYTES: usize = 64 * 1024;
const BLOB_MARKER_PREFIX: &str = "\u{0}tzx:v1:";

fn blob_sidecar_dir(cache_path: &Path) -> PathBuf {
    let mut os: OsString = cache_path.as_os_str().to_owned();
    os.push(".blobs");
    PathBuf::from(os)
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

fn blob_value_len(value: &str) -> usize {
    parse_blob_marker(value).map_or(value.len(), |(_, len)| len)
}

fn resolve_blob_value(cache_path: Option<&Path>, value: &str) -> Option<String> {
    let Some((hash, _)) = parse_blob_marker(value) else {
        return Some(value.to_string());
    };
    let cache_path = cache_path?;
    let path = blob_sidecar_dir(cache_path).join(format!("{hash}.txt"));
    let text = fs::read_to_string(path).ok()?;
    (sha256_hex(&text) == hash).then_some(text)
}

/// Journal sibling of the snapshot: `recovery-cache.json.journal`. Each line
/// is one persisted session delta (a `JournalEntry`); load replays them onto
/// the snapshot through `merge_states`, so on-disk state is always
/// `snapshot ⊕ journal` and merge semantics have a single implementation.
fn journal_path(path: &Path) -> PathBuf {
    let mut os: OsString = path.as_os_str().to_owned();
    os.push(".journal");
    PathBuf::from(os)
}

/// One persist's worth of new data: the refs stored this session plus a
/// minimal `RecoveryState` carrying only their entries (and the session's
/// aliases/shell outcomes, which merge unconditionally).
#[derive(Debug, Serialize, Deserialize)]
struct JournalEntry {
    refs: Vec<String>,
    state: RecoveryState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deleted_blob_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deleted_aliases: Vec<String>,
}

fn session_delta(
    state: &RecoveryState,
    session_refs: &[String],
    config: &RecoveryConfig,
) -> RecoveryState {
    let mut delta = RecoveryState::empty(config);
    for ref_id in session_refs {
        if let Some(value) = state.blobs.get(ref_id) {
            delta.blobs.insert(ref_id.clone(), value.clone());
        }
        if let Some(value) = state.files.get(ref_id) {
            delta.files.insert(ref_id.clone(), value.clone());
        }
        if let Some(value) = state.units.get(ref_id) {
            delta.units.insert(ref_id.clone(), value.clone());
        }
        if let Some(value) = state.search_hits.get(ref_id) {
            delta.search_hits.insert(ref_id.clone(), value.clone());
        }
    }
    // Aliases must travel wholesale: an alias is often stored AFTER the
    // persist that carried its target (persist clears session_refs), so any
    // session-ref filter silently drops it from the journal — codemode's
    // logical execution refs died exactly this way. The map is small and
    // merge_states upserts aliases unconditionally, so replay stays exact.
    delta.aliases = state.aliases.clone();
    // Shell outcomes are a small capped map merged "current wins per key";
    // carrying the whole map keeps replay exact without change tracking.
    delta.shell_outcomes = state.shell_outcomes.clone();
    delta.shell_outcome_seq = state.shell_outcome_seq;
    // Ambiguous aliases must travel wholesale — same rationale as aliases.
    delta.ambiguous_aliases = state.ambiguous_aliases.clone();
    delta.order = session_refs
        .iter()
        .filter(|ref_id| {
            delta.blobs.contains_key(*ref_id)
                || delta.files.contains_key(*ref_id)
                || delta.units.contains_key(*ref_id)
                || delta.search_hits.contains_key(*ref_id)
        })
        .cloned()
        .collect();
    delta
}

/// Compact once the journal outgrows the snapshot (with a floor so tiny
/// stores don't compact on every persist). Bounds disk and load cost at
/// ~2× snapshot while keeping persist amortized O(new data).
fn journal_compact_threshold(snapshot_len: u64) -> u64 {
    snapshot_len.max(64 * 1024)
}

fn append_journal(path: &Path, entry: &JournalEntry) -> Result<u64, RecoveryError> {
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(file.metadata()?.len())
}

/// Replay journal lines onto a loaded snapshot. Fail-open at every step: a
/// missing/oversized/corrupt journal simply yields the snapshot (the cache is
/// reconstructible by design). A parse failure stops replay at that line so a
/// torn tail write can never poison earlier, complete entries.
fn apply_journal(mut state: RecoveryState, path: &Path, config: &RecoveryConfig) -> RecoveryState {
    let journal = journal_path(path);
    let Ok(file) = fs::File::open(&journal) else {
        return state;
    };
    if file
        .metadata()
        .map(|meta| !meta.is_file() || meta.len() > config.max_load_bytes as u64)
        .unwrap_or(true)
    {
        return state;
    }
    let Ok(Some(text)) = read_limited_utf8(file, config.max_load_bytes) else {
        return state;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<JournalEntry>(line) else {
            break;
        };
        let JournalEntry {
            refs,
            state: delta,
            deleted_blob_refs,
            deleted_aliases,
        } = entry;
        let accumulated = std::mem::replace(&mut state, RecoveryState::empty(config));
        state = merge_states(accumulated, delta, &refs, config);
        for alias in deleted_aliases {
            state.aliases.remove(&alias);
            state.ambiguous_aliases.remove(&alias);
        }
        for ref_id in deleted_blob_refs {
            state.blobs.remove(&ref_id);
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

fn merge_states(
    existing: RecoveryState,
    current: RecoveryState,
    session_refs: &[String],
    config: &RecoveryConfig,
) -> RecoveryState {
    let session: HashSet<&str> = session_refs.iter().map(String::as_str).collect();
    let mut merged = existing;
    for (ref_id, value) in current.blobs {
        if session.contains(ref_id.as_str()) || merged.blobs.contains_key(&ref_id) {
            merged.blobs.insert(ref_id, value);
        }
    }
    for (ref_id, value) in current.files {
        if session.contains(ref_id.as_str()) || merged.files.contains_key(&ref_id) {
            merged.files.insert(ref_id, value);
        }
    }
    for (ref_id, value) in current.units {
        if session.contains(ref_id.as_str()) || merged.units.contains_key(&ref_id) {
            merged.units.insert(ref_id, value);
        }
    }
    for (ref_id, value) in current.search_hits {
        if session.contains(ref_id.as_str()) || merged.search_hits.contains_key(&ref_id) {
            merged.search_hits.insert(ref_id, value);
        }
    }
    for (alias, target) in current.aliases {
        merged.aliases.insert(alias, target);
    }
    merged.order.extend(session_refs.iter().cloned());
    let mut seen = HashSet::new();
    merged.order.retain(|ref_id| seen.insert(ref_id.clone()));
    // Shell outcomes: this session's observations win per key; the verdict
    // itself is always recomputed from a content hash at record time, so a
    // merge can never fabricate an "unchanged" result.
    merged.shell_outcome_seq = merged.shell_outcome_seq.max(current.shell_outcome_seq);
    for (key, outcome) in current.shell_outcomes {
        merged.shell_outcomes.insert(key, outcome);
    }
    while merged.shell_outcomes.len() > MAX_SHELL_OUTCOMES {
        let victim = merged
            .shell_outcomes
            .iter()
            .min_by_key(|(_, outcome)| outcome.seq)
            .map(|(key, _)| key.clone());
        match victim {
            Some(victim) => {
                merged.shell_outcomes.remove(&victim);
            }
            None => break,
        }
    }
    for alias in current.ambiguous_aliases {
        merged.ambiguous_aliases.insert(alias);
    }
    merged.max_blobs = config.max_blobs;
    merged.max_files = config.max_files;
    merged.max_units = config.max_units;
    merged.max_search_hits = config.max_search_hits;
    merged.max_bytes = config.max_bytes;
    merged
}

fn evict_prefix<T>(
    items: &mut BTreeMap<String, T>,
    order: &mut Vec<String>,
    prefix: &str,
    limit: usize,
) {
    if items.len() <= limit {
        return;
    }

    let excess = items.len() - limit;
    let mut victims = Vec::with_capacity(excess);
    let mut victim_set = HashSet::with_capacity(excess);

    for ref_id in order.iter() {
        if victims.len() == excess {
            break;
        }
        if ref_id.starts_with(prefix)
            && items.contains_key(ref_id)
            && victim_set.insert(ref_id.clone())
        {
            victims.push(ref_id.clone());
        }
    }

    if victims.len() < excess {
        for ref_id in items.keys() {
            if victims.len() == excess {
                break;
            }
            if victim_set.insert(ref_id.clone()) {
                victims.push(ref_id.clone());
            }
        }
    }

    for victim in &victims {
        items.remove(victim);
    }
    order.retain(|item| !victim_set.contains(item));
}

fn atomic_write_json(path: &Path, state: &RecoveryState) -> Result<(), RecoveryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut last_collision = None;
    for _ in 0..TMP_RETRIES {
        let tmp = recovery_tmp_path(path);
        match write_json_to_tmp(&tmp, state) {
            Ok(()) => {
                if let Err(err) = fs::rename(&tmp, path) {
                    // Best-effort cleanup: the rename error is the one worth
                    // surfacing; a failed unlink only strands a temp file.
                    let _ = fs::remove_file(&tmp);
                    return Err(err.into());
                }
                // No parent-dir fsync after rename: see write_json_to_tmp.
                // Atomicity (no torn read) is provided by rename itself;
                // power-loss durability of the rename is out of scope for a
                // reconstructible cache.
                return Ok(());
            }
            Err(RecoveryError::Io(err)) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(err);
            }
            Err(err) => {
                // Best-effort cleanup; propagate the write error unmasked.
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
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    // The cache holds the exact bytes of everything served for this
    // workspace; keep it owner-only. The atomic rename preserves the mode.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(tmp)?;
    // Buffer the serializer: serde_json::to_writer on a raw File issues one
    // write(2) per JSON fragment, which profiled as 95% of warm-op wall time
    // (tests/artifacts/perf/2026-06-11-pushmax). Identical bytes, ~3 syscalls.
    let mut writer = std::io::BufWriter::with_capacity(1 << 20, file);
    serde_json::to_writer(&mut writer, state)?;
    writer.write_all(b"\n")?;
    let _file = writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?;
    // sync_data, not sync_all: the cache is reconstructible working state
    // (a lost entry reports dangling-ref on expand, never wrong bytes; see
    // docs/racc.md), so file metadata durability is not required — only that
    // the tmp file's *bytes* are on disk before the rename publishes it, so a
    // reader can never observe a torn/partial cache. On macOS sync_all maps to
    // F_FULLFSYNC (full device flush, ~8ms pair); sync_data is fdatasync and
    // skips it (~16x faster) while still ordering the data write ahead of the
    // rename.
    Ok(())
}

// No sync_parent_dir: the directory entry created by rename is not flushed.
// Crash *consistency* does not need it — rename is atomic within a single
// directory, so a concurrent or post-crash reader sees either the old inode or
// the new one, never a torn file. Only power-loss *durability* of the rename
// would need a parent-dir fsync, and that is explicitly out of scope here: the
// cache is reconstructible working state. On macOS this fsync was a second
// F_FULLFSYNC, doubling persist cost for a guarantee the cache does not claim.
// Accepted power-loss window: a crash in the unflushed interval can revert the
// cache to the previous consistent state, degrading affected refs to
// dangling-ref — the designed-safe outcome (docs/racc.md), never wrong bytes.

fn recovery_file_ref(text: &str, path: Option<&Path>) -> String {
    let path_identity = path.map(path_identity_text).unwrap_or_default();
    format!(
        "tz://file/{}",
        id_for('f', &format!("{path_identity}:{text}"))
    )
}

#[cfg(unix)]
fn path_identity_text(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStrExt;

    let mut encoded = String::from("unix:");
    for byte in path.as_os_str().as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing hex bytes into String cannot fail");
    }
    encoded
}

#[cfg(unix)]
fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    let bytes = decode_hex_bytes(identity.strip_prefix("unix:")?)?;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_identity_text(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::windows::ffi::OsStrExt;

    let mut encoded = String::from("windows:");
    for unit in path.as_os_str().encode_wide() {
        write!(&mut encoded, "{unit:04x}").expect("writing hex units into String cannot fail");
    }
    encoded
}

#[cfg(windows)]
fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    let bytes = decode_hex_bytes(identity.strip_prefix("windows:")?)?;
    let mut chunks = bytes.chunks_exact(2);
    let units = chunks
        .by_ref()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    if !chunks.remainder().is_empty() {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(not(any(unix, windows)))]
fn path_identity_text(path: &Path) -> String {
    format!("display:{}", path.to_string_lossy())
}

#[cfg(not(any(unix, windows)))]
fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
    Some(PathBuf::from(identity.strip_prefix("display:")?))
}

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

/// Remove abandoned atomic-write temp files left beside the recovery cache by
/// processes that died mid-persist. Both the current hidden
/// `.{name}.{pid}.{n}.tmp` shape and the pre-1.0 visible `{name}.*.tmp` shape
/// are matched; the `.lock` anchor never is. Only files older than `max_age`
/// are touched, so an in-flight writer is never raced. Fail-open: per-file
/// failures are counted, a missing directory is an empty report.
pub fn sweep_stale_tmp_files(
    cache_path: &Path,
    max_age: Duration,
    dry_run: bool,
) -> TmpSweepReport {
    let mut report = TmpSweepReport {
        dry_run,
        ..TmpSweepReport::default()
    };
    let Some(parent) = cache_path.parent() else {
        return report;
    };
    let Some(cache_name) = cache_path.file_name().and_then(|name| name.to_str()) else {
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
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        report.scanned += 1;
        let expired = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|age| age > max_age)
            .unwrap_or(false);
        if !expired {
            continue;
        }
        if dry_run || fs::remove_file(&path).is_ok() {
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
                    // SAFETY: This file is a stable OS-lock anchor. Do not
                    // unlink it while holding or releasing the lock: replacing
                    // the inode lets a second process lock the replacement
                    // while this process still owns the original.
                    //
                    // No sync_all here: flock is a VFS-level kernel lock that
                    // releases on process death regardless of on-disk
                    // durability. The PID written below is diagnostic, not
                    // required for correctness. Skipping the fsync saves
                    // ~5-15ms per persist on macOS where sync_all =
                    // F_FULLFSYNC.
                    file.set_len(0)?;
                    writeln!(file, "{}", std::process::id())?;
                    return Ok(Self { file });
                }
                Err(TryLockError::WouldBlock) => {
                    if attempt + 1 < retries {
                        thread::sleep(LOCK_RETRY_DELAY);
                    }
                }
                Err(TryLockError::Error(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if attempt + 1 < retries {
                        thread::sleep(LOCK_RETRY_DELAY);
                    }
                }
                Err(TryLockError::Error(err)) => return Err(err.into()),
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

fn fingerprint_for_stored_payload(
    path: Option<&Path>,
    source_start_line: Option<usize>,
    source_end_line: Option<usize>,
) -> Option<SourceFingerprint> {
    if source_start_line.is_some() || source_end_line.is_some() {
        return None;
    }
    let path = path?;
    let path_text = path.to_string_lossy();
    if path_text.starts_with("shell:") || path_text.starts_with("search:") {
        return None;
    }
    source_fingerprint(path)
}

fn source_fingerprint(path: &Path) -> Option<SourceFingerprint> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let sha256 = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    Some(SourceFingerprint {
        size: meta.len(),
        mtime_ns,
        sha256,
    })
}

#[cfg(test)]
mod tests;
