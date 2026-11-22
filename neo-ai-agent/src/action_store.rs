//! Durable append-only state for bounded diagnostic actions.

use crate::store_files::{
    RotatedActiveFile, StoreFileError, discover_generations, hash_json_value,
    repair_unterminated_tail, rotate_active_file, unix_time_ms, valid_hash,
};
use crate::{OperationId, OperationalAction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;

const ACTION_STORE_SCHEMA_VERSION: u16 = 1;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const ACTION_STORE_ROTATE_DEFAULT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const ACTION_STORE_ROTATE_DEFAULT_MAX_ENTRIES: u64 = 100_000;
const ACTION_STORE_ROTATE_DEFAULT_GENERATIONS: u32 = 2;

/// Rotation policy for [`DurableActionStore`].
///
/// The journal rotates once the active file reaches the byte or entry threshold
/// (16 MiB / 100_000 entries by default). On rotation the active file is renamed to
/// `<path>.1` — up to `generations` (default 2) rotated copies are retained, `.1`
/// being the most recent — and the new active file opens with a *rotation checkpoint*
/// entry whose `previous_hash` and carried `previous_tail_hash` equal the final entry
/// hash of the previous generation, keeping the SHA-256 chain continuous across
/// files. Opening re-verifies every retained generation and fails closed on
/// corruption. The state machine, idempotent admission, and crash recovery semantics
/// are unchanged: the operations map is rebuilt by replaying every retained
/// generation in chronological order. Idempotent admission is guaranteed within the
/// retained window: an operation whose every entry has been pruned by retention is
/// forgotten and may be admitted anew.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionStoreOptions {
    /// Rotate once the active file reaches this many bytes.
    pub rotate_max_bytes: u64,
    /// Rotate once the active file holds this many journal entries (checkpoint included).
    pub rotate_max_entries: u64,
    /// Number of rotated generations retained on disk; `.1` is the most recent.
    pub generations: u32,
}

impl Default for ActionStoreOptions {
    fn default() -> Self {
        Self {
            rotate_max_bytes: ACTION_STORE_ROTATE_DEFAULT_MAX_BYTES,
            rotate_max_entries: ACTION_STORE_ROTATE_DEFAULT_MAX_ENTRIES,
            generations: ACTION_STORE_ROTATE_DEFAULT_GENERATIONS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBinding {
    schema_version: u16,
    pub operation_id: OperationId,
    pub action: OperationalAction,
    pub target: Option<String>,
    pub evidence_ids: Vec<String>,
    pub evidence_hashes: Vec<String>,
    pub rule_version: String,
}

impl ActionBinding {
    pub fn new(
        operation_id: OperationId,
        action: OperationalAction,
        target: Option<String>,
        evidence_ids: Vec<String>,
        evidence_hashes: Vec<String>,
        rule_version: String,
    ) -> Result<Self, ActionStoreError> {
        let binding = Self {
            schema_version: ACTION_STORE_SCHEMA_VERSION,
            operation_id,
            action,
            target,
            evidence_ids,
            evidence_hashes,
            rule_version,
        };
        binding.validate()?;
        Ok(binding)
    }

    fn validate(&self) -> Result<(), ActionStoreError> {
        if self.schema_version != ACTION_STORE_SCHEMA_VERSION {
            return Err(ActionStoreError::UnsupportedVersion(self.schema_version));
        }
        if self.evidence_ids.is_empty()
            || self.evidence_ids.len() != self.evidence_hashes.len()
            || self
                .evidence_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128)
            || self.evidence_hashes.iter().any(|hash| !valid_hash(hash))
            || self
                .target
                .as_ref()
                .is_some_and(|target| target.is_empty() || target.len() > 128)
            || self.rule_version.trim().is_empty()
            || self.rule_version.len() > 128
        {
            return Err(ActionStoreError::InvalidBinding);
        }
        Ok(())
    }

    fn content_hash(&self) -> Result<String, ActionStoreError> {
        self.validate()?;
        hash_value(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Admitted,
    Running,
    Completed,
    Uncertain,
    ReconciliationRequired,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRecord {
    pub binding: ActionBinding,
    pub binding_hash: String,
    pub state: ActionState,
    pub updated_at_unix_ms: u64,
    pub outcome_code: String,
}

#[derive(Debug)]
struct StoreState {
    path: PathBuf,
    file: File,
    options: ActionStoreOptions,
    /// Size of the active journal file in bytes.
    file_bytes: u64,
    /// Journal entries in the active file, including its leading rotation checkpoint.
    file_entries: u64,
    /// Global entry count across all retained generations (checkpoints included).
    total_entries: u64,
    /// `entry_hash` of the most recent entry, or the genesis hash when empty.
    tail_hash: String,
    operations: BTreeMap<OperationId, ActionRecord>,
}

#[derive(Debug)]
pub struct DurableActionStore {
    state: Mutex<StoreState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitOutcome {
    Admitted(ActionRecord),
    Existing(ActionRecord),
}

impl DurableActionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ActionStoreError> {
        Self::open_with_options(path, ActionStoreOptions::default())
    }

    /// Opens (or creates) the store with an explicit rotation policy.
    ///
    /// Fails closed (see [`ActionStoreOptions`]) when any retained generation or the
    /// cross-file checkpoint chain is broken. If a previous rotation was interrupted
    /// between the rename and the checkpoint write (the active file is missing or
    /// empty while generations remain), the active file is re-created with the
    /// checkpoint linking to the retained tail.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: ActionStoreOptions,
    ) -> Result<Self, ActionStoreError> {
        if options.generations == 0
            || options.rotate_max_bytes == 0
            || options.rotate_max_entries == 0
        {
            return Err(ActionStoreError::InvalidOptions);
        }
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| ActionStoreError::Unavailable)?;
        }
        repair_unterminated_tail::<ActionStoreError>(&path)?;
        // Chronological order: oldest retained generation first, active file last.
        let mut files: Vec<PathBuf> = discover_generations(&path).into_iter().rev().collect();
        let active_bytes = if path.exists() {
            std::fs::metadata(&path)
                .map_err(|_| ActionStoreError::Unavailable)?
                .len()
        } else {
            0
        };
        if active_bytes > 0 {
            files.push(path.clone());
        }
        let mut operations = BTreeMap::new();
        let mut tail_hash = GENESIS_HASH.to_owned();
        let mut total_entries = 0u64;
        let mut active_file_entries = 0u64;
        let mut history_pruned = false;
        for (index, file_path) in files.iter().enumerate() {
            let entries = read_entries(file_path)?;
            if entries.is_empty() {
                // Retained generation files are never empty: they were fully synced
                // before being rotated. An empty file indicates corruption.
                return Err(ActionStoreError::BrokenJournal(total_entries));
            }
            if index == 0 {
                // The retained history begins with a checkpoint exactly when older
                // generations were pruned by retention; operation admissions may then
                // be missing and their first retained entry acts as the anchor.
                history_pruned = entries
                    .first()
                    .is_some_and(|entry| entry.rotation.is_some());
            }
            if index + 1 == files.len() && active_bytes > 0 {
                active_file_entries = entries.len() as u64;
            }
            verify_entries(
                &entries,
                index == 0,
                history_pruned,
                &mut tail_hash,
                &mut total_entries,
                &mut operations,
            )?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|_| ActionStoreError::Unavailable)?;
        let mut state = StoreState {
            path,
            file,
            options,
            file_bytes: active_bytes,
            file_entries: active_file_entries,
            total_entries,
            tail_hash,
            operations,
        };
        if active_bytes == 0 && !files.is_empty() {
            // A rotation was interrupted between the rename and the checkpoint
            // write: re-create the active file with the linking checkpoint.
            write_rotation_checkpoint(&mut state)?;
        }
        let store = Self {
            state: Mutex::new(state),
        };
        store.recover_running()?;
        Ok(store)
    }

    pub fn admit(
        &self,
        binding: ActionBinding,
        now_unix_ms: u64,
    ) -> Result<AdmitOutcome, ActionStoreError> {
        binding.validate()?;
        validate_timestamp(now_unix_ms)?;
        let binding_hash = binding.content_hash()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ActionStoreError::Unavailable)?;
        if let Some(existing) = state.operations.get(&binding.operation_id) {
            return if existing.binding_hash == binding_hash {
                Ok(AdmitOutcome::Existing(existing.clone()))
            } else {
                Err(ActionStoreError::BindingConflict)
            };
        }
        let record = ActionRecord {
            binding,
            binding_hash,
            state: ActionState::Admitted,
            updated_at_unix_ms: now_unix_ms,
            outcome_code: "admitted".to_owned(),
        };
        append(&mut state, record.clone())?;
        Ok(AdmitOutcome::Admitted(record))
    }

    pub fn mark_running(
        &self,
        operation_id: &OperationId,
        now_unix_ms: u64,
    ) -> Result<ActionRecord, ActionStoreError> {
        self.transition(
            operation_id,
            &[ActionState::Admitted],
            ActionState::Running,
            "running",
            now_unix_ms,
        )
    }

    pub fn mark_completed(
        &self,
        operation_id: &OperationId,
        now_unix_ms: u64,
    ) -> Result<ActionRecord, ActionStoreError> {
        self.transition(
            operation_id,
            &[ActionState::Running],
            ActionState::Completed,
            "completed",
            now_unix_ms,
        )
    }

    pub fn mark_uncertain(
        &self,
        operation_id: &OperationId,
        now_unix_ms: u64,
    ) -> Result<ActionRecord, ActionStoreError> {
        self.transition(
            operation_id,
            &[ActionState::Running],
            ActionState::Uncertain,
            "completion_lost",
            now_unix_ms,
        )
    }

    pub fn mark_reconciliation_required(
        &self,
        operation_id: &OperationId,
        now_unix_ms: u64,
    ) -> Result<ActionRecord, ActionStoreError> {
        self.transition(
            operation_id,
            &[ActionState::Running],
            ActionState::ReconciliationRequired,
            "post_effect_audit_failed",
            now_unix_ms,
        )
    }

    pub fn reject(
        &self,
        operation_id: &OperationId,
        outcome_code: &'static str,
        now_unix_ms: u64,
    ) -> Result<ActionRecord, ActionStoreError> {
        if !matches!(outcome_code, "pre_audit_failed" | "effect_rejected") {
            return Err(ActionStoreError::InvalidOutcomeCode);
        }
        self.transition(
            operation_id,
            &[ActionState::Admitted, ActionState::Running],
            ActionState::Rejected,
            outcome_code,
            now_unix_ms,
        )
    }

    pub fn query(
        &self,
        operation_id: &OperationId,
    ) -> Result<Option<ActionRecord>, ActionStoreError> {
        self.state
            .lock()
            .map(|state| state.operations.get(operation_id).cloned())
            .map_err(|_| ActionStoreError::Unavailable)
    }

    fn transition(
        &self,
        operation_id: &OperationId,
        expected: &[ActionState],
        next: ActionState,
        outcome_code: &str,
        now_unix_ms: u64,
    ) -> Result<ActionRecord, ActionStoreError> {
        validate_timestamp(now_unix_ms)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ActionStoreError::Unavailable)?;
        let mut record = state
            .operations
            .get(operation_id)
            .cloned()
            .ok_or(ActionStoreError::NotFound)?;
        if !expected.contains(&record.state) {
            return Err(ActionStoreError::InvalidTransition(record.state));
        }
        record.state = next;
        record.updated_at_unix_ms = now_unix_ms;
        outcome_code.clone_into(&mut record.outcome_code);
        append(&mut state, record.clone())?;
        Ok(record)
    }

    fn recover_running(&self) -> Result<(), ActionStoreError> {
        let running = {
            let state = self
                .state
                .lock()
                .map_err(|_| ActionStoreError::Unavailable)?;
            state
                .operations
                .iter()
                .filter(|(_, record)| record.state == ActionState::Running)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        for operation_id in running {
            self.mark_uncertain(&operation_id, unix_time_ms())?;
        }
        Ok(())
    }
}

fn hash_value(value: &impl Serialize) -> Result<String, ActionStoreError> {
    let hash = hash_json_value::<ActionStoreError>(value)?;
    if !valid_hash(&hash) {
        return Err(ActionStoreError::Serialization);
    }
    Ok(hash)
}

fn validate_timestamp(now_unix_ms: u64) -> Result<(), ActionStoreError> {
    if now_unix_ms == 0 {
        Err(ActionStoreError::InvalidTimestamp)
    } else {
        Ok(())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ActionStoreError {
    #[error("unsupported action store schema version: {0}")]
    UnsupportedVersion(u16),
    #[error("action binding is invalid")]
    InvalidBinding,
    #[error("action store rotation options are invalid")]
    InvalidOptions,
    #[error("operation ID is already bound to different content")]
    BindingConflict,
    #[error("action operation was not found")]
    NotFound,
    #[error("invalid action transition from {0:?}")]
    InvalidTransition(ActionState),
    #[error("action outcome code is invalid")]
    InvalidOutcomeCode,
    #[error("action timestamp is invalid")]
    InvalidTimestamp,
    #[error("action store is unavailable")]
    Unavailable,
    #[error("action store serialization failed")]
    Serialization,
    #[error("action journal is broken at sequence {0}")]
    BrokenJournal(u64),
}

impl StoreFileError for ActionStoreError {
    fn unavailable() -> Self {
        Self::Unavailable
    }

    fn serialization() -> Self {
        Self::Serialization
    }
}

mod journal;

use journal::{append, read_entries, verify_entries, write_rotation_checkpoint};

#[cfg(test)]
mod tests;
