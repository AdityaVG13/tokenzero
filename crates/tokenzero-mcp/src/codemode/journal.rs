//! Crash-safe, plan-scoped CodeMode mutation journal.
//! Payload bytes never enter the journal: only digests, sizes and recovery refs.

use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PLAN_JOURNAL_VERSION: &str = "tokenzero.plan-journal.v1";
pub const PIN_SCHEMA_VERSION: &str = "zerostack.cas-gc.v1";
const MAX_DIAGNOSTIC_BYTES: usize = 2048;
const MAX_RESOLVED_JOURNALS: usize = 128;
const MAX_RESOLVED_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationClass {
    ReadOnly,
    ReversibleStoreMutation,
    IrreversibleExternal,
    Unknown,
}

/// Classification table derived from the canonical PR18 operation descriptor.
pub fn classify_method(method: &str) -> OperationClass {
    let bare = method
        .strip_prefix("zero.token.")
        .or_else(|| method.strip_prefix("zero."))
        .or_else(|| method.strip_prefix("tz_"))
        .unwrap_or(method)
        .replace('-', "_");
    match bare.as_str() {
        "read"
        | "find"
        | "grep"
        | "glob"
        | "tree"
        | "expand"
        | "expandMany"
        | "expand_many"
        | "dedupe"
        | "mem"
        | "recall"
        | "rewrite"
        | "discover"
        | "pick"
        | "filter_lines"
        | "count"
        | "first"
        | "verdict"
        | "raw"
        | "count_tokens"
        | "assert"
        | "codemode.search"
        | "codemode.describe"
        | "codemode.limits"
        | "codemode.journalDoctor"
        | "journalDoctor"
        | "journal_doctor"
        | "codemode.journalInspect"
        | "journalInspect"
        | "journal_inspect"
        | "codemode.journalResume"
        | "journalResume"
        | "journal_resume"
        | "search"
        | "describe"
        | "limits" => OperationClass::ReadOnly,
        "edit"
        | "codemode.journalRollback"
        | "journalRollback"
        | "journal_rollback"
        | "compact"
        | "compactMany"
        | "compact_many"
        | "compact_max"
        | "ingest"
        | "cache_pack"
        | "store_put"
        | "store_alias"
        | "migration_apply" => OperationClass::ReversibleStoreMutation,
        "shell" | "fetch" | "network" | "external" => OperationClass::IrreversibleExternal,
        _ => OperationClass::Unknown,
    }
}

pub fn classify_descriptor_tool(tool: &str) -> OperationClass {
    match tool {
        "tz_execute_code" | "tz_batch" => OperationClass::Unknown,
        "tz_report_tool_issue" => OperationClass::IrreversibleExternal,
        other => classify_method(other),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    Prepared,
    Applying,
    Committed,
    RollingBack,
    RolledBack,
    ManualIntervention,
}
impl JournalState {
    pub fn is_resolved(self) -> bool {
        matches!(self, Self::Committed | Self::RolledBack)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Prepared,
    Applying,
    Applied,
    RollingBack,
    RolledBack,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct OperationSpec {
    pub id: String,
    pub method: String,
    pub target: Option<PathBuf>,
    pub precondition_digest: Option<String>,
    pub precondition_exists: Option<bool>,
    pub postcondition_digest: Option<String>,
    pub undo_refs: Vec<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalOperation {
    pub index: usize,
    pub id: String,
    pub method: String,
    pub classification: OperationClass,
    pub idempotency_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precondition_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precondition_exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postcondition_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undo_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compensation_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub state: StepState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanJournal {
    pub version: String,
    pub plan_id: String,
    pub execution_id: String,
    pub project_id: String,
    pub store_id: String,
    pub atomic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downgrade_reason: Option<String>,
    pub state: JournalState,
    pub operations: Vec<JournalOperation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rollback_errors: Vec<String>,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalDoctorEntry {
    pub execution_id: String,
    pub state: JournalState,
    pub journal_path: String,
    pub next_step: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalDoctorReport {
    pub schema_version: String,
    pub unresolved: Vec<JournalDoctorEntry>,
    pub resolved_count: usize,
    pub corrupt: Vec<String>,
}

#[derive(Debug)]
pub enum BeginOutcome {
    Disabled,
    Downgraded { reason: String },
    AlreadyCommitted,
    Transaction(Box<JournalTransaction>),
}

#[derive(Debug)]
pub struct JournalTransaction {
    root: PathBuf,
    path: PathBuf,
    lock: File,
    journal: PlanJournal,
    fault_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedRollbackError {
    pub original: String,
    pub rollback: Vec<String>,
}
impl std::fmt::Display for CombinedRollbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "original error: {}", self.original)?;
        if !self.rollback.is_empty() {
            write!(f, "; rollback errors: {}", self.rollback.join(" | "))?;
        }
        Ok(())
    }
}
impl std::error::Error for CombinedRollbackError {}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn current_digest(path: &Path) -> io::Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(sha256_bytes(&bytes))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}
pub fn journal_root(cache_path: &Path) -> PathBuf {
    cache_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("plan-journals")
}

pub fn begin_plan(
    cache_path: &Path,
    project_root: &Path,
    plan_id: &str,
    execution_id: &str,
    specs: Vec<OperationSpec>,
    atomic_requested: bool,
) -> Result<BeginOutcome, String> {
    begin_plan_inner(
        cache_path,
        project_root,
        plan_id,
        execution_id,
        specs,
        atomic_requested,
        None,
    )
}

#[cfg(test)]
fn begin_plan_with_fault(
    cache_path: &Path,
    project_root: &Path,
    plan_id: &str,
    execution_id: &str,
    specs: Vec<OperationSpec>,
    atomic_requested: bool,
    fault_at: &str,
) -> Result<BeginOutcome, String> {
    begin_plan_inner(
        cache_path,
        project_root,
        plan_id,
        execution_id,
        specs,
        atomic_requested,
        Some(fault_at.to_string()),
    )
}

fn begin_plan_inner(
    cache_path: &Path,
    project_root: &Path,
    plan_id: &str,
    execution_id: &str,
    specs: Vec<OperationSpec>,
    atomic_requested: bool,
    fault_at: Option<String>,
) -> Result<BeginOutcome, String> {
    let classes: Vec<_> = specs
        .iter()
        .map(|spec| classify_method(&spec.method))
        .collect();
    let unsafe_methods: Vec<_> = specs
        .iter()
        .zip(&classes)
        .filter(|(_, class)| {
            matches!(
                class,
                OperationClass::IrreversibleExternal | OperationClass::Unknown
            )
        })
        .map(|(spec, _)| spec.method.clone())
        .collect();
    if !unsafe_methods.is_empty() {
        let reason = format!(
            "plan contains non-rollbackable or unknown operations: {}",
            unsafe_methods.join(", ")
        );
        if atomic_requested {
            return Err(format!("atomic plan rejected: {reason}"));
        }
        record_downgrade(
            cache_path,
            project_root,
            plan_id,
            execution_id,
            &specs,
            &classes,
            &reason,
        )?;
        return Ok(BeginOutcome::Downgraded { reason });
    }
    if !classes.contains(&OperationClass::ReversibleStoreMutation) {
        return Ok(BeginOutcome::Disabled);
    }

    let root = journal_root(cache_path);
    fs::create_dir_all(&root).map_err(|err| format!("create journal directory: {err}"))?;
    let lock_path = root.join("mutation.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|err| format!("open journal lock: {err}"))?;
    FileExt::try_lock(&lock).map_err(|err| {
        format!(
            "plan conflict: another mutation plan holds {}: {err}",
            lock_path.display()
        )
    })?;
    let path = root.join(format!("{}.json", safe_execution_id(execution_id)));
    if path.exists() {
        let journal = read_journal(&path)?;
        if journal.execution_id != execution_id || journal.plan_id != plan_id {
            return Err("journal identity collision".into());
        }
        if journal.state == JournalState::Committed {
            remove_pins(&root, &journal)?;
            return Ok(BeginOutcome::AlreadyCommitted);
        }
        if journal.state == JournalState::RolledBack {
            return Err("execution was already rolled back; use a new execution_id".into());
        }
        return Ok(BeginOutcome::Transaction(Box::new(JournalTransaction {
            root,
            path,
            lock,
            journal,
            fault_at: fault_at.clone(),
        })));
    }

    let created = now_ms();
    let operations = specs
        .into_iter()
        .enumerate()
        .map(|(index, spec)| {
            let pre = spec
                .precondition_digest
                .clone()
                .unwrap_or_else(|| "none".into());
            JournalOperation {
                index,
                id: spec.id,
                method: spec.method.clone(),
                classification: classes[index],
                idempotency_key: sha256_bytes(
                    format!("{execution_id}:{index}:{}:{pre}", spec.method).as_bytes(),
                ),
                target: spec.target.map(|path| path.to_string_lossy().into_owned()),
                precondition_digest: spec.precondition_digest,
                precondition_exists: spec.precondition_exists,
                postcondition_digest: spec.postcondition_digest,
                undo_refs: spec.undo_refs,
                compensation_refs: Vec::new(),
                size: spec.size,
                state: StepState::Prepared,
                diagnostic: None,
            }
        })
        .collect();
    let journal = PlanJournal {
        version: PLAN_JOURNAL_VERSION.into(),
        plan_id: plan_id.into(),
        execution_id: execution_id.into(),
        project_id: sha256_bytes(project_root.to_string_lossy().as_bytes()),
        store_id: sha256_bytes(cache_path.to_string_lossy().as_bytes()),
        atomic: true,
        downgrade_reason: None,
        state: JournalState::Prepared,
        operations,
        original_error: None,
        rollback_errors: Vec::new(),
        created_at_ms: created,
        updated_at_ms: created,
    };
    write_pins(&root, &journal)?;
    persist_journal(&path, &journal)?;
    let tx = JournalTransaction {
        root,
        path,
        lock,
        journal,
        fault_at,
    };
    tx.check_fault("prepare")?;
    Ok(BeginOutcome::Transaction(Box::new(tx)))
}

fn record_downgrade(
    cache_path: &Path,
    project_root: &Path,
    plan_id: &str,
    execution_id: &str,
    specs: &[OperationSpec],
    classes: &[OperationClass],
    reason: &str,
) -> Result<(), String> {
    let root = journal_root(cache_path);
    fs::create_dir_all(&root).map_err(|err| format!("create journal directory: {err}"))?;
    let lock_path = root.join("mutation.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|err| format!("open journal lock: {err}"))?;
    FileExt::try_lock(&lock).map_err(|err| {
        format!(
            "plan conflict: another mutation plan holds {}: {err}",
            lock_path.display()
        )
    })?;
    let path = root.join(format!("{}.json", safe_execution_id(execution_id)));
    if path.exists() {
        let existing = read_journal(&path)?;
        if existing.execution_id != execution_id || existing.plan_id != plan_id {
            return Err("journal identity collision".into());
        }
        if existing.atomic || existing.downgrade_reason.as_deref() != Some(reason) {
            return Err("journal replay classification changed".into());
        }
        return Ok(());
    }
    let created = now_ms();
    let operations = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| JournalOperation {
            index,
            id: spec.id.clone(),
            method: spec.method.clone(),
            classification: classes[index],
            idempotency_key: sha256_bytes(
                format!("{execution_id}:{index}:{}:non-atomic", spec.method).as_bytes(),
            ),
            target: spec
                .target
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            precondition_digest: spec.precondition_digest.clone(),
            precondition_exists: spec.precondition_exists,
            postcondition_digest: spec.postcondition_digest.clone(),
            undo_refs: Vec::new(),
            compensation_refs: Vec::new(),
            size: spec.size,
            state: StepState::Skipped,
            diagnostic: None,
        })
        .collect();
    let journal = PlanJournal {
        version: PLAN_JOURNAL_VERSION.into(),
        plan_id: plan_id.into(),
        execution_id: execution_id.into(),
        project_id: sha256_bytes(project_root.to_string_lossy().as_bytes()),
        store_id: sha256_bytes(cache_path.to_string_lossy().as_bytes()),
        atomic: false,
        downgrade_reason: Some(reason.to_string()),
        state: JournalState::Committed,
        operations,
        original_error: None,
        rollback_errors: Vec::new(),
        created_at_ms: created,
        updated_at_ms: created,
    };
    persist_journal(&path, &journal)?;
    enforce_retention(&root)?;
    let _ = FileExt::unlock(&lock);
    Ok(())
}

impl JournalTransaction {
    pub fn journal(&self) -> &PlanJournal {
        &self.journal
    }
    #[cfg(test)]
    pub fn set_fault(&mut self, boundary: impl Into<String>) {
        self.fault_at = Some(boundary.into());
    }
    fn check_fault(&self, boundary: &str) -> Result<(), String> {
        if self.fault_at.as_deref() == Some(boundary) {
            Err(format!("fault injected at {boundary}"))
        } else {
            Ok(())
        }
    }
    fn persist(&mut self) -> Result<(), String> {
        self.journal.updated_at_ms = now_ms();
        persist_journal(&self.path, &self.journal)
    }
    pub fn step_needs_apply(&self, index: usize) -> bool {
        self.journal.operations.get(index).is_some_and(|op| {
            !matches!(
                op.state,
                StepState::Applied | StepState::RolledBack | StepState::Skipped
            )
        })
    }
    pub fn verify_precondition(&self, index: usize) -> Result<(), String> {
        let op = self
            .journal
            .operations
            .get(index)
            .ok_or_else(|| "journal step missing".to_string())?;
        let Some(target) = op.target.as_deref() else {
            return Ok(());
        };
        let actual = current_digest(Path::new(target))
            .map_err(|err| format!("read precondition target: {err}"))?;
        match (&op.precondition_digest, op.precondition_exists, actual) {
            (Some(expected), _, Some(actual)) if expected == &actual => Ok(()),
            (None, Some(false), None) | (None, None, _) => Ok(()),
            (_, _, actual) => Err(format!(
                "CAS precondition failed for {target}: expected {:?}, actual {:?}",
                op.precondition_digest, actual
            )),
        }
    }
    pub fn mark_applying(&mut self, index: usize) -> Result<(), String> {
        self.verify_precondition(index)?;
        self.journal.state = JournalState::Applying;
        self.journal.operations[index].state = StepState::Applying;
        self.persist()
    }
    pub fn mark_applied(
        &mut self,
        index: usize,
        postcondition_digest: Option<String>,
        compensation_refs: Vec<String>,
    ) -> Result<(), String> {
        let op = self
            .journal
            .operations
            .get_mut(index)
            .ok_or_else(|| "journal step missing".to_string())?;
        if let (Some(expected), Some(actual)) = (&op.postcondition_digest, &postcondition_digest) {
            if expected != actual {
                return Err(format!(
                    "postcondition CAS failed for step {index}: expected {expected}, actual {actual}"
                ));
            }
        }
        if op.postcondition_digest.is_none() {
            op.postcondition_digest = postcondition_digest;
        }
        op.compensation_refs = compensation_refs;
        op.state = StepState::Applied;
        self.persist()?;
        self.check_fault(&format!("apply-step-{index}"))
    }
    pub fn commit(mut self) -> Result<PlanJournal, String> {
        self.journal.state = JournalState::Committed;
        self.persist()?;
        self.check_fault("commit")?;
        remove_pins(&self.root, &self.journal)?;
        enforce_retention(&self.root)?;
        let _ = FileExt::unlock(&self.lock);
        Ok(self.journal.clone())
    }
    pub fn rollback<F>(
        &mut self,
        original_error: impl Into<String>,
        mut undo: F,
    ) -> Result<(), CombinedRollbackError>
    where
        F: FnMut(&JournalOperation) -> Result<(), String>,
    {
        let original = bounded(&original_error.into());
        self.journal.original_error = Some(original.clone());
        self.journal.state = JournalState::RollingBack;
        if let Err(err) = self.persist() {
            return Err(CombinedRollbackError {
                original,
                rollback: vec![err],
            });
        }
        let mut errors = Vec::new();
        for index in (0..self.journal.operations.len()).rev() {
            if !matches!(
                self.journal.operations[index].state,
                StepState::Applied | StepState::Applying | StepState::RollingBack
            ) {
                continue;
            }
            self.journal.operations[index].state = StepState::RollingBack;
            if let Err(err) = self.persist() {
                errors.push(err);
                break;
            }
            let snapshot = self.journal.operations[index].clone();
            match undo(&snapshot) {
                Ok(()) => {
                    self.journal.operations[index].state = StepState::RolledBack;
                    if let Err(err) = self.persist() {
                        errors.push(err);
                        break;
                    }
                    if let Err(err) = self.check_fault(&format!("rollback-step-{index}")) {
                        // Test fault models process death after the durable boundary:
                        // leave rolling_back evidence instead of converting it to a handled failure.
                        return Err(CombinedRollbackError {
                            original,
                            rollback: vec![err],
                        });
                    }
                }
                Err(err) => {
                    let message = bounded(&format!("step {index}: {err}"));
                    self.journal.operations[index].diagnostic = Some(message.clone());
                    errors.push(message);
                    let _ = self.persist();
                }
            }
        }
        for operation in &mut self.journal.operations {
            if matches!(operation.state, StepState::Prepared | StepState::Applying) {
                operation.state = StepState::Skipped;
            }
        }
        self.journal.rollback_errors = errors.clone();
        self.journal.state = if errors.is_empty() {
            JournalState::RolledBack
        } else {
            JournalState::ManualIntervention
        };
        if let Err(err) = self.persist() {
            errors.push(err);
        }
        if errors.is_empty() {
            if let Err(err) = remove_pins(&self.root, &self.journal) {
                errors.push(err);
            }
            let _ = enforce_retention(&self.root);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CombinedRollbackError {
                original,
                rollback: errors,
            })
        }
    }
}
impl Drop for JournalTransaction {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
    }
}

pub fn doctor(cache_path: &Path) -> JournalDoctorReport {
    let root = journal_root(cache_path);
    let mut report = JournalDoctorReport {
        schema_version: PLAN_JOURNAL_VERSION.into(),
        unresolved: Vec::new(),
        resolved_count: 0,
        corrupt: Vec::new(),
    };
    let Ok(entries) = fs::read_dir(&root) else {
        return report;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        match read_journal(&path) {
            Ok(journal) if journal.state.is_resolved() => {
                report.resolved_count += 1;
                if let Err(err) = remove_pins(&root, &journal) {
                    report.corrupt.push(format!(
                        "{}: resolved pin cleanup failed: {}",
                        path.display(),
                        bounded(&err),
                    ));
                }
            }
            Ok(journal) => report.unresolved.push(JournalDoctorEntry {
                execution_id: journal.execution_id,
                state: journal.state,
                journal_path: path.to_string_lossy().into_owned(),
                next_step: next_step(journal.state).into(),
            }),
            Err(err) => report
                .corrupt
                .push(format!("{}: {}", path.display(), bounded(&err))),
        }
    }
    report
        .unresolved
        .sort_by(|a, b| a.execution_id.cmp(&b.execution_id));
    report.corrupt.sort();
    report
}
pub fn inspect(cache_path: &Path, execution_id: &str) -> Result<PlanJournal, String> {
    let path = journal_root(cache_path).join(format!("{}.json", safe_execution_id(execution_id)));
    if !path.exists() {
        return Err(format!("journal not found: {}", path.display()));
    }
    read_journal(&path)
}
pub fn open_unresolved(
    cache_path: &Path,
    execution_id: &str,
) -> Result<JournalTransaction, String> {
    let root = journal_root(cache_path);
    fs::create_dir_all(&root).map_err(|err| format!("create journal directory: {err}"))?;
    let lock_path = root.join("mutation.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|err| format!("open journal lock: {err}"))?;
    FileExt::try_lock(&lock).map_err(|err| {
        format!(
            "plan conflict: another mutation plan holds {}: {err}",
            lock_path.display()
        )
    })?;
    let path = root.join(format!("{}.json", safe_execution_id(execution_id)));
    let journal = read_journal(&path)?;
    if journal.state.is_resolved() {
        return Err(format!(
            "journal {execution_id} is already resolved as {:?}",
            journal.state
        ));
    }
    Ok(JournalTransaction {
        root,
        path,
        lock,
        journal,
        fault_at: None,
    })
}
pub fn doctor_json(cache_path: &Path) -> Value {
    serde_json::to_value(doctor(cache_path))
        .unwrap_or_else(|_| json!({"schema_version": PLAN_JOURNAL_VERSION, "unresolved": []}))
}
fn next_step(state: JournalState) -> &'static str {
    match state {
        JournalState::Prepared => "resume with the same plan and execution_id, or rollback",
        JournalState::Applying => "inspect completed steps, then resume or rollback",
        JournalState::RollingBack => "resume rollback; do not re-apply",
        JournalState::ManualIntervention => {
            "inspect original_error and rollback_errors; repair, then rollback"
        }
        JournalState::Committed | JournalState::RolledBack => "resolved",
    }
}
fn bounded(value: &str) -> String {
    if value.len() <= MAX_DIAGNOSTIC_BYTES {
        return value.into();
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &value[..end])
}
fn safe_execution_id(value: &str) -> String {
    let tail = value.strip_prefix("cm://exec/").unwrap_or(value);
    let safe: String = tail
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .take(128)
        .collect();
    if safe.is_empty() {
        sha256_bytes(value.as_bytes())
    } else {
        safe
    }
}
fn read_journal(path: &Path) -> Result<PlanJournal, String> {
    let bytes = fs::read(path).map_err(|err| format!("read journal {}: {err}", path.display()))?;
    let journal: PlanJournal =
        serde_json::from_slice(&bytes).map_err(|err| format!("parse journal: {err}"))?;
    if journal.version != PLAN_JOURNAL_VERSION {
        return Err(format!("unsupported journal version {}", journal.version));
    }
    Ok(journal)
}
fn persist_journal(path: &Path, journal: &PlanJournal) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(journal).map_err(|err| format!("serialize journal: {err}"))?;
    atomic_write(path, &bytes).map_err(|err| format!("persist journal: {err}"))
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|err| err.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
fn pin_hash(reference: &str) -> Option<String> {
    let value = reference.split('#').next()?.rsplit('/').next()?;
    let hash = if value.len() == 65 && value.starts_with('b') {
        &value[1..]
    } else {
        value
    };
    (hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
    .then(|| hash.into())
}
fn pin_path(root: &Path, execution_id: &str, index: usize, hash: &str) -> PathBuf {
    root.join("pins").join(format!(
        "plan-{}-{index}-{}.json",
        safe_execution_id(execution_id),
        &hash[..12]
    ))
}
fn write_pins(root: &Path, journal: &PlanJournal) -> Result<(), String> {
    for op in &journal.operations {
        for reference in &op.undo_refs {
            let Some(hash) = pin_hash(reference) else {
                continue;
            };
            let record = json!({"schema_version": PIN_SCHEMA_VERSION, "record_type": "pin", "engine": "tokenzero", "project_id": journal.project_id, "pin_id": format!("plan-{}-{}", &sha256_bytes(journal.execution_id.as_bytes())[..24], op.index), "created_at": rfc3339_now(), "blob_hash": hash});
            let bytes = serde_json::to_vec_pretty(&record).map_err(|err| err.to_string())?;
            atomic_write(
                &pin_path(root, &journal.execution_id, op.index, &hash),
                &bytes,
            )
            .map_err(|err| format!("persist undo pin: {err}"))?;
        }
    }
    Ok(())
}
fn remove_pins(root: &Path, journal: &PlanJournal) -> Result<(), String> {
    for op in &journal.operations {
        for reference in &op.undo_refs {
            let Some(hash) = pin_hash(reference) else {
                continue;
            };
            let path = pin_path(root, &journal.execution_id, op.index, &hash);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(format!("remove resolved pin {}: {err}", path.display())),
            }
        }
    }
    Ok(())
}
fn enforce_retention(root: &Path) -> Result<(), String> {
    let mut resolved = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.to_string()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        if let Ok(journal) = read_journal(&path) {
            if journal.state.is_resolved() {
                resolved.push((
                    journal.updated_at_ms,
                    entry.metadata().map(|m| m.len()).unwrap_or(0),
                    path,
                ));
            }
        }
    }
    resolved.sort_by_key(|item| item.0);
    let mut total: u64 = resolved.iter().map(|item| item.1).sum();
    let mut count = resolved.len();
    for (_, bytes, path) in resolved {
        if count <= MAX_RESOLVED_JOURNALS && total <= MAX_RESOLVED_BYTES {
            break;
        }
        fs::remove_file(&path)
            .map_err(|err| format!("prune resolved journal {}: {err}", path.display()))?;
        count -= 1;
        total = total.saturating_sub(bytes);
    }
    Ok(())
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
fn rfc3339_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let rem = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codemode::{CodeModeOptions, CodeModeStatus, execute_codemode_with_options};
    use std::process::Command;
    use tempfile::tempdir;

    fn fake_ref(seed: u8) -> String {
        format!("tz://blob/{}", format!("{seed:02x}").repeat(32))
    }

    fn spec(index: usize) -> OperationSpec {
        OperationSpec {
            id: format!("step{index}"),
            method: "zero.edit".to_string(),
            target: None,
            precondition_digest: None,
            precondition_exists: None,
            postcondition_digest: None,
            undo_refs: vec![fake_ref(index as u8 + 1)],
            size: Some(0),
        }
    }

    fn cache(dir: &Path) -> PathBuf {
        dir.join("tokenzero").join("recovery-cache.json")
    }

    #[test]
    fn journal_state_machine_fault_boundaries_restart_classification() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        let execution = "cm://exec/fault-boundaries";
        let error = begin_plan_with_fault(
            &cache,
            temp.path(),
            "plan-a",
            execution,
            vec![spec(0)],
            true,
            "prepare",
        )
        .unwrap_err();
        assert!(error.contains("fault injected at prepare"));
        assert_eq!(
            inspect(&cache, execution).unwrap().state,
            JournalState::Prepared
        );

        let mut tx = open_unresolved(&cache, execution).unwrap();
        tx.mark_applying(0).unwrap();
        drop(tx);
        assert_eq!(
            inspect(&cache, execution).unwrap().state,
            JournalState::Applying
        );

        let mut tx = open_unresolved(&cache, execution).unwrap();
        tx.set_fault("apply-step-0");
        assert!(
            tx.mark_applied(0, Some(sha256_bytes(b"post")), Vec::new())
                .is_err()
        );
        drop(tx);
        let partial = inspect(&cache, execution).unwrap();
        assert_eq!(partial.state, JournalState::Applying);
        assert_eq!(partial.operations[0].state, StepState::Applied);

        let mut tx = open_unresolved(&cache, execution).unwrap();
        tx.set_fault("commit");
        assert!(tx.commit().is_err());
        assert_eq!(
            inspect(&cache, execution).unwrap().state,
            JournalState::Committed
        );
        doctor(&cache);
        assert_eq!(
            fs::read_dir(journal_root(&cache).join("pins"))
                .unwrap()
                .count(),
            0
        );

        let rollback_execution = "cm://exec/fault-rollback";
        let mut tx = match begin_plan(
            &cache,
            temp.path(),
            "plan-b",
            rollback_execution,
            vec![spec(0)],
            true,
        )
        .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        tx.mark_applying(0).unwrap();
        tx.mark_applied(0, Some(sha256_bytes(b"post")), Vec::new())
            .unwrap();
        tx.set_fault("rollback-step-0");
        assert!(tx.rollback("later failure", |_| Ok(())).is_err());
        drop(tx);
        let crashed = inspect(&cache, rollback_execution).unwrap();
        assert_eq!(crashed.state, JournalState::RollingBack);
        assert_eq!(crashed.operations[0].state, StepState::RolledBack);
        let mut tx = open_unresolved(&cache, rollback_execution).unwrap();
        tx.rollback("resume rollback", |_| Ok(())).unwrap();
        assert_eq!(
            inspect(&cache, rollback_execution).unwrap().state,
            JournalState::RolledBack
        );
    }

    #[test]
    fn journal_idempotent_replay_has_single_effect() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        let execution = "cm://exec/idempotent";
        let mut tx = match begin_plan(&cache, temp.path(), "plan", execution, vec![spec(0)], true)
            .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        tx.mark_applying(0).unwrap();
        tx.mark_applied(0, Some(sha256_bytes(b"once")), Vec::new())
            .unwrap();
        tx.commit().unwrap();
        assert!(matches!(
            begin_plan(&cache, temp.path(), "plan", execution, vec![spec(0)], true).unwrap(),
            BeginOutcome::AlreadyCommitted
        ));
    }

    #[test]
    fn journal_rollback_is_reverse_order_and_preserves_both_errors() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        let execution = "cm://exec/reverse-rollback";
        let mut tx = match begin_plan(
            &cache,
            temp.path(),
            "plan",
            execution,
            vec![spec(0), spec(1)],
            true,
        )
        .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        for index in 0..2 {
            tx.mark_applying(index).unwrap();
            tx.mark_applied(
                index,
                Some(sha256_bytes(format!("post{index}").as_bytes())),
                Vec::new(),
            )
            .unwrap();
        }
        let mut order = Vec::new();
        let error = tx
            .rollback("apply failed", |operation| {
                order.push(operation.index);
                if operation.index == 0 {
                    Err("undo failed".to_string())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(order, vec![1, 0]);
        assert_eq!(error.original, "apply failed");
        assert_eq!(error.rollback.len(), 1);
        drop(tx);
        let journal = inspect(&cache, execution).unwrap();
        assert_eq!(journal.state, JournalState::ManualIntervention);
        assert_eq!(journal.original_error.as_deref(), Some("apply failed"));
        assert!(journal.rollback_errors[0].contains("undo failed"));
    }

    #[test]
    fn journal_mixed_shell_plan_rejects_atomic_or_records_downgrade() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        let specs = vec![
            spec(0),
            OperationSpec {
                id: "shell".to_string(),
                method: "zero.shell".to_string(),
                target: None,
                precondition_digest: None,
                precondition_exists: None,
                postcondition_digest: None,
                undo_refs: Vec::new(),
                size: None,
            },
        ];
        let error = begin_plan(
            &cache,
            temp.path(),
            "plan",
            "cm://exec/mixed-atomic",
            specs.clone(),
            true,
        )
        .unwrap_err();
        assert!(error.contains("atomic plan rejected"));
        match begin_plan(
            &cache,
            temp.path(),
            "plan",
            "cm://exec/mixed-downgrade",
            specs,
            false,
        )
        .unwrap()
        {
            BeginOutcome::Downgraded { reason } => assert!(reason.contains("zero.shell")),
            other => panic!("unexpected outcome: {other:?}"),
        }
        let downgrade = inspect(&cache, "cm://exec/mixed-downgrade").unwrap();
        assert!(!downgrade.atomic);
        assert_eq!(downgrade.state, JournalState::Committed);
        assert!(
            downgrade
                .downgrade_reason
                .as_deref()
                .unwrap()
                .contains("zero.shell")
        );
        assert!(
            downgrade
                .operations
                .iter()
                .all(|op| op.state == StepState::Skipped)
        );
    }

    #[test]
    fn journal_conflict_child() {
        let Some(root) = std::env::var_os("TOKENZERO_JOURNAL_CONFLICT_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let error = begin_plan(
            &cache(&root),
            &root,
            "plan",
            "cm://exec/child",
            vec![spec(0)],
            true,
        )
        .unwrap_err();
        assert!(error.contains("plan conflict"), "{error}");
    }

    #[test]
    fn journal_multi_process_conflict_fails_before_overwrite() {
        let temp = tempdir().unwrap();
        let _held = match begin_plan(
            &cache(temp.path()),
            temp.path(),
            "plan",
            "cm://exec/parent",
            vec![spec(0)],
            true,
        )
        .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("codemode::journal::tests::journal_conflict_child")
            .arg("--test-threads=1")
            .env("TOKENZERO_JOURNAL_CONFLICT_ROOT", temp.path())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn journal_doctor_lists_unresolved_and_pins_redacted_undo_refs() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        let execution = "cm://exec/doctor";
        let tx = match begin_plan(&cache, temp.path(), "plan", execution, vec![spec(0)], true)
            .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        let report = doctor(&cache);
        assert_eq!(report.unresolved.len(), 1);
        assert_eq!(report.unresolved[0].state, JournalState::Prepared);
        assert!(report.unresolved[0].next_step.contains("resume"));
        let pin_dir = journal_root(&cache).join("pins");
        let pin_paths = fs::read_dir(&pin_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(pin_paths.len(), 1);
        let pin: Value = serde_json::from_slice(&fs::read(&pin_paths[0]).unwrap()).unwrap();
        assert_eq!(pin["schema_version"], PIN_SCHEMA_VERSION);
        assert_eq!(pin["record_type"], "pin");
        assert_eq!(pin["engine"], "tokenzero");
        assert_eq!(pin["project_id"].as_str().unwrap().len(), 64);
        assert_eq!(pin["blob_hash"].as_str().unwrap().len(), 64);
        assert!(pin["pin_id"].as_str().unwrap().starts_with("plan-"));
        assert!(pin["created_at"].as_str().unwrap().ends_with('Z'));
        let encoded = serde_json::to_string(tx.journal()).unwrap();
        assert!(!encoded.contains("payload"));
        assert!(!encoded.contains("secret-body"));
        let mut tx = tx;
        tx.rollback("test cleanup", |_| Ok(())).unwrap();
        assert_eq!(fs::read_dir(pin_dir).unwrap().count(), 0);
    }

    fn options(root: &Path, cache_path: &Path) -> CodeModeOptions {
        CodeModeOptions {
            root: Some(root.to_path_buf()),
            cache_path: Some(cache_path.to_path_buf()),
            ref_first: false,
            ..CodeModeOptions::default()
        }
    }

    #[test]
    fn journal_json_plan_commit_replay_and_restart_smoke() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("value.txt");
        fs::write(&path, "before").unwrap();
        let cache = cache(temp.path());
        let plan = json!({
            "atomic": true,
            "plan_id": "json-commit",
            "execution_id": "cm://exec/json-commit",
            "steps": [{
                "id": "edit",
                "method": "zero.edit",
                "args": ["value.txt", [{"find": "before", "replace": "after-secret-body"}], {}]
            }]
        })
        .to_string();
        let first = execute_codemode_with_options(&plan, options(temp.path(), &cache));
        assert_eq!(first.status, CodeModeStatus::Completed, "{:?}", first.error);
        assert_eq!(fs::read_to_string(&path).unwrap(), "after-secret-body");
        let journal = inspect(&cache, "cm://exec/json-commit").unwrap();
        assert_eq!(journal.state, JournalState::Committed);
        assert!(
            !serde_json::to_string(&journal)
                .unwrap()
                .contains("after-secret-body")
        );
        let second = execute_codemode_with_options(&plan, options(temp.path(), &cache));
        assert_eq!(
            second.status,
            CodeModeStatus::Completed,
            "{:?}",
            second.error
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "after-secret-body");
        assert_eq!(second.value.unwrap()["idempotent_replay"], true);
    }

    #[test]
    fn journal_json_plan_rejects_invalid_later_edit_before_apply() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("a.txt"), "a-before").unwrap();
        fs::write(temp.path().join("b.txt"), "b-before").unwrap();
        let cache = cache(temp.path());
        let plan = json!({
            "atomic": true,
            "plan_id": "json-rollback",
            "execution_id": "cm://exec/json-rollback",
            "steps": [
                {"id": "a", "method": "zero.edit", "args": ["a.txt", [{"find": "a-before", "replace": "a-after"}], {}]},
                {"id": "b", "method": "zero.edit", "args": ["b.txt", [{"find": "missing", "replace": "b-after"}], {}]}
            ]
        }).to_string();
        let result = execute_codemode_with_options(&plan, options(temp.path(), &cache));
        assert_eq!(result.status, CodeModeStatus::Error);
        assert_eq!(
            fs::read_to_string(temp.path().join("a.txt")).unwrap(),
            "a-before"
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("b.txt")).unwrap(),
            "b-before"
        );
        assert!(inspect(&cache, "cm://exec/json-rollback").is_err());
    }

    #[test]
    fn journal_json_atomic_mixed_plan_rejected_before_shell() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("a.txt"), "before").unwrap();
        let cache = cache(temp.path());
        let plan = json!({
            "atomic": true,
            "plan_id": "json-mixed",
            "execution_id": "cm://exec/json-mixed",
            "steps": [
                {"method": "zero.edit", "args": ["a.txt", [{"find": "before", "replace": "after"}], {}]},
                {"method": "zero.shell", "args": ["touch shell-ran"]}
            ]
        }).to_string();
        let result = execute_codemode_with_options(&plan, options(temp.path(), &cache));
        assert_eq!(result.status, CodeModeStatus::Error);
        assert!(
            result
                .error
                .unwrap()
                .message
                .contains("atomic plan rejected")
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("a.txt")).unwrap(),
            "before"
        );
        assert!(!temp.path().join("shell-ran").exists());
    }

    #[test]
    fn journal_fault_injection_covers_each_apply_and_rollback_step() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        for fault_index in 0..3 {
            let execution = format!("cm://exec/apply-fault-{fault_index}");
            let mut tx = match begin_plan(
                &cache,
                temp.path(),
                "apply-plan",
                &execution,
                vec![spec(0), spec(1), spec(2)],
                true,
            )
            .unwrap()
            {
                BeginOutcome::Transaction(tx) => tx,
                other => panic!("unexpected outcome: {other:?}"),
            };
            for index in 0..=fault_index {
                tx.mark_applying(index).unwrap();
                if index == fault_index {
                    tx.set_fault(format!("apply-step-{index}"));
                }
                let result = tx.mark_applied(
                    index,
                    Some(sha256_bytes(format!("post-{index}").as_bytes())),
                    Vec::new(),
                );
                if index == fault_index {
                    assert!(result.is_err());
                } else {
                    result.unwrap();
                }
            }
            drop(tx);
            let journal = inspect(&cache, &execution).unwrap();
            assert_eq!(journal.state, JournalState::Applying);
            assert_eq!(journal.operations[fault_index].state, StepState::Applied);
        }
        for fault_index in 0..3 {
            let execution = format!("cm://exec/rollback-fault-{fault_index}");
            let mut tx = match begin_plan(
                &cache,
                temp.path(),
                "rollback-plan",
                &execution,
                vec![spec(0), spec(1), spec(2)],
                true,
            )
            .unwrap()
            {
                BeginOutcome::Transaction(tx) => tx,
                other => panic!("unexpected outcome: {other:?}"),
            };
            for index in 0..3 {
                tx.mark_applying(index).unwrap();
                tx.mark_applied(
                    index,
                    Some(sha256_bytes(format!("post-{index}").as_bytes())),
                    Vec::new(),
                )
                .unwrap();
            }
            tx.set_fault(format!("rollback-step-{fault_index}"));
            assert!(tx.rollback("later failure", |_| Ok(())).is_err());
            drop(tx);
            let journal = inspect(&cache, &execution).unwrap();
            assert_eq!(journal.state, JournalState::RollingBack);
            assert_eq!(journal.operations[fault_index].state, StepState::RolledBack);
        }
    }

    #[test]
    fn journal_cas_conflict_never_clobbers_newer_data() {
        let temp = tempdir().unwrap();
        let cache = cache(temp.path());
        let target = temp.path().join("cas.txt");
        fs::write(&target, "old").unwrap();
        let mut cas_spec = spec(0);
        cas_spec.target = Some(target.clone());
        cas_spec.precondition_digest = Some(sha256_bytes(b"old"));
        cas_spec.precondition_exists = Some(true);
        cas_spec.postcondition_digest = Some(sha256_bytes(b"post"));
        let mut tx = match begin_plan(
            &cache,
            temp.path(),
            "cas-before",
            "cm://exec/cas-before",
            vec![cas_spec.clone()],
            true,
        )
        .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        fs::write(&target, "newer").unwrap();
        assert!(
            tx.mark_applying(0)
                .unwrap_err()
                .contains("CAS precondition failed")
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "newer");
        drop(tx);

        fs::write(&target, "old").unwrap();
        let mut tx = match begin_plan(
            &cache,
            temp.path(),
            "cas-rollback",
            "cm://exec/cas-rollback",
            vec![cas_spec],
            true,
        )
        .unwrap()
        {
            BeginOutcome::Transaction(tx) => tx,
            other => panic!("unexpected outcome: {other:?}"),
        };
        tx.mark_applying(0).unwrap();
        fs::write(&target, "post").unwrap();
        tx.mark_applied(0, Some(sha256_bytes(b"post")), Vec::new())
            .unwrap();
        fs::write(&target, "newest").unwrap();
        let error = tx
            .rollback("later failure", |operation| {
                let actual =
                    current_digest(Path::new(operation.target.as_deref().unwrap())).unwrap();
                if actual != operation.postcondition_digest {
                    return Err("rollback CAS refused newer data".to_string());
                }
                Ok(())
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rollback CAS refused newer data")
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "newest");
    }

    #[test]
    fn journal_classification_table_covers_policy_boundaries() {
        assert_eq!(classify_method("zero.read"), OperationClass::ReadOnly);
        assert_eq!(
            classify_method("zero.edit"),
            OperationClass::ReversibleStoreMutation
        );
        assert_eq!(
            classify_method("zero.shell"),
            OperationClass::IrreversibleExternal
        );
        assert_eq!(
            classify_method("not.in.descriptor"),
            OperationClass::Unknown
        );
        assert_eq!(
            classify_descriptor_tool("tz_ingest"),
            OperationClass::ReversibleStoreMutation
        );
        let descriptor = crate::capability_descriptor::CapabilityDescriptor::for_surface(
            tokenzero_core::McpToolSurface::Classic,
        );
        for tool in descriptor.tools {
            assert_eq!(
                tool.operation_class,
                classify_descriptor_tool(&tool.name),
                "descriptor classification drift for {}",
                tool.name,
            );
        }
    }
}
