//! CacheZero shadow ledger: would-have-hit decisions without serving.
//!
//! `TOKENZERO_CACHEZERO=shadow` (or `TOKENZERO_CACHEZERO_MODE`) writes every
//! ActionCache decision to `<store>/cachezero/shadow.jsonl`. The serve path
//! stays off until graduation (causal-hit mass > 20% of session mass).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::action_cache::{ActionCacheEntry, ActionCacheIndex};
use crate::store_schema::append_shadow_jsonl;

pub const CACHEZERO_ENV: &str = "TOKENZERO_CACHEZERO";
pub const CACHEZERO_MODE_ENV: &str = "TOKENZERO_CACHEZERO_MODE";
pub const CACHEZERO_REL_DIR: &str = "cachezero";
pub const CACHEZERO_SHADOW_FILE: &str = "shadow.jsonl";
pub const CACHEZERO_GRADUATION_PCT: f64 = 20.0;
pub const CACHEZERO_STATS_SCHEMA: &str = "tokenzero.cachezero.stats.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CachezeroMode {
    #[default]
    Off,
    Shadow,
}

impl CachezeroMode {
    pub fn from_env() -> Self {
        Self::parse(
            std::env::var(CACHEZERO_ENV)
                .ok()
                .or_else(|| std::env::var(CACHEZERO_MODE_ENV).ok())
                .as_deref(),
        )
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("shadow") => Self::Shadow,
            _ => Self::Off,
        }
    }

    pub fn is_shadow(self) -> bool {
        matches!(self, Self::Shadow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheStatus {
    ExactHit,
    CausalHit,
    SwrStale,
    CollapsedWait,
    ForcedMiss,
}

impl CacheStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactHit => "exact-hit",
            Self::CausalHit => "causal-hit",
            Self::SwrStale => "swr-stale",
            Self::CollapsedWait => "collapsed-wait",
            Self::ForcedMiss => "forced-miss",
        }
    }

    pub fn would_have_hit(self) -> bool {
        matches!(self, Self::ExactHit | Self::CausalHit | Self::SwrStale)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowDecision {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
    pub blast_intersect: bool,
    pub result_digest: String,
    pub result_tokens: u64,
    pub wall_ms: u64,
    pub would_be_status: CacheStatus,
    pub artifact_class: String,
    pub saved_tokens_estimate: u64,
}

/// Classify a computed result against the live index. Never serves bytes.
pub fn classify_would_be_status(
    entry: Option<&ActionCacheEntry>,
    result_digest: &str,
    in_flight_serve: bool,
    blast_intersect: bool,
) -> CacheStatus {
    if in_flight_serve {
        return CacheStatus::CollapsedWait;
    }
    let Some(entry) = entry else {
        return CacheStatus::ForcedMiss;
    };
    let stored =
        crate::artifact_full_hash(&entry.artifact_ref).unwrap_or(entry.artifact_ref.as_str());
    if stored == result_digest {
        if entry.fszero_bookmark.is_some() && !blast_intersect {
            return CacheStatus::CausalHit;
        }
        return CacheStatus::ExactHit;
    }
    if entry.class == "swr" {
        return CacheStatus::SwrStale;
    }
    CacheStatus::ForcedMiss
}

pub fn store_root_from_cache_path(cache_path: &Path) -> PathBuf {
    let parent = cache_path.parent().unwrap_or(cache_path);
    if parent.file_name().and_then(|name| name.to_str()) == Some("tokenzero") {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

pub fn shadow_jsonl_path(store_root: &Path) -> PathBuf {
    store_root
        .join(CACHEZERO_REL_DIR)
        .join(CACHEZERO_SHADOW_FILE)
}

pub fn record_shadow_decision(store_root: &Path, decision: &ShadowDecision) -> io::Result<()> {
    let line = serde_json::to_string(decision).map_err(io::Error::other)?;
    append_shadow_jsonl(&shadow_jsonl_path(store_root), &line)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CachezeroClassStats {
    pub decisions: u64,
    pub would_have_hits: u64,
    pub saved_tokens_estimate: u64,
    pub by_status: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CachezeroStats {
    pub schema: &'static str,
    pub mode: &'static str,
    pub decisions: u64,
    pub would_have_hits: u64,
    pub would_have_hit_rate: f64,
    pub causal_hit_mass: u64,
    pub session_mass: u64,
    pub causal_hit_mass_pct: f64,
    pub graduation_gate_pct: f64,
    pub graduation: bool,
    pub saved_tokens_estimate: u64,
    pub by_class: BTreeMap<String, CachezeroClassStats>,
}

impl CachezeroStats {
    pub fn empty(mode: CachezeroMode) -> Self {
        Self {
            schema: CACHEZERO_STATS_SCHEMA,
            mode: match mode {
                CachezeroMode::Shadow => "shadow",
                CachezeroMode::Off => "off",
            },
            decisions: 0,
            would_have_hits: 0,
            would_have_hit_rate: 0.0,
            causal_hit_mass: 0,
            session_mass: 0,
            causal_hit_mass_pct: 0.0,
            graduation_gate_pct: CACHEZERO_GRADUATION_PCT,
            graduation: false,
            saved_tokens_estimate: 0,
            by_class: BTreeMap::new(),
        }
    }
}

pub fn aggregate_cachezero(store_root: &Path) -> io::Result<CachezeroStats> {
    let mut stats = CachezeroStats::empty(CachezeroMode::from_env());
    let path = shadow_jsonl_path(store_root);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(stats),
        Err(err) => return Err(err),
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(decision) = serde_json::from_str::<ShadowDecision>(line) else {
            continue;
        };
        stats.decisions += 1;
        stats.session_mass = stats.session_mass.saturating_add(decision.result_tokens);
        stats.saved_tokens_estimate = stats
            .saved_tokens_estimate
            .saturating_add(decision.saved_tokens_estimate);
        if decision.would_be_status.would_have_hit() {
            stats.would_have_hits += 1;
        }
        if decision.would_be_status == CacheStatus::CausalHit {
            stats.causal_hit_mass = stats.causal_hit_mass.saturating_add(decision.result_tokens);
        }
        let class = stats.by_class.entry(decision.artifact_class).or_default();
        class.decisions += 1;
        if decision.would_be_status.would_have_hit() {
            class.would_have_hits += 1;
        }
        class.saved_tokens_estimate = class
            .saved_tokens_estimate
            .saturating_add(decision.saved_tokens_estimate);
        *class
            .by_status
            .entry(decision.would_be_status.as_str().to_string())
            .or_insert(0) += 1;
    }
    if stats.decisions > 0 {
        stats.would_have_hit_rate = stats.would_have_hits as f64 / stats.decisions as f64;
    }
    if stats.session_mass > 0 {
        stats.causal_hit_mass_pct =
            (stats.causal_hit_mass as f64 / stats.session_mass as f64) * 100.0;
    }
    stats.graduation = stats.causal_hit_mass_pct > CACHEZERO_GRADUATION_PCT;
    Ok(stats)
}

pub fn cachezero_stats_json(store_root: &Path) -> serde_json::Value {
    match aggregate_cachezero(store_root) {
        Ok(stats) => serde_json::to_value(stats)
            .unwrap_or_else(|_| json!({"schema": CACHEZERO_STATS_SCHEMA, "error": "encode"})),
        Err(err) => json!({
            "schema": CACHEZERO_STATS_SCHEMA,
            "error": err.to_string(),
        }),
    }
}

/// Look up a live entry for classification. Missing or unreadable index is a miss.
pub fn live_entry_for_key(store_root: &Path, key: &str) -> Option<ActionCacheEntry> {
    ActionCacheIndex::open(store_root).get(key).ok().flatten()
}

#[cfg(test)]
mod cachezero_tests {
    use super::*;
    use crate::ActionCacheEntry;
    use tempfile::tempdir;

    fn entry(digest: &str, class: &str, bookmark: Option<&str>) -> ActionCacheEntry {
        ActionCacheEntry {
            key: "aa".repeat(32),
            artifact_ref: format!("tz://blob/{digest}"),
            fszero_bookmark: bookmark.map(str::to_string),
            dep_closure_ref: None,
            class: class.into(),
            verified: true,
            world_id: None,
            tombstone: false,
            tombstoned_at_unix: None,
        }
    }

    #[test]
    fn tz0zjn_classify_miss_hit_stale_causal_and_collapsed() {
        let digest = "bb".repeat(32);
        assert_eq!(
            classify_would_be_status(None, &digest, false, false),
            CacheStatus::ForcedMiss
        );
        assert_eq!(
            classify_would_be_status(
                Some(&entry(&digest, "must_block_revalidate", None)),
                &digest,
                false,
                false
            ),
            CacheStatus::ExactHit
        );
        assert_eq!(
            classify_would_be_status(
                Some(&entry(&digest, "must_block_revalidate", Some("bm"))),
                &digest,
                false,
                false
            ),
            CacheStatus::CausalHit
        );
        let other = "cc".repeat(32);
        assert_eq!(
            classify_would_be_status(Some(&entry(&other, "swr", None)), &digest, false, false),
            CacheStatus::SwrStale
        );
        assert_eq!(
            classify_would_be_status(
                Some(&entry(&digest, "must_block_revalidate", None)),
                &digest,
                true,
                false
            ),
            CacheStatus::CollapsedWait
        );
    }

    #[test]
    fn tz0zjn_shadow_ring_and_stats_graduation_gate() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let miss = ShadowDecision {
            key: "aa".repeat(32),
            bookmark: None,
            blast_intersect: false,
            result_digest: "dd".repeat(32),
            result_tokens: 80,
            wall_ms: 1,
            would_be_status: CacheStatus::ForcedMiss,
            artifact_class: "read".into(),
            saved_tokens_estimate: 0,
        };
        let causal = ShadowDecision {
            result_tokens: 15,
            would_be_status: CacheStatus::CausalHit,
            saved_tokens_estimate: 15,
            ..miss.clone()
        };
        record_shadow_decision(root, &miss).unwrap();
        record_shadow_decision(root, &causal).unwrap();
        let stats = aggregate_cachezero(root).unwrap();
        assert_eq!(stats.decisions, 2);
        assert_eq!(stats.would_have_hits, 1);
        assert_eq!(stats.session_mass, 95);
        assert_eq!(stats.causal_hit_mass, 15);
        assert!(stats.causal_hit_mass_pct < CACHEZERO_GRADUATION_PCT);
        assert!(!stats.graduation, "15/95 is under the 20% gate");
        assert_eq!(stats.by_class["read"].saved_tokens_estimate, 15);
        assert_eq!(shadow_jsonl_path(root), root.join("cachezero/shadow.jsonl"));
    }
}
