use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

// Re-export items child test modules expect via `use super::*`.
pub use serde_json::{Value, json};
pub use std::collections::BTreeMap;
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::time::Duration;
pub use tokenzero_core::{Accounting, ContentType, Mode, ToolResponse, count_tokens, ref_record};
pub use tokenzero_engine::{
    exact_ref_token_count, find_rg_in_path, load_fetch_index, parse_rg_line, prune_dead_refs,
    record_fetch, session_persist,
};
pub use tokenzero_recovery::RecoveryStore;

static REF_INDEX_OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod edit;
mod expand;
mod fetch;
mod jsonrpc;
mod misc;
mod read;
mod search;
mod session;
mod shell;
mod support;
mod working_set;
mod zeroref_claims;
