//! Cache-prefix stability invariants shared by renderers and golden tests.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;
use tokenzero_core::{count_tokens, sha256_hex};

pub const MAX_CACHE_BLOCKS_PER_TURN: usize = 15;
/// The public-runtime estimator and provider tokenizers differ near cache
/// floors. Requiring 5 estimator tokens for every 4 provider tokens is a
/// conservative 25% boundary tolerance: warn early rather than claim caching.
pub const ESTIMATOR_FLOOR_SAFETY_NUMERATOR: usize = 5;
pub const ESTIMATOR_FLOOR_SAFETY_DENOMINATOR: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheModelTier {
    Opus,
    FableOrSonnet46,
    OlderSonnet,
}

impl CacheModelTier {
    pub const fn min_cacheable_tokens(self) -> usize {
        match self {
            Self::Opus => 4_096,
            Self::FableOrSonnet46 => 2_048,
            Self::OlderSonnet => 1_024,
        }
    }

    pub const fn min_cacheable_estimator_tokens(self) -> usize {
        self.min_cacheable_tokens()
            .saturating_mul(ESTIMATOR_FLOOR_SAFETY_NUMERATOR)
            .div_ceil(ESTIMATOR_FLOOR_SAFETY_DENOMINATOR)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderObservation<'a> {
    pub content: &'a str,
    pub rendered: &'a str,
    pub level: &'a str,
    /// A real tokenizer identifier, or an explicitly labelled estimator id.
    pub tokenizer_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheablePrefix {
    pub bytes: String,
    pub cache_breakpoint: bool,
    /// Number of provider cache-control blocks attributed to each turn.
    pub blocks_per_turn: BTreeMap<u64, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixStabilityAlert {
    BelowCacheableFloor {
        observed_tokens: usize,
        required_tokens: usize,
        model_tier: CacheModelTier,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrefixStabilityViolation {
    #[error("cacheable prefix changed between provider cache breakpoints")]
    NonMonotonePrefix,
    #[error("render changed for identical content, level, and tokenizer ({tokenizer_id})")]
    NonDeterministicRender { tokenizer_id: String },
    #[error("turn {turn} has {blocks} cache blocks; maximum is {maximum}")]
    BlockBudgetExceeded {
        turn: u64,
        blocks: usize,
        maximum: usize,
    },
}

#[derive(Debug, Default)]
pub struct PrefixStabilityGuard {
    last_prefix: Option<String>,
    renders: HashMap<(String, String, String), String>,
    prefix_observations: usize,
}

impl PrefixStabilityGuard {
    pub fn observe_prefix(
        &mut self,
        prefix: &CacheablePrefix,
        model_tier: CacheModelTier,
    ) -> Result<Option<PrefixStabilityAlert>, PrefixStabilityViolation> {
        if !prefix.cache_breakpoint
            && self
                .last_prefix
                .as_ref()
                .is_some_and(|previous| !prefix.bytes.starts_with(previous))
        {
            return Err(PrefixStabilityViolation::NonMonotonePrefix);
        }
        for (&turn, &blocks) in &prefix.blocks_per_turn {
            if blocks > MAX_CACHE_BLOCKS_PER_TURN {
                return Err(PrefixStabilityViolation::BlockBudgetExceeded {
                    turn,
                    blocks,
                    maximum: MAX_CACHE_BLOCKS_PER_TURN,
                });
            }
        }
        self.last_prefix = Some(prefix.bytes.clone());
        self.prefix_observations = self.prefix_observations.saturating_add(1);
        let observed_tokens = count_tokens(&prefix.bytes);
        let required_tokens = model_tier.min_cacheable_estimator_tokens();
        Ok((observed_tokens < required_tokens).then_some(
            PrefixStabilityAlert::BelowCacheableFloor {
                observed_tokens,
                required_tokens,
                model_tier,
            },
        ))
    }

    pub fn observe_render(
        &mut self,
        observation: RenderObservation<'_>,
    ) -> Result<String, PrefixStabilityViolation> {
        let key = (
            sha256_hex(observation.content),
            observation.level.to_owned(),
            observation.tokenizer_id.to_owned(),
        );
        if self
            .renders
            .get(&key)
            .is_some_and(|prior| prior.as_bytes() != observation.rendered.as_bytes())
        {
            return Err(PrefixStabilityViolation::NonDeterministicRender {
                tokenizer_id: observation.tokenizer_id.to_owned(),
            });
        }
        self.renders
            .entry(key)
            .or_insert_with(|| observation.rendered.to_owned());
        Ok(sha256_hex(observation.rendered))
    }

    pub fn observation_counts(&self) -> (usize, usize) {
        (self.prefix_observations, self.renders.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(bytes: String, cache_breakpoint: bool, blocks: usize) -> CacheablePrefix {
        CacheablePrefix {
            bytes,
            cache_breakpoint,
            blocks_per_turn: BTreeMap::from([(7, blocks)]),
        }
    }

    #[test]
    fn golden_prefix_is_monotone_between_breakpoints_and_injected_violation_fails() {
        let mut guard = PrefixStabilityGuard::default();
        let base = "cache ".repeat(1_100);
        guard
            .observe_prefix(&prefix(base.clone(), true, 1), CacheModelTier::OlderSonnet)
            .unwrap();
        guard
            .observe_prefix(
                &prefix(format!("{base}tail"), false, 1),
                CacheModelTier::OlderSonnet,
            )
            .unwrap();
        let violation = guard.observe_prefix(
            &prefix(format!("mutated-{base}"), false, 1),
            CacheModelTier::OlderSonnet,
        );
        assert_eq!(violation, Err(PrefixStabilityViolation::NonMonotonePrefix));
    }

    #[test]
    fn golden_breakpoint_may_reset_to_a_non_extending_prefix() {
        let mut guard = PrefixStabilityGuard::default();
        guard
            .observe_prefix(
                &prefix("old provider prefix ".repeat(1_300), false, 1),
                CacheModelTier::OlderSonnet,
            )
            .unwrap();
        guard
            .observe_prefix(
                &prefix("replacement prefix ".repeat(1_300), true, 1),
                CacheModelTier::OlderSonnet,
            )
            .expect("an explicit provider breakpoint permits a prefix reset");
    }

    #[test]
    fn golden_render_is_byte_identical_for_content_level_and_tokenizer() {
        let mut guard = PrefixStabilityGuard::default();
        let observation = RenderObservation {
            content: "same evidence",
            rendered: "stable capsule",
            level: "capsule",
            tokenizer_id: "claude-opus-4-8",
        };
        let first = guard.observe_render(observation.clone()).unwrap();
        let second = guard.observe_render(observation).unwrap();
        assert_eq!(first, second);
        let injected = guard.observe_render(RenderObservation {
            content: "same evidence",
            rendered: "changed capsule",
            level: "capsule",
            tokenizer_id: "claude-opus-4-8",
        });
        assert!(matches!(
            injected,
            Err(PrefixStabilityViolation::NonDeterministicRender { .. })
        ));
    }

    #[test]
    fn golden_cache_block_budget_is_fifteen_per_turn() {
        let mut guard = PrefixStabilityGuard::default();
        guard
            .observe_prefix(
                &prefix("x ".repeat(1_100), true, 15),
                CacheModelTier::OlderSonnet,
            )
            .unwrap();
        assert_eq!(
            guard.observe_prefix(
                &prefix("x ".repeat(1_100), true, 16),
                CacheModelTier::OlderSonnet
            ),
            Err(PrefixStabilityViolation::BlockBudgetExceeded {
                turn: 7,
                blocks: 16,
                maximum: 15
            })
        );
    }

    #[test]
    fn golden_model_floors_alert_before_caching_silently_stops() {
        for (tier, provider_floor, required) in [
            (CacheModelTier::Opus, 4_096, 5_120),
            (CacheModelTier::FableOrSonnet46, 2_048, 2_560),
            (CacheModelTier::OlderSonnet, 1_024, 1_280),
        ] {
            assert_eq!(tier.min_cacheable_tokens(), provider_floor);
            assert_eq!(tier.min_cacheable_estimator_tokens(), required);
            let mut guard = PrefixStabilityGuard::default();
            let alert = guard
                .observe_prefix(&prefix("too short".into(), true, 1), tier)
                .unwrap();
            assert_eq!(
                alert,
                Some(PrefixStabilityAlert::BelowCacheableFloor {
                    observed_tokens: count_tokens("too short"),
                    required_tokens: required,
                    model_tier: tier,
                })
            );
        }
    }
}
