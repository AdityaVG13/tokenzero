//! Crash-safe, plan-scoped CodeMode mutation journal.
//! Payload bytes never enter the journal: only digests, sizes and recovery refs.

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};

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
    let classes = [
        (
            "read,find,grep,glob,tree,expand,expandMany,expand_many,dedupe,mem,recall,rewrite,discover,pick,filter_lines,count,first,verdict,raw,count_tokens,assert,codemode.search,codemode.describe,codemode.limits,codemode.journalDoctor,journalDoctor,journal_doctor,codemode.journalInspect,journalInspect,journal_inspect,codemode.journalResume,journalResume,journal_resume,search,describe,limits",
            OperationClass::ReadOnly,
        ),
        (
            "edit,codemode.journalRollback,journalRollback,journal_rollback,compact,compactMany,compact_many,compact_max,ingest,cache_pack,store_put,store_alias,migration_apply",
            OperationClass::ReversibleStoreMutation,
        ),
        (
            "shell,fetch,network,external",
            OperationClass::IrreversibleExternal,
        ),
    ];
    classes
        .into_iter()
        .find(|(methods, _)| methods.split(',').any(|candidate| candidate == bare))
        .map_or(OperationClass::Unknown, |(_, class)| class)
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

mod flows {
    use super::*;
    use fs4::FileExt;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn sha256_bytes(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
    pub fn current_digest(path: &Path) -> io::Result<Option<String>> {
        match fs::read(path) {
            Ok(b) => Ok(Some(sha256_bytes(&b))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
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

    fn open_journal_lock(
        cache_path: &Path,
        execution_id: &str,
    ) -> Result<(PathBuf, PathBuf, File), String> {
        let root = journal_root(cache_path);
        fs::create_dir_all(&root).map_err(|e| format!("create journal directory: {e}"))?;
        let lock_path = root.join("mutation.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| format!("open journal lock: {e}"))?;
        FileExt::try_lock(&lock).map_err(|e| {
            format!(
                "plan conflict: another mutation plan holds {}: {e}",
                lock_path.display()
            )
        })?;
        Ok((
            root.clone(),
            root.join(format!("{}.json", safe_execution_id(execution_id))),
            lock,
        ))
    }

    fn make_tx(
        root: PathBuf,
        path: PathBuf,
        lock: File,
        journal: PlanJournal,
        fault_at: Option<String>,
    ) -> JournalTransaction {
        JournalTransaction {
            root,
            path,
            lock,
            journal,
            fault_at,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_plan_journal(
        project_root: &Path,
        store_path: &Path,
        plan_id: &str,
        execution_id: &str,
        specs: &[OperationSpec],
        classes: &[OperationClass],
        atomic: bool,
        downgrade_reason: Option<String>,
        state: JournalState,
    ) -> PlanJournal {
        let operations = specs
            .iter()
            .zip(classes)
            .enumerate()
            .map(|(index, (spec, &classification))| {
                let idempotency_suffix = if atomic {
                    spec.precondition_digest.as_deref().unwrap_or("none")
                } else {
                    "non-atomic"
                };
                JournalOperation {
                    index,
                    id: spec.id.clone(),
                    method: spec.method.clone(),
                    classification,
                    idempotency_key: sha256_bytes(
                        format!(
                            "{execution_id}:{index}:{}:{idempotency_suffix}",
                            spec.method
                        )
                        .as_bytes(),
                    ),
                    target: spec
                        .target
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    precondition_digest: spec.precondition_digest.clone(),
                    precondition_exists: spec.precondition_exists,
                    postcondition_digest: spec.postcondition_digest.clone(),
                    undo_refs: if atomic {
                        spec.undo_refs.clone()
                    } else {
                        Vec::new()
                    },
                    compensation_refs: Vec::new(),
                    size: spec.size,
                    state: if atomic {
                        StepState::Prepared
                    } else {
                        StepState::Skipped
                    },
                    diagnostic: None,
                }
            })
            .collect();
        let created = now_ms();
        PlanJournal {
            version: PLAN_JOURNAL_VERSION.into(),
            plan_id: plan_id.into(),
            execution_id: execution_id.into(),
            project_id: sha256_bytes(project_root.to_string_lossy().as_bytes()),
            store_id: sha256_bytes(store_path.to_string_lossy().as_bytes()),
            atomic,
            downgrade_reason,
            state,
            operations,
            original_error: None,
            rollback_errors: Vec::new(),
            created_at_ms: created,
            updated_at_ms: created,
        }
    }

    fn check_identity(
        existing: &PlanJournal,
        plan_id: &str,
        execution_id: &str,
    ) -> Result<(), String> {
        if existing.execution_id != execution_id || existing.plan_id != plan_id {
            Err("journal identity collision".into())
        } else {
            Ok(())
        }
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
        let classes: Vec<_> = specs.iter().map(|s| classify_method(&s.method)).collect();
        let unsafe_methods: Vec<_> = specs
            .iter()
            .zip(&classes)
            .filter(|(_, c)| {
                matches!(
                    c,
                    OperationClass::IrreversibleExternal | OperationClass::Unknown
                )
            })
            .map(|(s, _)| s.method.clone())
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
        let (root, path, lock) = open_journal_lock(cache_path, execution_id)?;
        if path.exists() {
            let journal = read_journal(&path)?;
            check_identity(&journal, plan_id, execution_id)?;
            if journal.state == JournalState::Committed {
                remove_pins(&root, &journal)?;
                return Ok(BeginOutcome::AlreadyCommitted);
            }
            if journal.state == JournalState::RolledBack {
                return Err("execution was already rolled back; use a new execution_id".into());
            }
            return Ok(BeginOutcome::Transaction(Box::new(make_tx(
                root, path, lock, journal, fault_at,
            ))));
        }
        let journal = build_plan_journal(
            project_root,
            cache_path,
            plan_id,
            execution_id,
            &specs,
            &classes,
            true,
            None,
            JournalState::Prepared,
        );
        write_pins(&root, &journal)?;
        persist_journal(&path, &journal)?;
        let tx = make_tx(root, path, lock, journal, fault_at);
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
        let (root, path, lock) = open_journal_lock(cache_path, execution_id)?;
        if path.exists() {
            let existing = read_journal(&path)?;
            check_identity(&existing, plan_id, execution_id)?;
            if existing.atomic || existing.downgrade_reason.as_deref() != Some(reason) {
                return Err("journal replay classification changed".into());
            }
            return Ok(());
        }
        let journal = build_plan_journal(
            project_root,
            cache_path,
            plan_id,
            execution_id,
            specs,
            classes,
            false,
            Some(reason.to_string()),
            JournalState::Committed,
        );
        persist_journal(&path, &journal)?;
        enforce_retention(&root)?;
        let _ = FileExt::unlock(&lock);
        Ok(())
    }

    impl JournalTransaction {
        pub fn journal(&self) -> &PlanJournal {
            &self.journal
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
                .map_err(|e| format!("read precondition target: {e}"))?;
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
            if let (Some(expected), Some(actual)) =
                (&op.postcondition_digest, &postcondition_digest)
            {
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
                            return Err(CombinedRollbackError {
                                original,
                                rollback: vec![err],
                            });
                        }
                    }
                    Err(err) => {
                        let msg = bounded(&format!("step {index}: {err}"));
                        self.journal.operations[index].diagnostic = Some(msg.clone());
                        errors.push(msg);
                        let _ = self.persist();
                    }
                }
            }
            for op in &mut self.journal.operations {
                if matches!(op.state, StepState::Prepared | StepState::Applying) {
                    op.state = StepState::Skipped;
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

    fn journal_entries(root: &Path) -> io::Result<impl Iterator<Item = fs::DirEntry>> {
        Ok(fs::read_dir(root)?.flatten().filter(|entry| {
            entry.path().extension().and_then(|value| value.to_str()) == Some("json")
        }))
    }

    pub fn doctor(cache_path: &Path) -> JournalDoctorReport {
        let root = journal_root(cache_path);
        let mut report = JournalDoctorReport {
            schema_version: PLAN_JOURNAL_VERSION.into(),
            unresolved: Vec::new(),
            resolved_count: 0,
            corrupt: Vec::new(),
        };
        let Ok(entries) = journal_entries(&root) else {
            return report;
        };
        for entry in entries {
            let path = entry.path();
            match read_journal(&path) {
                Ok(j) if j.state.is_resolved() => {
                    report.resolved_count += 1;
                    if let Err(err) = remove_pins(&root, &j) {
                        report.corrupt.push(format!(
                            "{}: resolved pin cleanup failed: {}",
                            path.display(),
                            bounded(&err)
                        ));
                    }
                }
                Ok(j) => report.unresolved.push(JournalDoctorEntry {
                    execution_id: j.execution_id,
                    state: j.state,
                    journal_path: path.to_string_lossy().into_owned(),
                    next_step: next_step(j.state).into(),
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
        read_journal(
            &journal_root(cache_path).join(format!("{}.json", safe_execution_id(execution_id))),
        )
    }

    pub fn open_unresolved(
        cache_path: &Path,
        execution_id: &str,
    ) -> Result<JournalTransaction, String> {
        let (root, path, lock) = open_journal_lock(cache_path, execution_id)?;
        let journal = read_journal(&path)?;
        if journal.state.is_resolved() {
            return Err(format!(
                "journal {execution_id} is already resolved as {:?}",
                journal.state
            ));
        }
        Ok(make_tx(root, path, lock, journal, None))
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
        let bytes = fs::read(path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                format!("journal not found: {e}")
            } else {
                format!("read journal: {e}")
            }
        })?;
        let journal: PlanJournal =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse journal: {e}"))?;
        if journal.version != PLAN_JOURNAL_VERSION {
            return Err(format!("unsupported journal version {}", journal.version));
        }
        Ok(journal)
    }

    fn persist_journal(path: &Path, journal: &PlanJournal) -> Result<(), String> {
        let bytes =
            serde_json::to_vec_pretty(journal).map_err(|e| format!("serialize journal: {e}"))?;
        atomic_write(path, &bytes).map_err(|e| format!("persist journal: {e}"))
    }

    pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        static NEXT_TMP_ID: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp = parent.join(format!(
            ".{}.{}.{nonce}.{}.tmp",
            path.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("journal"),
            std::process::id(),
            NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            // Directory fsync is Unix-only: on Windows a directory handle
            // cannot be opened via `File::open` (no FILE_FLAG_BACKUP_SEMANTICS),
            // which would spuriously fail every journal write after the rename
            // already succeeded.
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
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

    fn pins<'a>(
        root: &'a Path,
        journal: &'a PlanJournal,
    ) -> impl Iterator<Item = (PathBuf, usize, String)> + 'a {
        journal.operations.iter().flat_map(move |operation| {
            operation.undo_refs.iter().filter_map(move |reference| {
                pin_hash(reference).map(|hash| {
                    let path = pin_path(root, &journal.execution_id, operation.index, &hash);
                    (path, operation.index, hash)
                })
            })
        })
    }

    fn write_pins(root: &Path, journal: &PlanJournal) -> Result<(), String> {
        for (path, index, hash) in pins(root, journal) {
            let pin_id = format!(
                "plan-{}-{}",
                &sha256_bytes(journal.execution_id.as_bytes())[..24],
                index
            );
            let record = json!({"schema_version":PIN_SCHEMA_VERSION,"record_type":"pin","engine":"tokenzero","project_id":journal.project_id,"pin_id":pin_id,"created_at":rfc3339_now(),"blob_hash":hash});
            let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
            atomic_write(&path, &bytes).map_err(|e| format!("persist undo pin: {e}"))?;
        }
        Ok(())
    }

    fn remove_pins(root: &Path, journal: &PlanJournal) -> Result<(), String> {
        for (path, _, _) in pins(root, journal) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("remove resolved pin {}: {error}", path.display()));
                }
            }
        }
        Ok(())
    }

    fn enforce_retention(root: &Path) -> Result<(), String> {
        let mut resolved = Vec::new();
        let entries = match journal_entries(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            let path = entry.path();
            if let Ok(j) = read_journal(&path) {
                if j.state.is_resolved() {
                    resolved.push((
                        j.updated_at_ms,
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
                .map_err(|e| format!("prune resolved journal {}: {e}", path.display()))?;
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
        let rem = seconds % 86_400;
        let (y, m, d) = civil_from_days((seconds / 86_400) as i64);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
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
}

pub(crate) use flows::atomic_write;
pub use flows::{
    begin_plan, current_digest, doctor, doctor_json, inspect, journal_root, open_unresolved,
    sha256_bytes,
};
