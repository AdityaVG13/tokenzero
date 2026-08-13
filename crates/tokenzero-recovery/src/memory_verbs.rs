//! RACC actions-v2 memory verbs as the TokenZero working-set policy surface.
//!
//! The substrate stays deterministic (`WorkingSet` admit/evict/touch). These
//! verbs are the named interface a hub policy may drive later. This module is
//! the type stub: it names every verb and the existing primitive it maps onto.
//! It does not run a trained policy.

use serde::{Deserialize, Serialize};

/// Six RACC actions-v2 memory-management verbs (tokenzero-fmeo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVerb {
    Store,
    CommitSession,
    UpdateCapsule,
    ForgetVisible,
    PromoteAnchor,
    LinkRefs,
}

impl MemoryVerb {
    pub const ALL: [Self; 6] = [
        Self::Store,
        Self::CommitSession,
        Self::UpdateCapsule,
        Self::ForgetVisible,
        Self::PromoteAnchor,
        Self::LinkRefs,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::CommitSession => "commit_session",
            Self::UpdateCapsule => "update_capsule",
            Self::ForgetVisible => "forget_visible",
            Self::PromoteAnchor => "promote_anchor",
            Self::LinkRefs => "link_refs",
        }
    }

    /// Existing TokenZero working-set / recovery primitive this verb names.
    /// Policy does not live here.
    pub const fn substrate_target(self) -> &'static str {
        match self {
            Self::Store => "working_set.admit",
            Self::CommitSession => "recovery_store.persist",
            Self::UpdateCapsule => "working_set.rewrite_render",
            Self::ForgetVisible => "working_set.evict",
            Self::PromoteAnchor => "working_set.touch",
            Self::LinkRefs => "working_set.evicted_refs",
        }
    }
}

/// Policy-facing request. Fields are optional because stubs do not execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryVerbRequest {
    pub verb: MemoryVerb,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Describe-only effect: names the substrate target without mutating it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryVerbEffect {
    pub verb: MemoryVerb,
    pub substrate: String,
    /// Stubs never apply; a later bead wires execution.
    pub applied: bool,
}

/// Map a request onto the deterministic substrate. Does not mutate state.
pub fn describe_memory_verb(request: &MemoryVerbRequest) -> MemoryVerbEffect {
    MemoryVerbEffect {
        verb: request.verb,
        substrate: request.verb.substrate_target().to_string(),
        applied: false,
    }
}

#[cfg(test)]
#[path = "../../../tests/recovery/inline/memory_verbs__tests.rs"]
mod tests;
