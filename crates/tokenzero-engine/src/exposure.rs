//! vz89.10 session exposure ledger (mirror of hub
//! zerostack-racc-caching-output-vz89.10): track which evidence objects have
//! already crossed into the model for a session scope, so a second reference
//! sends the short ref instead of re-inlining bytes.
//!
//! Scope identity matches session_persist (TOKENZERO_SESSION_SCOPE or the
//! cache-path-derived scope), so the per-call engines CodeMode builds share
//! one ledger inside a server process. The ledger is deliberately
//! memory-resident: losing it (process restart) only causes a re-inline,
//! never wrong bytes, and re-expansion is always available and accounted as
//! recovery.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// One exposed evidence object: (session scope, object digest/ref, span).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExposureRow {
    /// Content-addressed ref of the exposed object (digest-bearing).
    pub object_ref: String,
    /// Span within the object; None = whole object.
    pub span: Option<String>,
    /// Session turn (codemode execution index) of first exposure.
    pub first_exposure_turn: u64,
    pub byte_len: u64,
    /// Expands after first exposure; each is accounted as recovery.
    pub reexpansions: u64,
}

#[derive(Debug, Default)]
pub struct SessionExposureLedger {
    rows: HashMap<(String, Option<String>), ExposureRow>,
    turn: u64,
}

impl SessionExposureLedger {
    /// Advance the session turn; called once per codemode execution.
    pub fn next_turn(&mut self) -> u64 {
        self.turn = self.turn.saturating_add(1);
        self.turn
    }

    /// The recorded exposure for (object_ref, span), if the session already
    /// holds those bytes.
    pub fn exposure(&self, object_ref: &str, span: Option<&str>) -> Option<&ExposureRow> {
        self.rows
            .get(&(object_ref.to_string(), span.map(str::to_string)))
    }

    /// Record first exposure of byte_len bytes. Returns true when newly
    /// recorded, false when the session already held the object.
    pub fn record(&mut self, object_ref: &str, span: Option<String>, byte_len: u64) -> bool {
        let key = (object_ref.to_string(), span.clone());
        if self.rows.contains_key(&key) {
            return false;
        }
        self.rows.insert(
            key,
            ExposureRow {
                object_ref: object_ref.to_string(),
                span,
                first_exposure_turn: self.turn,
                byte_len,
                reexpansions: 0,
            },
        );
        true
    }

    /// Record a re-expansion of a session-known object; returns the running
    /// re-expansion count, or None when the object was never exposed (an
    /// expand of foreign bytes is ordinary recovery, not a session replay).
    pub fn record_reexpansion(&mut self, object_ref: &str, span: Option<&str>) -> Option<u64> {
        let key = (object_ref.to_string(), span.map(str::to_string));
        let row = self.rows.get_mut(&key)?;
        row.reexpansions = row.reexpansions.saturating_add(1);
        Some(row.reexpansions)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

static REGISTRY: LazyLock<Mutex<HashMap<String, Arc<Mutex<SessionExposureLedger>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The process-wide ledger for a session scope. Engines built per call under
/// the same scope (CodeMode) share it; different scopes are isolated.
pub fn session_exposure_ledger(scope_id: &str) -> Arc<Mutex<SessionExposureLedger>> {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    Arc::clone(
        registry
            .entry(scope_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(SessionExposureLedger::default()))),
    )
}

#[cfg(test)]
#[path = "../../../tests/engine/inline/exposure__tests.rs"]
mod tests;
