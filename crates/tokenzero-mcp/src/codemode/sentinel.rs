//! One-token sentinel interception for CodeMode takeover channels.
//!
//! The model-visible channel contains one certified opcode and one ACK/2 atom.
//! Recipe payloads and details stay server-side or ref-backed.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use tokenzero_core::{AckClass, ProtocolTokenizer, is_verified_one_token_atom, render_ack};

use super::recipe_registry::{self, RECIPE_REGISTRY_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentinelMode {
    Text,
    Takeover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideEffectClass {
    Observe,
    Derive,
    Stage,
    Mutate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentinelOpcode {
    pub atom: &'static str,
    pub recipe: &'static str,
    pub recipe_version: &'static str,
    pub side_effect: SideEffectClass,
}

/// Deterministic declaration embedded in the provider-cacheable prefix.
pub const SENTINEL_V1_PREFIX: &str = "1TP-SENTINEL/1 5=tree_shallow@1.0.0:observe 6=recall_top@1.0.0:derive 7=ingest_text@1.0.0:mutate";

const SENTINEL_V1: [SentinelOpcode; 3] = [
    SentinelOpcode {
        atom: "5",
        recipe: "tree_shallow",
        recipe_version: RECIPE_REGISTRY_VERSION,
        side_effect: SideEffectClass::Observe,
    },
    SentinelOpcode {
        atom: "6",
        recipe: "recall_top",
        recipe_version: RECIPE_REGISTRY_VERSION,
        side_effect: SideEffectClass::Derive,
    },
    SentinelOpcode {
        atom: "7",
        recipe: "ingest_text",
        recipe_version: RECIPE_REGISTRY_VERSION,
        side_effect: SideEffectClass::Mutate,
    },
];

pub fn sentinel_v1_table() -> &'static [SentinelOpcode] {
    &SENTINEL_V1
}

#[derive(Debug)]
pub struct ArmedReservation {
    session: String,
    atom: String,
    recipe: String,
    recipe_version: String,
    expires_at_ms: u64,
    consumed: AtomicBool,
}

impl ArmedReservation {
    pub fn arm(session: impl Into<String>, opcode: SentinelOpcode, expires_at_ms: u64) -> Self {
        Self {
            session: session.into(),
            atom: opcode.atom.into(),
            recipe: opcode.recipe.into(),
            recipe_version: opcode.recipe_version.into(),
            expires_at_ms,
            consumed: AtomicBool::new(false),
        }
    }

    fn consume(&self, session: &str, opcode: SentinelOpcode, now_ms: u64) -> bool {
        if self.session != session
            || self.atom != opcode.atom
            || self.recipe != opcode.recipe
            || self.recipe_version != opcode.recipe_version
            || now_ms > self.expires_at_ms
        {
            return false;
        }
        self.consumed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeExecution {
    pub success: bool,
    pub detail_ref: Option<String>,
    pub error_kind: Option<String>,
    pub retryable: bool,
}

impl RecipeExecution {
    pub fn success(detail_ref: Option<String>) -> Self {
        Self {
            success: true,
            detail_ref,
            error_kind: None,
            retryable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentinelOutcome {
    /// Text mode never interprets a glyph as control input.
    NotIntercepted,
    Ack {
        atom: String,
        detail_ref: Option<String>,
        recipe: Option<String>,
        /// Input opcode plus output ACK. Recipe details are not model-visible.
        visible_tokens: usize,
    },
}

fn ack(class: AckClass, recipe: Option<&str>, detail_ref: Option<String>) -> SentinelOutcome {
    SentinelOutcome::Ack {
        atom: render_ack(class, false).into(),
        detail_ref,
        recipe: recipe.map(str::to_owned),
        visible_tokens: 2,
    }
}

/// Intercept one certified glyph in takeover mode and execute its mapped recipe.
///
/// Mutating opcodes fail closed unless a matching, unexpired, single-use
/// reservation is consumed before the executor is called.
pub fn intercept_with<F>(
    input: &str,
    mode: SentinelMode,
    tokenizer: ProtocolTokenizer,
    session: &str,
    now_ms: u64,
    reservation: Option<&ArmedReservation>,
    mut execute: F,
) -> SentinelOutcome
where
    F: FnMut(&str) -> RecipeExecution,
{
    if mode != SentinelMode::Takeover {
        return SentinelOutcome::NotIntercepted;
    }

    let Some(opcode) = SENTINEL_V1
        .iter()
        .copied()
        .find(|entry| entry.atom == input)
    else {
        return ack(AckClass::Validation, None, None);
    };
    if !is_verified_one_token_atom(tokenizer, opcode.atom) {
        return ack(AckClass::Validation, None, None);
    }
    let Some(recipe) = recipe_registry::get(opcode.recipe) else {
        return ack(AckClass::Internal, Some(opcode.recipe), None);
    };
    if recipe.version != opcode.recipe_version {
        return ack(AckClass::Policy, Some(opcode.recipe), None);
    }

    if opcode.side_effect == SideEffectClass::Mutate
        && !reservation.is_some_and(|armed| armed.consume(session, opcode, now_ms))
    {
        return ack(AckClass::Policy, Some(opcode.recipe), None);
    }

    let execution = execute(opcode.recipe);
    if execution.success {
        ack(AckClass::Success, Some(opcode.recipe), execution.detail_ref)
    } else {
        let class = AckClass::from_error_kind(
            execution.error_kind.as_deref().unwrap_or("internal"),
            execution.retryable,
        );
        ack(class, Some(opcode.recipe), execution.detail_ref)
    }
}

/// Production CodeMode sentinel entrypoint. Recipe arguments are server-side;
/// only the opcode and ACK are counted as model-visible protocol tokens.
#[cfg(feature = "surface-codemode")]
pub fn execute_sentinel(
    input: &str,
    mode: SentinelMode,
    tokenizer: ProtocolTokenizer,
    session: &str,
    now_ms: u64,
    reservation: Option<&ArmedReservation>,
    args: &Value,
) -> SentinelOutcome {
    intercept_with(
        input,
        mode,
        tokenizer,
        session,
        now_ms,
        reservation,
        |recipe_name| {
            let Some(recipe) = recipe_registry::get(recipe_name) else {
                return RecipeExecution {
                    success: false,
                    detail_ref: None,
                    error_kind: Some("internal".into()),
                    retryable: false,
                };
            };
            let args = serde_json::to_string(args).unwrap_or_else(|_| "{}".into());
            let plan = format!("const args = {args}; {}", recipe.source);
            let result = super::exec::execute_codemode_with_options(
                &plan,
                super::CodeModeOptions::default(),
            );
            RecipeExecution {
                success: result.status == super::CodeModeStatus::Completed,
                detail_ref: result.detail_ref.or_else(|| result.refs.into_iter().next()),
                error_kind: result.error.as_ref().map(|error| error.kind.clone()),
                retryable: result.error.as_ref().is_some_and(|error| error.retryable),
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn metered_observe_call_is_two_visible_tokens() {
        let calls = AtomicUsize::new(0);
        let outcome = intercept_with(
            "5",
            SentinelMode::Takeover,
            ProtocolTokenizer::Anthropic,
            "s1",
            10,
            None,
            |recipe| {
                calls.fetch_add(1, Ordering::Relaxed);
                assert_eq!(recipe, "tree_shallow");
                RecipeExecution::success(Some("tz://detail".into()))
            },
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            outcome,
            SentinelOutcome::Ack {
                atom: "0".into(),
                detail_ref: Some("tz://detail".into()),
                recipe: Some("tree_shallow".into()),
                visible_tokens: 2,
            }
        );
        assert!(is_verified_one_token_atom(
            ProtocolTokenizer::Anthropic,
            "5"
        ));
        assert!(2 < 10, "sentinel call must remain single-digit");
    }

    #[test]
    fn stray_glyph_and_unarmed_mutation_never_execute() {
        let calls = AtomicUsize::new(0);

        assert_eq!(
            intercept_with(
                "7",
                SentinelMode::Text,
                ProtocolTokenizer::O200k,
                "s1",
                10,
                None,
                |_: &str| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    RecipeExecution::success(None)
                }
            ),
            SentinelOutcome::NotIntercepted,
        );
        let denied = intercept_with(
            "7",
            SentinelMode::Takeover,
            ProtocolTokenizer::O200k,
            "s1",
            10,
            None,
            |_: &str| {
                calls.fetch_add(1, Ordering::Relaxed);
                RecipeExecution::success(None)
            },
        );
        assert!(matches!(denied, SentinelOutcome::Ack { ref atom, .. } if atom == "2"));
        let unknown = intercept_with(
            "x",
            SentinelMode::Takeover,
            ProtocolTokenizer::O200k,
            "s1",
            10,
            None,
            |_: &str| {
                calls.fetch_add(1, Ordering::Relaxed);
                RecipeExecution::success(None)
            },
        );
        assert!(matches!(unknown, SentinelOutcome::Ack { ref atom, .. } if atom == "1"));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn mutating_reservation_is_bound_and_single_use() {
        let opcode = sentinel_v1_table()
            .iter()
            .copied()
            .find(|item| item.atom == "7")
            .unwrap();
        let reservation = ArmedReservation::arm("s1", opcode, 20);
        let calls = AtomicUsize::new(0);
        let first = intercept_with(
            "7",
            SentinelMode::Takeover,
            ProtocolTokenizer::Gemini,
            "s1",
            10,
            Some(&reservation),
            |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                RecipeExecution::success(None)
            },
        );
        assert!(matches!(first, SentinelOutcome::Ack { ref atom, .. } if atom == "0"));
        let replay = intercept_with(
            "7",
            SentinelMode::Takeover,
            ProtocolTokenizer::Gemini,
            "s1",
            11,
            Some(&reservation),
            |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                RecipeExecution::success(None)
            },
        );
        assert!(matches!(replay, SentinelOutcome::Ack { ref atom, .. } if atom == "2"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[cfg(feature = "surface-codemode")]
    #[test]
    fn production_takeover_executes_mapped_recipe() {
        let outcome = execute_sentinel(
            "5",
            SentinelMode::Takeover,
            ProtocolTokenizer::Kimi,
            "s1",
            10,
            None,
            &serde_json::json!({"path": "."}),
        );
        assert!(matches!(
            outcome,
            SentinelOutcome::Ack { ref atom, visible_tokens: 2, ref recipe, .. }
                if atom == "0" && recipe.as_deref() == Some("tree_shallow")
        ));
    }

    #[test]
    fn every_sentinel_opcode_is_portable_and_recipe_versioned() {
        assert_eq!(
            SENTINEL_V1_PREFIX,
            "1TP-SENTINEL/1 5=tree_shallow@1.0.0:observe 6=recall_top@1.0.0:derive 7=ingest_text@1.0.0:mutate"
        );
        for opcode in sentinel_v1_table() {
            for tokenizer in ProtocolTokenizer::ALL {
                assert!(is_verified_one_token_atom(tokenizer, opcode.atom));
            }
            let recipe = recipe_registry::get(opcode.recipe).expect("sentinel recipe exists");
            assert_eq!(recipe.version, opcode.recipe_version);
        }
    }
}
