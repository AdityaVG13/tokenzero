//! FeatureUniverse loader for the gauntlet surface matrix.
//!
//! Lives in `tokenzero-test-support` (no `tokenzero-harness` crate).
//! Partial NEVER rounds up to Passing. Excluded is still strict-100 debt.
//! MCP `EngineIdentity::TokenZero` is Excluded as a gauntlet oracle (F-TZ-018).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Frozen copy of Phase 2 `supported_surface_matrix.toml`. Tests of the sum
/// invariant use this. Workspace file must byte-match (see `embedded_matrix_sha256`).
pub const EMBEDDED_SURFACE_MATRIX: &str = include_str!("fixtures/supported_surface_matrix.toml");

/// Env var: absolute path to `supported_surface_matrix.toml`.
pub const SURFACE_MATRIX_PATH_ENV: &str = "TOKENZERO_GAUNTLET_SURFACE_MATRIX";

/// Env var: gauntlet workspace root. Loader then reads
/// `docs/contracts/supported_surface_matrix.toml`.
pub const GAUNTLET_WORKSPACE_ENV: &str = "TOKENZERO_GAUNTLET_WORKSPACE";

pub const FORBIDDEN_MCP_FEATURE_ID: &str = "F-TZ-018";
pub const STRICT_MODE_FEATURE_ID: &str = "F-TZ-011";

/// Truncate to 6 decimal places (toward zero). x86 vs ARM vs WASM LSB noise.
pub fn truncate_score(x: f64) -> f64 {
    (x * 1_000_000.0).trunc() / 1_000_000.0
}

/// FEATURE-UNIVERSE.md: weight sums use abs(sum-1) < 1e-9, not trunc-equality.
/// `0.35+0.30+0.20+0.15` is 1.0 in the TOML and within 1e-9 in f64, but
/// `truncate_score(sum)` is `0.999999` because 0.35 is not a binary 6-decimal.
/// Rejecting that would fail-close the Phase 2 matrix. Trunc-equality still
/// rejects 0.60+0.50 (1.10) and 0.40+0.50 (0.90).
const WEIGHT_SUM_ABS_TOL: f64 = 1e-9;

fn sums_to_one(sum: f64) -> bool {
    (sum - 1.0).abs() < WEIGHT_SUM_ABS_TOL
}

fn canonical_unit_sum(sum: f64) -> f64 {
    if sums_to_one(sum) {
        truncate_score(1.0)
    } else {
        truncate_score(sum)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeatureId(pub String);

impl FeatureId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Matrix `supported`/`present` → Passing. Partial never becomes Passing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParityStatus {
    Passing,
    Partial,
    Missing,
    Excluded,
}

impl ParityStatus {
    /// Only `Passing` counts. Partial is never treated as a pass.
    pub fn counts_as_passing(self) -> bool {
        matches!(self, Self::Passing)
    }

    /// Score contribution. Partial is 0.5 and MUST NOT be rounded to 1.0.
    pub fn score_contribution(self) -> f64 {
        match self {
            Self::Passing => 1.0,
            Self::Partial => 0.5,
            Self::Missing | Self::Excluded => 0.0,
        }
    }

    /// Excluded still counts as coverage debt for a strict-100% claim.
    pub fn is_strict_100_debt(self) -> bool {
        matches!(self, Self::Excluded | Self::Missing | Self::Partial)
    }
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub id: FeatureId,
    pub title: String,
    pub category: String,
    pub weight: f64,
    pub status: ParityStatus,
    pub exclusion_rationale: Option<String>,
    pub partial_rationale: Option<String>,
    pub missing_rationale: Option<String>,
    pub authority_axis: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FeatureUniverse {
    features: BTreeMap<FeatureId, Feature>,
    category_weights: BTreeMap<String, f64>,
    origin: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub total: usize,
    pub passing: usize,
    pub partial: usize,
    pub missing: usize,
    pub excluded: usize,
    pub per_category: BTreeMap<String, CategoryStats>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategoryStats {
    pub total: usize,
    pub passing: usize,
    pub partial: usize,
    pub missing: usize,
    pub excluded: usize,
    pub weight_sum: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    WeightSum {
        category: String,
        sum: String,
        expected: String,
    },
    DuplicateId(String),
    UnknownStatus {
        id: String,
        status: String,
    },
    ExcludedWithoutRationale(String),
    MissingCategory {
        id: String,
        category: String,
    },
}

#[derive(Debug)]
pub enum LoaderError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Toml {
        origin: String,
        message: String,
    },
    Validation(Vec<Violation>),
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "read {}: {source}", path.display())
            }
            Self::Toml { origin, message } => {
                write!(f, "toml parse ({origin}): {message}")
            }
            Self::Validation(v) => {
                write!(f, "surface matrix rejected ({} violation(s)): ", v.len())?;
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    match item {
                        Violation::WeightSum {
                            category,
                            sum,
                            expected,
                        } => write!(
                            f,
                            "weight sum for {category} is {sum} after truncate_score (expected {expected})"
                        )?,
                        Violation::DuplicateId(id) => write!(f, "duplicate FeatureId {id}")?,
                        Violation::UnknownStatus { id, status } => {
                            write!(f, "unknown status {status:?} on {id}")?
                        }
                        Violation::ExcludedWithoutRationale(id) => {
                            write!(f, "excluded {id} missing exclusion_rationale")?
                        }
                        Violation::MissingCategory { id, category } => {
                            write!(f, "{id} category {category} is not in [categories]")?
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LoaderError {}

#[derive(Debug, Deserialize)]
struct MatrixFile {
    #[serde(default)]
    categories: BTreeMap<String, MatrixCategory>,
    #[serde(default)]
    features: Vec<MatrixFeature>,
}

#[derive(Debug, Deserialize)]
struct MatrixCategory {
    weight: f64,
}

#[derive(Debug, Deserialize)]
struct MatrixFeature {
    id: String,
    title: String,
    category: String,
    weight: f64,
    status: String,
    #[serde(default)]
    exclusion_rationale: Option<String>,
    #[serde(default)]
    partial_rationale: Option<String>,
    #[serde(default)]
    missing_rationale: Option<String>,
    #[serde(default)]
    authority_axis: Option<u32>,
}

impl ParityStatus {
    fn from_matrix_status(raw: &str) -> Option<Self> {
        match raw {
            "supported" | "present" => Some(Self::Passing),
            "partial" => Some(Self::Partial),
            "missing" => Some(Self::Missing),
            "excluded" => Some(Self::Excluded),
            // n/a is skip, not Passing. Reject here so it cannot round up.
            "n/a" | "na" => None,
            _ => None,
        }
    }
}

impl FeatureUniverse {
    pub fn load_from_toml(path: &Path) -> Result<Self, LoaderError> {
        let src = std::fs::read_to_string(path).map_err(|source| LoaderError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::load_from_str(&src, &path.display().to_string())
    }

    pub fn load_from_str(src: &str, origin: &str) -> Result<Self, LoaderError> {
        let parsed: MatrixFile = toml::from_str(src).map_err(|err| LoaderError::Toml {
            origin: origin.to_string(),
            message: err.to_string(),
        })?;
        Self::from_parsed(parsed, origin)
    }

    pub fn load_embedded() -> Result<Self, LoaderError> {
        Self::load_from_str(
            EMBEDDED_SURFACE_MATRIX,
            "embedded:supported_surface_matrix.toml",
        )
    }

    /// Prefer `TOKENZERO_GAUNTLET_SURFACE_MATRIX`, then
    /// `$TOKENZERO_GAUNTLET_WORKSPACE/docs/contracts/supported_surface_matrix.toml`,
    /// else the frozen embed.
    pub fn load_from_env_or_embedded() -> Result<Self, LoaderError> {
        if let Some(path) = std::env::var_os(SURFACE_MATRIX_PATH_ENV) {
            return Self::load_from_toml(Path::new(&path));
        }
        if let Some(ws) = std::env::var_os(GAUNTLET_WORKSPACE_ENV) {
            let path = Path::new(&ws).join("docs/contracts/supported_surface_matrix.toml");
            return Self::load_from_toml(&path);
        }
        Self::load_embedded()
    }

    pub fn embedded_matrix_sha256() -> String {
        sha256_hex(EMBEDDED_SURFACE_MATRIX.as_bytes())
    }

    fn from_parsed(parsed: MatrixFile, origin: &str) -> Result<Self, LoaderError> {
        let mut features = BTreeMap::new();
        let mut violations = Vec::new();
        let category_weights: BTreeMap<String, f64> = parsed
            .categories
            .iter()
            .map(|(k, v)| (k.clone(), v.weight))
            .collect();

        for raw in parsed.features {
            if !category_weights.contains_key(&raw.category) {
                violations.push(Violation::MissingCategory {
                    id: raw.id.clone(),
                    category: raw.category.clone(),
                });
            }
            let Some(status) = ParityStatus::from_matrix_status(&raw.status) else {
                violations.push(Violation::UnknownStatus {
                    id: raw.id.clone(),
                    status: raw.status.clone(),
                });
                continue;
            };
            if status == ParityStatus::Excluded
                && raw
                    .exclusion_rationale
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                violations.push(Violation::ExcludedWithoutRationale(raw.id.clone()));
            }
            let id = FeatureId(raw.id.clone());
            if features.contains_key(&id) {
                violations.push(Violation::DuplicateId(raw.id.clone()));
                continue;
            }
            features.insert(
                id.clone(),
                Feature {
                    id,
                    title: raw.title,
                    category: raw.category,
                    weight: raw.weight,
                    status,
                    exclusion_rationale: raw.exclusion_rationale,
                    partial_rationale: raw.partial_rationale,
                    missing_rationale: raw.missing_rationale,
                    authority_axis: raw.authority_axis,
                },
            );
        }

        let universe = Self {
            features,
            category_weights,
            origin: origin.to_string(),
        };
        violations.extend(universe.weight_sum_violations());
        if !violations.is_empty() {
            return Err(LoaderError::Validation(violations));
        }
        Ok(universe)
    }

    fn weight_sum_violations(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        let expected = format!("{:.6}", truncate_score(1.0));
        let mut by_cat: BTreeMap<&str, f64> = BTreeMap::new();
        for feat in self.features.values() {
            *by_cat.entry(feat.category.as_str()).or_insert(0.0) += feat.weight;
        }
        for (category, sum) in by_cat {
            if !sums_to_one(sum) {
                out.push(Violation::WeightSum {
                    category: category.to_string(),
                    sum: format!("{:.6}", canonical_unit_sum(sum)),
                    expected: expected.clone(),
                });
            }
        }
        let cat_weight_sum: f64 = self.category_weights.values().copied().sum();
        if !sums_to_one(cat_weight_sum) {
            out.push(Violation::WeightSum {
                category: "<global-category-weights>".to_string(),
                sum: format!("{:.6}", canonical_unit_sum(cat_weight_sum)),
                expected,
            });
        }
        out
    }

    pub fn features(&self) -> impl Iterator<Item = &Feature> {
        self.features.values()
    }

    pub fn by_category<'a>(&'a self, cat: &'a str) -> impl Iterator<Item = &'a Feature> + 'a {
        self.features.values().filter(move |f| f.category == cat)
    }

    pub fn get(&self, id: &str) -> Option<&Feature> {
        self.features.get(&FeatureId(id.to_string()))
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn validate(&self) -> Vec<Violation> {
        self.weight_sum_violations()
    }

    pub fn stats(&self) -> Stats {
        let mut passing = 0;
        let mut partial = 0;
        let mut missing = 0;
        let mut excluded = 0;
        let mut per_category: BTreeMap<String, CategoryStats> = BTreeMap::new();
        for feat in self.features.values() {
            match feat.status {
                ParityStatus::Passing => passing += 1,
                ParityStatus::Partial => partial += 1,
                ParityStatus::Missing => missing += 1,
                ParityStatus::Excluded => excluded += 1,
            }
            let entry = per_category
                .entry(feat.category.clone())
                .or_insert_with(|| CategoryStats {
                    total: 0,
                    passing: 0,
                    partial: 0,
                    missing: 0,
                    excluded: 0,
                    weight_sum: 0.0,
                });
            entry.total += 1;
            entry.weight_sum += feat.weight;
            match feat.status {
                ParityStatus::Passing => entry.passing += 1,
                ParityStatus::Partial => entry.partial += 1,
                ParityStatus::Missing => entry.missing += 1,
                ParityStatus::Excluded => entry.excluded += 1,
            }
        }
        for stats in per_category.values_mut() {
            stats.weight_sum = canonical_unit_sum(stats.weight_sum);
        }
        Stats {
            total: self.features.len(),
            passing,
            partial,
            missing,
            excluded,
            per_category,
        }
    }

    /// Weighted effective coverage. Partial contributes 0.5, never 1.0.
    pub fn effective_coverage(&self) -> f64 {
        let mut acc = 0.0;
        for feat in self.features.values() {
            let cat_w = self
                .category_weights
                .get(&feat.category)
                .copied()
                .unwrap_or(0.0);
            acc += cat_w * feat.weight * feat.status.score_contribution();
        }
        truncate_score(acc)
    }

    /// Same as effective_coverage but treating Partial as Passing (FORBIDDEN).
    /// Used only to prove the loader does not round up.
    pub fn coverage_if_partial_rounded_up(&self) -> f64 {
        let mut acc = 0.0;
        for feat in self.features.values() {
            let cat_w = self
                .category_weights
                .get(&feat.category)
                .copied()
                .unwrap_or(0.0);
            let contrib = match feat.status {
                ParityStatus::Passing | ParityStatus::Partial => 1.0,
                ParityStatus::Missing | ParityStatus::Excluded => 0.0,
            };
            acc += cat_w * feat.weight * contrib;
        }
        truncate_score(acc)
    }

    /// Strict-100% fails if any Excluded, Missing, or Partial remains.
    pub fn strict_100_certifiable(&self) -> bool {
        let s = self.stats();
        s.excluded == 0 && s.missing == 0 && s.partial == 0 && s.passing == s.total && s.total > 0
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gauntlet::{
        assert_distinct, is_forbidden_gauntlet_identity, GauntletOracle,
        FORBIDDEN_MCP_ENGINE_IDENTITY, FORBIDDEN_MCP_REGISTRY_ENGINE, SUBJECT_IDENTITY,
    };
    use std::panic::catch_unwind;

    fn universe() -> FeatureUniverse {
        FeatureUniverse::load_from_env_or_embedded().expect("surface matrix must load")
    }

    #[test]
    fn truncate_score_six_decimals() {
        assert_eq!(truncate_score(0.123456789), 0.123456);
        assert_eq!(truncate_score(1.0), 1.0);
        assert_eq!(truncate_score(0.9999994), 0.999999);
        assert_ne!(truncate_score(0.999999), 1.0);
        // 1.000001 is not an exact f64; 1.0+1.5e-6 is safely above 1.000001.
        assert_eq!(truncate_score(1.0 + 1.5e-6), 1.000001);
        assert!(sums_to_one(0.35 + 0.30 + 0.20 + 0.15));
        assert!(!sums_to_one(0.60 + 0.50));
    }

    #[test]
    fn weights_sum_to_one() {
        let u = FeatureUniverse::load_embedded().expect("embedded matrix");
        let expected = truncate_score(1.0);
        for (cat, stats) in u.stats().per_category {
            assert_eq!(
                stats.weight_sum, expected,
                "category {cat} feature weights sum to {} not 1.0 after truncate_score",
                stats.weight_sum
            );
        }
        let cat_sum = canonical_unit_sum(u.category_weights.values().copied().sum::<f64>());
        assert_eq!(cat_sum, expected, "global category weights must sum to 1.0");
        assert!(
            sums_to_one(u.category_weights.values().copied().sum()),
            "global category weights must be within 1e-9 of 1.0"
        );
        assert!(u.validate().is_empty(), "embedded matrix must validate");
    }

    #[test]
    fn partial_does_not_count_as_passing() {
        let u = universe();
        let s = u.stats();
        assert!(s.partial > 0, "fixture must include Partial rows");
        assert_eq!(
            s.passing, 6,
            "Phase 2 supported_count=6; Partial must not join Passing"
        );
        assert_eq!(s.partial, 11);
        for feat in u.features() {
            if feat.status == ParityStatus::Partial {
                assert!(
                    !feat.status.counts_as_passing(),
                    "{} Partial must not count as passing",
                    feat.id
                );
                assert_eq!(feat.status.score_contribution(), 0.5);
                assert_ne!(feat.status.score_contribution(), 1.0);
            }
        }
        let honest = u.effective_coverage();
        let rounded = u.coverage_if_partial_rounded_up();
        assert!(
            honest < rounded,
            "rounding Partial up must increase coverage (honest={honest} rounded={rounded})"
        );
        assert_ne!(honest, rounded);
    }

    #[test]
    fn excluded_still_debt_for_strict_100() {
        let u = universe();
        let s = u.stats();
        assert_eq!(s.excluded, 4);
        assert!(
            !u.strict_100_certifiable(),
            "excluded count is a hard fail for strict-100"
        );
        for feat in u.features() {
            if feat.status == ParityStatus::Excluded {
                assert!(feat.status.is_strict_100_debt());
                assert_eq!(feat.status.score_contribution(), 0.0);
                assert!(
                    feat.exclusion_rationale
                        .as_deref()
                        .map(|r| !r.trim().is_empty())
                        .unwrap_or(false),
                    "{} needs exclusion_rationale",
                    feat.id
                );
            }
        }
    }

    #[test]
    fn missing_strict_mode() {
        let u = universe();
        let feat = u
            .get(STRICT_MODE_FEATURE_ID)
            .unwrap_or_else(|| panic!("{STRICT_MODE_FEATURE_ID} must exist"));
        assert_eq!(
            feat.status,
            ParityStatus::Missing,
            "axis 11 strict-mode stays Missing; do not invent a flag or round to Partial/Passing"
        );
        assert!(!feat.status.counts_as_passing());
        assert_eq!(feat.status.score_contribution(), 0.0);
        assert_eq!(u.stats().missing, 1);
        assert!(!u.strict_100_certifiable());
    }

    #[test]
    fn forbidden_mcp_identity_excluded() {
        let u = universe();
        let feat = u
            .get(FORBIDDEN_MCP_FEATURE_ID)
            .unwrap_or_else(|| panic!("{FORBIDDEN_MCP_FEATURE_ID} must exist"));
        assert_eq!(
            feat.status,
            ParityStatus::Excluded,
            "MCP EngineIdentity::TokenZero is Excluded as gauntlet oracle"
        );
        let rationale = feat.exclusion_rationale.as_deref().unwrap_or("");
        assert!(
            rationale.contains(FORBIDDEN_MCP_ENGINE_IDENTITY),
            "exclusion rationale must name {FORBIDDEN_MCP_ENGINE_IDENTITY}"
        );
        assert!(is_forbidden_gauntlet_identity(
            FORBIDDEN_MCP_ENGINE_IDENTITY
        ));
        assert!(is_forbidden_gauntlet_identity(
            FORBIDDEN_MCP_REGISTRY_ENGINE
        ));
        let oracle = GauntletOracle::Spec.as_str();
        let caught = catch_unwind(|| assert_distinct(FORBIDDEN_MCP_ENGINE_IDENTITY, oracle));
        assert!(
            caught.is_err(),
            "identity guard must still reject MCP TokenZero"
        );
        assert_ne!(SUBJECT_IDENTITY, FORBIDDEN_MCP_ENGINE_IDENTITY);
    }

    #[test]
    fn features_sorted_by_id() {
        let u = FeatureUniverse::load_embedded().unwrap();
        let ids: Vec<&str> = u.features().map(|f| f.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "features() must be lexicographic by FeatureId");
        let first = ids.first().copied();
        let has_est = ids.iter().any(|id| *id == "F-TZ-001-EST");
        assert_eq!(first, Some("F-TZ-001"));
        assert!(has_est);
        let pos_001 = ids.iter().position(|id| *id == "F-TZ-001").unwrap();
        let pos_est = ids.iter().position(|id| *id == "F-TZ-001-EST").unwrap();
        assert!(pos_001 < pos_est);
    }

    #[test]
    fn load_rejects_weight_sum_not_one() {
        let bad = r#"
schema_version = "gauntlet.supported_surface_matrix.v1"
[categories.tokenizer-identity]
weight = 1.0
[[features]]
id = "F-TZ-001"
title = "broken"
category = "tokenizer-identity"
weight = 0.60
status = "supported"
[[features]]
id = "F-TZ-001-EST"
title = "also broken"
category = "tokenizer-identity"
weight = 0.50
status = "supported"
"#;
        let err = FeatureUniverse::load_from_str(bad, "malformed-test")
            .expect_err("0.60+0.50 must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("weight sum") && msg.contains("tokenizer-identity"),
            "expected weight-sum error, got: {msg}"
        );
    }

    #[test]
    fn n_a_does_not_round_to_passing() {
        let na = r#"
schema_version = "gauntlet.supported_surface_matrix.v1"
[categories.tokenizer-identity]
weight = 1.0
[[features]]
id = "F-TZ-001"
title = "na row"
category = "tokenizer-identity"
weight = 1.0
status = "n/a"
"#;
        let err = FeatureUniverse::load_from_str(na, "na-test")
            .expect_err("n/a must not load as Passing");
        assert!(err.to_string().contains("unknown status"), "{err}");
    }

    #[test]
    fn embedded_matches_workspace_when_path_set() {
        let embedded = FeatureUniverse::load_embedded().unwrap();
        let from_env = FeatureUniverse::load_from_env_or_embedded().unwrap();
        let a: Vec<_> = embedded
            .features()
            .map(|f| {
                (
                    f.id.as_str().to_string(),
                    f.status,
                    truncate_score(f.weight),
                )
            })
            .collect();
        let b: Vec<_> = from_env
            .features()
            .map(|f| {
                (
                    f.id.as_str().to_string(),
                    f.status,
                    truncate_score(f.weight),
                )
            })
            .collect();
        assert_eq!(a, b, "env/workspace load must match frozen embed");

        if let Ok(path) = std::env::var(SURFACE_MATRIX_PATH_ENV) {
            let bytes = std::fs::read(&path).expect("env matrix path readable");
            assert_eq!(
                sha256_hex(&bytes),
                FeatureUniverse::embedded_matrix_sha256(),
                "frozen embed must byte-match {path}"
            );
        }
    }
}
