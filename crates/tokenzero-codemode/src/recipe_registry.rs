//! Versioned server-side CodeMode recipes and min-plus token envelopes.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const RECIPE_REGISTRY_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvelopeComponent {
    pub operation: String,
    pub worst_case_visible_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeDefinition {
    pub name: String,
    pub version: String,
    pub source: String,
    pub pulse_operation: String,
    pub pulse_calls: usize,
    pub measured_visible_tokens: usize,
    /// Alternatives are combined with min; components within an alternative
    /// are sequential and combined with plus.
    pub alternatives: Vec<Vec<EnvelopeComponent>>,
}

impl RecipeDefinition {
    pub fn envelope_tokens(&self) -> usize {
        self.alternatives
            .iter()
            .map(|alternative| {
                alternative.iter().fold(0usize, |sum, component| {
                    sum.saturating_add(component.worst_case_visible_tokens)
                })
            })
            .min()
            .unwrap_or(0)
    }
}

fn registry() -> &'static Vec<RecipeDefinition> {
    static REGISTRY: OnceLock<Vec<RecipeDefinition>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        // Resilient in production (RA-ASSERT): a corrupt or contract-violating
        // fixture must never panic the release path. Strict validation of the
        // committed fixture lives in the test suite.
        load_registry(include_str!("fixtures/codemode-recipes.json")).unwrap_or_default()
    })
}

fn load_registry(source: &str) -> Result<Vec<RecipeDefinition>, String> {
    let recipes: Vec<RecipeDefinition> = serde_json::from_str(source)
        .map_err(|err| format!("CodeMode recipe registry is not valid JSON: {err}"))?;
    Ok(recipes
        .into_iter()
        .filter(|recipe| {
            recipe.version == RECIPE_REGISTRY_VERSION
                && !recipe.alternatives.is_empty()
                && recipe.measured_visible_tokens <= recipe.envelope_tokens()
        })
        .collect())
}

pub fn get(name: &str) -> Option<RecipeDefinition> {
    registry()
        .iter()
        .find(|recipe| recipe.name == name)
        .cloned()
}

pub fn list() -> Vec<RecipeDefinition> {
    registry().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_top_ten_are_versioned_and_measured_under_envelopes() {
        let recipes = list();
        assert_eq!(recipes.len(), 10);
        for recipe in recipes {
            assert_eq!(recipe.version, RECIPE_REGISTRY_VERSION);
            assert!(
                recipe.pulse_calls > 0,
                "{} lacks pulse evidence",
                recipe.name
            );
            assert!(recipe.measured_visible_tokens <= recipe.envelope_tokens());
        }
    }

    #[test]
    fn corrupt_fixture_fails_without_panicking() {
        assert!(load_registry("{ not json").is_err());
        assert!(
            load_registry("[]")
                .expect("empty registry parses")
                .is_empty()
        );
    }

    #[test]
    fn invariant_violating_recipes_are_filtered_not_fatal() {
        let mut recipes: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("fixtures/codemode-recipes.json"))
                .expect("fixture parses");
        recipes[0]["version"] = serde_json::Value::String("bogus".into());
        let loaded = load_registry(&serde_json::to_string(&recipes).expect("serialize"))
            .expect("loads with one bad recipe");
        assert_eq!(loaded.len(), 9);
    }

    #[test]
    fn envelope_uses_min_plus_composition() {
        let recipe = RecipeDefinition {
            name: "test".into(),
            version: RECIPE_REGISTRY_VERSION.into(),
            source: String::new(),
            pulse_operation: "test".into(),
            pulse_calls: 1,
            measured_visible_tokens: 3,
            alternatives: vec![
                vec![
                    EnvelopeComponent {
                        operation: "a".into(),
                        worst_case_visible_tokens: 5,
                    },
                    EnvelopeComponent {
                        operation: "b".into(),
                        worst_case_visible_tokens: 7,
                    },
                ],
                vec![EnvelopeComponent {
                    operation: "c".into(),
                    worst_case_visible_tokens: 9,
                }],
            ],
        };
        assert_eq!(recipe.envelope_tokens(), 9);
    }
}
