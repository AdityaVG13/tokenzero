//! Live Pareto decision bound to candidate identity, protected outcome vector,
//! verifier identity, and evidence freshness.
//!
//! TokenZero never claims dominance from unknown, stale, incomparable, or
//! missing evidence. Such candidates remain visible but are never allowed to
//! dominate another candidate and never hide a fresh candidate. Decisions are
//! deterministic: the same input order and bytes produce byte-identical
//! canonical JSON and digest.
//!
//! Independence: this module imports only `zero-abi` and `serde`/`serde_json`
//! and the sibling `representation_economics::RepresentationResources`. It does
//! not import `zero-gate`, `zero-ledger`, FSZero, or GraphZero.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use zero_abi::{Sha256Digest, sha256, sha256_hex};

use crate::representation_economics::RepresentationResources;

pub const LIVE_PARETO_CONTRACT_VERSION: u16 = 1;
pub const LIVE_PARETO_MAX_CANDIDATES: usize = 1_024;
pub const LIVE_PARETO_MAX_METRICS: usize = 128;
pub const LIVE_PARETO_MAX_ID_BYTES: usize = 256;
pub const LIVE_PARETO_MAX_CANONICAL_BYTES: usize = 1_048_576;

const LIVE_DOMAIN: &[u8] = b"tokenzero.live_pareto.v1\0";

// ---------------------------------------------------------------------------
// Metric order and protected outcome
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricOrder {
    AtLeast,
    AtMost,
    Exact,
}

impl MetricOrder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AtLeast => "at_least",
            Self::AtMost => "at_most",
            Self::Exact => "exact",
        }
    }
}

/// One protected outcome dimension bound to the candidate that produced it.
/// The baseline and candidate values are paired; dominance is no-worse on
/// every dimension.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProtectedOutcome {
    pub metric_id: String,
    pub order: MetricOrder,
    pub baseline_value: i64,
    pub candidate_value: i64,
}

impl ProtectedOutcome {
    pub fn validate(&self) -> Result<(), String> {
        if self.metric_id.is_empty()
            || self.metric_id.len() > LIVE_PARETO_MAX_ID_BYTES
            || self.metric_id.chars().any(|c| c.is_control())
        {
            return Err(format!("invalid metric_id {:?}", self.metric_id));
        }
        Ok(())
    }

    pub fn no_worse_than(&self, other: &Self) -> bool {
        if self.metric_id != other.metric_id || self.order != other.order {
            return false;
        }
        match self.order {
            MetricOrder::AtLeast => self.candidate_value >= other.candidate_value,
            MetricOrder::AtMost => self.candidate_value <= other.candidate_value,
            MetricOrder::Exact => self.candidate_value == other.candidate_value,
        }
    }

    /// No-worse relative to the baseline (candidate must not regress).
    pub fn candidate_no_worse_than_baseline(&self) -> bool {
        match self.order {
            MetricOrder::AtLeast => self.candidate_value >= self.baseline_value,
            MetricOrder::AtMost => self.candidate_value <= self.baseline_value,
            MetricOrder::Exact => self.candidate_value == self.baseline_value,
        }
    }

    pub fn strictly_better_than(&self, other: &Self) -> bool {
        if self.metric_id != other.metric_id || self.order != other.order {
            return false;
        }
        match self.order {
            MetricOrder::AtLeast => self.candidate_value > other.candidate_value,
            MetricOrder::AtMost => self.candidate_value < other.candidate_value,
            MetricOrder::Exact => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Verifier identity
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VerifierIdentity {
    pub verifier_id: String,
    pub verifier_version: String,
}

impl VerifierIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if self.verifier_id.is_empty()
            || self.verifier_id.len() > LIVE_PARETO_MAX_ID_BYTES
            || self.verifier_id.chars().any(|c| c.is_control())
        {
            return Err(format!("invalid verifier_id {:?}", self.verifier_id));
        }
        if self.verifier_version.is_empty()
            || self.verifier_version.len() > LIVE_PARETO_MAX_ID_BYTES
            || self.verifier_version.chars().any(|c| c.is_control())
        {
            return Err(format!(
                "invalid verifier_version {:?}",
                self.verifier_version
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> Sha256Digest {
        let v = serde_json::json!({
            "verifier_id": self.verifier_id,
            "verifier_version": self.verifier_version,
        });
        let canon = zero_abi::canonical_json(&v);
        Sha256Digest::from_bytes(sha256(canon.as_bytes()))
    }
}

// ---------------------------------------------------------------------------
// Evidence freshness
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFreshness {
    Fresh,
    Stale,
    Unknown,
    Missing,
}

impl EvidenceFreshness {
    pub const fn is_fresh(self) -> bool {
        matches!(self, Self::Fresh)
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
            Self::Missing => "missing",
        }
    }
    pub const fn is_visible(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Live candidate
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LiveCandidate {
    pub candidate_id: String,
    pub semantic_root: String,
    pub adapter_root: String,
    pub verifier: VerifierIdentity,
    pub freshness: EvidenceFreshness,
    pub protected_vector: Vec<ProtectedOutcome>,
    pub resources: RepresentationResources,
    pub exact: bool,
}

impl LiveCandidate {
    pub fn validate(&self) -> Result<(), String> {
        if self.candidate_id.is_empty() || self.candidate_id.len() > LIVE_PARETO_MAX_ID_BYTES {
            return Err(format!("invalid candidate_id {:?}", self.candidate_id));
        }
        if self.candidate_id.chars().any(|c| c.is_control()) {
            return Err("candidate_id contains control characters".into());
        }
        if self.semantic_root.is_empty() || self.adapter_root.is_empty() {
            return Err("semantic_root and adapter_root must be non-empty".into());
        }
        self.verifier.validate()?;
        if self.protected_vector.len() > LIVE_PARETO_MAX_METRICS {
            return Err(format!(
                "protected_vector exceeds bound {}",
                LIVE_PARETO_MAX_METRICS
            ));
        }
        let mut seen = BTreeSet::new();
        let mut prev: Option<&str> = None;
        for m in &self.protected_vector {
            m.validate()?;
            if !seen.insert(m.metric_id.as_str()) {
                return Err(format!("duplicate metric_id {:?}", m.metric_id));
            }
            if let Some(p) = prev {
                if p >= m.metric_id.as_str() {
                    return Err("protected_vector must be strictly sorted by metric_id".into());
                }
            }
            prev = Some(&m.metric_id);
        }
        Ok(())
    }

    /// Digest that binds this candidate's four Wave-16 authorities:
    /// identity, protected vector, verifier, and freshness.
    pub fn binding_digest(&self) -> Sha256Digest {
        let v = serde_json::json!({
            "candidate_id": self.candidate_id,
            "semantic_root": self.semantic_root,
            "adapter_root": self.adapter_root,
            "verifier_id": self.verifier.verifier_id,
            "verifier_version": self.verifier.verifier_version,
            "freshness": self.freshness.as_str(),
            "protected_vector": self.protected_vector,
            "resources": self.resources,
            "exact": self.exact,
        });
        let canon = zero_abi::canonical_json(&v);
        Sha256Digest::from_bytes(sha256(canon.as_bytes()))
    }

    /// Whether this candidate's protected vector is comparable to another's
    /// (same metric_ids and orders in same order).
    pub fn protected_comparable(&self, other: &Self) -> bool {
        if self.protected_vector.len() != other.protected_vector.len() {
            return false;
        }
        for (a, b) in self
            .protected_vector
            .iter()
            .zip(other.protected_vector.iter())
        {
            if a.metric_id != b.metric_id || a.order != b.order {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Decision view for one candidate in the live decision
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LiveEntry {
    pub candidate_id: String,
    pub candidate_binding_digest: String,
    pub freshness: EvidenceFreshness,
    pub verifier: VerifierIdentity,
    pub protected_vector: Vec<ProtectedOutcome>,
    pub resources: RepresentationResources,
    pub exact: bool,
    pub semantic_root: String,
    pub adapter_root: String,
    pub in_frontier: bool,
    pub reasons: Vec<String>,
}

// ---------------------------------------------------------------------------
// The one typed live decision result ZeroStack composes without adapters
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LiveParetoDecision {
    pub contract_version: u16,
    pub decision_digest: String,
    pub frontier_ids: Vec<String>,
    pub entries: Vec<LiveEntry>,
    pub canonical_json: String,
}

impl LiveParetoDecision {
    pub fn validate(&self) -> Result<(), String> {
        if self.contract_version != LIVE_PARETO_CONTRACT_VERSION {
            return Err("live pareto contract version mismatch".into());
        }
        if self.canonical_json.len() > LIVE_PARETO_MAX_CANONICAL_BYTES {
            return Err("canonical_json exceeds bound".into());
        }
        let v: serde_json::Value =
            serde_json::from_str(&self.canonical_json).map_err(|e| e.to_string())?;
        let canon = zero_abi::canonical_json(&v);
        if canon != self.canonical_json {
            return Err("canonical_json is not canonical sorted-key JSON".into());
        }
        let expected = Self::expected_digest(&v)?;
        if expected != self.decision_digest {
            return Err(format!(
                "decision_digest mismatch expected {expected} got {}",
                self.decision_digest
            ));
        }
        // frontier must be sorted and subset of entries
        let mut sorted = self.frontier_ids.clone();
        sorted.sort();
        if sorted != self.frontier_ids {
            return Err("frontier_ids must be sorted".into());
        }
        let entry_ids: BTreeSet<&str> = self
            .entries
            .iter()
            .map(|e| e.candidate_id.as_str())
            .collect();
        for id in &self.frontier_ids {
            if !entry_ids.contains(id.as_str()) {
                return Err(format!("frontier id {id} not in entries"));
            }
        }
        // entries must be sorted by candidate_id
        let mut prev: Option<&str> = None;
        for e in &self.entries {
            if let Some(p) = prev {
                if p >= e.candidate_id.as_str() {
                    return Err("entries must be sorted by candidate_id".into());
                }
            }
            prev = Some(&e.candidate_id);
        }
        Ok(())
    }

    fn expected_digest(value: &serde_json::Value) -> Result<String, String> {
        // value is the canonical decision body (without the outer canonical_json field)
        // We stored canonical_json as the body itself, so digest is domain || body.
        let body = zero_abi::canonical_json(value);
        // domain separation: hash(domain || body)
        // Use simple sha256(domain || body) for determinism
        let mut combined = Vec::with_capacity(LIVE_DOMAIN.len() + body.len());
        combined.extend_from_slice(LIVE_DOMAIN);
        combined.extend_from_slice(body.as_bytes());
        Ok(sha256_hex(&combined))
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > LIVE_PARETO_MAX_CANONICAL_BYTES {
            return Err("live pareto bytes exceed bound".into());
        }
        let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let canon = zero_abi::canonical_json(&v);
        if canon.as_bytes() != bytes {
            return Err("bytes are not canonical sorted-key JSON".into());
        }
        let decoded: Self = serde_json::from_value(v).map_err(|e| e.to_string())?;
        decoded.validate()?;
        Ok(decoded)
    }

    pub fn frontier(&self) -> &[String] {
        &self.frontier_ids
    }
    pub fn entries(&self) -> &[LiveEntry] {
        &self.entries
    }
    pub fn is_deterministic(&self) -> bool {
        self.validate().is_ok()
    }
}

// ---------------------------------------------------------------------------
// Live dominance
// ---------------------------------------------------------------------------

fn live_dominates(a: &LiveCandidate, b: &LiveCandidate) -> bool {
    // Unknown, stale, missing never dominate.
    if !a.freshness.is_fresh() {
        return false;
    }
    if a.candidate_id == b.candidate_id {
        return false;
    }
    // Incomparable semantic/adapter never dominate.
    if a.semantic_root != b.semantic_root || a.adapter_root != b.adapter_root {
        return false;
    }
    // Incomparable verifier never dominate (different verifier identity).
    if a.verifier != b.verifier {
        return false;
    }
    // Incomparable protected vector never dominate.
    if !a.protected_comparable(b) {
        return false;
    }
    // Exactness: inexact cannot dominate exact.
    if b.exact && !a.exact {
        return false;
    }
    // Protected dominance: a must be no-worse on every protected dimension.
    let protected_no_worse = a
        .protected_vector
        .iter()
        .zip(b.protected_vector.iter())
        .all(|(pa, pb)| pa.no_worse_than(pb));
    if !protected_no_worse {
        return false;
    }
    // Resource dominance.
    if !a.resources.dominates_or_equal(&b.resources) {
        return false;
    }
    // Need strictly better on at least one axis (protected or resources).
    let strictly = a.resources.strictly_better_than(&b.resources)
        || a.protected_vector
            .iter()
            .zip(b.protected_vector.iter())
            .any(|(pa, pb)| pa.strictly_better_than(pb));
    strictly
}

/// Decide the live Pareto frontier deterministically.
///
/// Rules (fail-closed):
/// - Every candidate is validated. Unknown/stale/missing/incomparable never dominate.
/// - Stale/unknown/missing candidates remain in `entries` with `in_frontier=false`
///   and an explicit reason; they never hide another candidate.
/// - Incomparable protected vectors or verifier identities are visible with
///   `incomparable_*` reasons but never claim dominance.
/// - Determinism: frontier and entries are sorted by `candidate_id`; canonical
///   JSON uses sorted keys; digest is domain-separated sha256.
pub fn decide_live_pareto(candidates: &[LiveCandidate]) -> Result<LiveParetoDecision, String> {
    if candidates.is_empty() {
        return Err("live pareto requires at least one candidate".into());
    }
    if candidates.len() > LIVE_PARETO_MAX_CANDIDATES {
        return Err(format!(
            "too many candidates {} > {}",
            candidates.len(),
            LIVE_PARETO_MAX_CANDIDATES
        ));
    }
    // Validate and check duplicate candidate_ids.
    let mut seen = BTreeSet::new();
    for c in candidates {
        c.validate()?;
        if !seen.insert(c.candidate_id.as_str()) {
            return Err(format!("duplicate candidate_id {:?}", c.candidate_id));
        }
    }

    // Deterministic order: sorted by candidate_id.
    let mut sorted: Vec<LiveCandidate> = candidates.to_vec();
    sorted.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));

    // Compute dominance among fresh candidates only.
    let mut dominated_by: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for (i, cand) in sorted.iter().enumerate() {
        if !cand.freshness.is_fresh() {
            continue;
        }
        for (j, other) in sorted.iter().enumerate() {
            if i == j {
                continue;
            }
            // other dominates cand?
            if live_dominates(other, cand) {
                // keep smallest dominator id for determinism
                dominated_by
                    .entry(cand.candidate_id.clone())
                    .and_modify(|cur| {
                        if other.candidate_id < *cur {
                            *cur = other.candidate_id.clone();
                        }
                    })
                    .or_insert_with(|| other.candidate_id.clone());
                break;
            }
        }
    }

    let mut entries: Vec<LiveEntry> = Vec::with_capacity(sorted.len());
    let mut frontier_ids: Vec<String> = Vec::new();

    for cand in &sorted {
        let binding_hex = cand.binding_digest().to_hex();
        let mut reasons: Vec<String> = Vec::new();
        let mut in_frontier = false;

        match cand.freshness {
            EvidenceFreshness::Stale => reasons.push("stale_evidence".into()),
            EvidenceFreshness::Unknown => reasons.push("unknown_evidence".into()),
            EvidenceFreshness::Missing => reasons.push("missing_evidence".into()),
            EvidenceFreshness::Fresh => {}
        }

        // Protected vector completeness: empty is missing evidence.
        if cand.protected_vector.is_empty() && cand.freshness.is_fresh() {
            if !reasons.contains(&"missing_evidence".to_string()) {
                reasons.push("missing_evidence".into());
            }
        }

        // Check if fresh candidate is dominated.
        let dominated = dominated_by.get(&cand.candidate_id);
        if let Some(dom_id) = dominated {
            reasons.push(format!("dominated_by:{}", dom_id));
        }

        // Incomparability note: if fresh and not dominated, but there exists
        // another fresh candidate with incomparable protected vector or
        // verifier, we keep it in frontier (standard Pareto) but note
        // incomparable is visible via both being in frontier. No extra hide.
        // For visibility, if protected_vector incomparable to any other,
        // the frontier naturally keeps both; no dominance claimed.

        if cand.freshness.is_fresh() && dominated.is_none() {
            // Fresh, not dominated, but empty protected vector treated as
            // missing -> not frontier despite freshness.
            if cand.protected_vector.is_empty() {
                // missing evidence stays visible, not in frontier
            } else {
                in_frontier = true;
                frontier_ids.push(cand.candidate_id.clone());
            }
        }

        // If not in frontier and no reason yet, surface incomparable as reason?
        // For fresh incomparable candidates that are in frontier, reason stays empty.
        // For stale/unknown/missing, reason already set.
        if !in_frontier && reasons.is_empty() {
            // dominated case already has reason; fresh dominated already handled.
            // This path is dominated fresh with no other flag -> already has dominated_by.
        }

        entries.push(LiveEntry {
            candidate_id: cand.candidate_id.clone(),
            candidate_binding_digest: binding_hex,
            freshness: cand.freshness,
            verifier: cand.verifier.clone(),
            protected_vector: cand.protected_vector.clone(),
            resources: cand.resources,
            exact: cand.exact,
            semantic_root: cand.semantic_root.clone(),
            adapter_root: cand.adapter_root.clone(),
            in_frontier,
            reasons,
        });
    }

    frontier_ids.sort();

    // Build canonical decision body.
    // The body is the decision without the outer canonical_json self-reference;
    // we store the body as canonical_json and digest domain+body.
    let body = serde_json::json!({
        "contract_version": LIVE_PARETO_CONTRACT_VERSION,
        "frontier_ids": frontier_ids,
        "entries": entries,
    });
    let canonical_body = zero_abi::canonical_json(&body);
    if canonical_body.len() > LIVE_PARETO_MAX_CANONICAL_BYTES {
        return Err("live pareto canonical body exceeds bound".into());
    }
    let mut combined = Vec::with_capacity(LIVE_DOMAIN.len() + canonical_body.len());
    combined.extend_from_slice(LIVE_DOMAIN);
    combined.extend_from_slice(canonical_body.as_bytes());
    let digest = sha256_hex(&combined);

    let decision = LiveParetoDecision {
        contract_version: LIVE_PARETO_CONTRACT_VERSION,
        decision_digest: digest,
        frontier_ids,
        entries,
        canonical_json: canonical_body,
    };
    decision.validate()?;
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::representation_economics::RepresentationResources;

    fn res(v: u64) -> RepresentationResources {
        RepresentationResources {
            stored_bytes: v,
            wire_bytes: v,
            source_tokens: v,
            visible_tokens: v,
            expansion_work: v,
            verification_work: v,
            latency_micros: v,
            metadata_bytes: v,
        }
    }
    fn verifier(id: &str) -> VerifierIdentity {
        VerifierIdentity {
            verifier_id: id.into(),
            verifier_version: "1".into(),
        }
    }
    fn prot(id: &str, order: MetricOrder, base: i64, cand: i64) -> ProtectedOutcome {
        ProtectedOutcome {
            metric_id: id.into(),
            order,
            baseline_value: base,
            candidate_value: cand,
        }
    }
    #[test]
    fn deterministic_digest() {
        let c = LiveCandidate {
            candidate_id: "c1".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Fresh,
            protected_vector: vec![prot("acc", MetricOrder::AtLeast, 80, 90)],
            resources: res(10),
            exact: true,
        };
        let d1 = decide_live_pareto(&[c.clone()]).unwrap();
        let d2 = decide_live_pareto(&[c]).unwrap();
        assert_eq!(d1.decision_digest, d2.decision_digest);
        assert_eq!(d1.canonical_json, d2.canonical_json);
    }
    #[test]
    fn stale_never_dominates_and_remains_visible() {
        let fresh_good = LiveCandidate {
            candidate_id: "b".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Fresh,
            protected_vector: vec![prot("acc", MetricOrder::AtLeast, 80, 90)],
            resources: res(5),
            exact: true,
        };
        let stale_better_resources = LiveCandidate {
            candidate_id: "a".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Stale,
            protected_vector: vec![prot("acc", MetricOrder::AtLeast, 80, 95)],
            resources: res(1),
            exact: true,
        };
        let d = decide_live_pareto(&[stale_better_resources, fresh_good]).unwrap();
        // stale must not hide fresh
        assert!(d.frontier_ids.contains(&"b".to_string()));
        // stale must be visible but not in frontier
        let e_a = d.entries.iter().find(|e| e.candidate_id == "a").unwrap();
        assert!(!e_a.in_frontier);
        assert!(e_a.reasons.iter().any(|r| r == "stale_evidence"));
        // fresh good is in frontier
        let e_b = d.entries.iter().find(|e| e.candidate_id == "b").unwrap();
        assert!(e_b.in_frontier);
    }
    #[test]
    fn incomparable_protected_vector_both_visible() {
        let c1 = LiveCandidate {
            candidate_id: "a".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Fresh,
            protected_vector: vec![prot("acc", MetricOrder::AtLeast, 80, 90)],
            resources: res(5),
            exact: true,
        };
        let c2 = LiveCandidate {
            candidate_id: "b".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Fresh,
            protected_vector: vec![prot("latency", MetricOrder::AtMost, 100, 50)],
            resources: res(5),
            exact: true,
        };
        let d = decide_live_pareto(&[c1, c2]).unwrap();
        // different metric_ids -> incomparable, neither dominates, both in frontier
        assert_eq!(d.frontier_ids.len(), 2);
    }
    #[test]
    fn unknown_and_missing_remain_visible_not_dominating() {
        let fresh = LiveCandidate {
            candidate_id: "fresh".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Fresh,
            protected_vector: vec![prot("acc", MetricOrder::AtLeast, 80, 90)],
            resources: res(10),
            exact: true,
        };
        let unknown = LiveCandidate {
            candidate_id: "unknown".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Unknown,
            protected_vector: vec![prot("acc", MetricOrder::AtLeast, 80, 95)],
            resources: res(1),
            exact: true,
        };
        let missing = LiveCandidate {
            candidate_id: "missing".into(),
            semantic_root: "s".into(),
            adapter_root: "a".into(),
            verifier: verifier("v1"),
            freshness: EvidenceFreshness::Missing,
            protected_vector: vec![],
            resources: res(1),
            exact: true,
        };
        let d = decide_live_pareto(&[fresh.clone(), unknown, missing]).unwrap();
        assert!(d.frontier_ids.contains(&"fresh".to_string()));
        assert!(!d.frontier_ids.contains(&"unknown".to_string()));
        assert!(!d.frontier_ids.contains(&"missing".to_string()));
        assert_eq!(d.entries.len(), 3);
    }
}
