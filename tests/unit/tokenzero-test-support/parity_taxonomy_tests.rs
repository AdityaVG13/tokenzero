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
schema_version = "gauntlet.supported_surface_matrix"
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
    fn load_rejects_non_positive_weight() {
        let negative = r#"
schema_version = "gauntlet.supported_surface_matrix"
[categories.tokenizer-identity]
weight = 1.0
[[features]]
id = "F-TZ-001"
title = "negative weight"
category = "tokenizer-identity"
weight = -0.5
status = "supported"
[[features]]
id = "F-TZ-001-EST"
title = "compensating"
category = "tokenizer-identity"
weight = 1.5
status = "supported"
"#;
        let err = FeatureUniverse::load_from_str(negative, "negative-weight-test")
            .expect_err("negative feature weight must be rejected even if the category sums to 1.0");
        assert!(
            err.to_string().contains("must be finite and > 0"),
            "expected non-positive weight error, got: {err}"
        );

        let zero_cat = r#"
schema_version = "gauntlet.supported_surface_matrix"
[categories.tokenizer-identity]
weight = 0.0
[[features]]
id = "F-TZ-001"
title = "zero category"
category = "tokenizer-identity"
weight = 1.0
status = "supported"
"#;
        let err = FeatureUniverse::load_from_str(zero_cat, "zero-category-weight-test")
            .expect_err("zero category weight must be rejected");
        assert!(
            err.to_string().contains("[categories.tokenizer-identity]"),
            "expected category weight error, got: {err}"
        );
    }

    #[test]
    fn n_a_does_not_round_to_passing() {
        let na = r#"
schema_version = "gauntlet.supported_surface_matrix"
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

