use crate::OperationalAction;
use crate::evidence::EvidenceAuthority;
use crate::probe::{OperationId, ProbeAuthorizerMetadata};
use crate::store_files::{
    RotatedActiveFile, StoreFileError, discover_generations, hash_json_value,
    repair_unterminated_tail, rotate_active_file, unix_time_ms, valid_hash,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;

pub const ACTION_LOG_SCHEMA_VERSION: u16 = 1;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const ACTION_LOG_ROTATE_DEFAULT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const ACTION_LOG_ROTATE_DEFAULT_MAX_ENTRIES: u64 = 100_000;
const ACTION_LOG_ROTATE_DEFAULT_GENERATIONS: u32 = 2;

/// Rotation policy for [`DurableActionLog`].
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
pub struct DurableActionLogOptions {
    /// Rotate once the active file reaches this many bytes.
    pub rotate_max_bytes: u64,
    /// Rotate once the active file holds this many journal entries (checkpoint included).
    pub rotate_max_entries: u64,
    /// Number of rotated generations retained on disk; `.1` is the most recent.
    pub generations: u32,
}

impl Default for DurableActionLogOptions {
    fn default() -> Self {
        Self {
            rotate_max_bytes: ACTION_LOG_ROTATE_DEFAULT_MAX_BYTES,
            rotate_max_entries: ACTION_LOG_ROTATE_DEFAULT_MAX_ENTRIES,
            generations: ACTION_LOG_ROTATE_DEFAULT_GENERATIONS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionResourceReservation {
    pub slots: u16,
    pub memory_bytes: u64,
    pub max_runtime_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionCatalogEntry {
    pub action: OperationalAction,
    pub maximum_reservation: ActionResourceReservation,
    pub target_required: bool,
}

pub const OPERATIONAL_ACTION_CATALOG: [ActionCatalogEntry; 6] = [
    ActionCatalogEntry {
        action: OperationalAction::IncreaseSampling,
        maximum_reservation: ActionResourceReservation {
            slots: 1,
            memory_bytes: 65_536,
            max_runtime_ms: 1_000,
        },
        target_required: false,
    },
    ActionCatalogEntry {
        action: OperationalAction::StartProbeRequest,
        maximum_reservation: ActionResourceReservation {
            slots: 1,
            memory_bytes: 524_288,
            max_runtime_ms: 30_000,
        },
        target_required: false,
    },
    ActionCatalogEntry {
        action: OperationalAction::QuarantineTransaction,
        maximum_reservation: ActionResourceReservation {
            slots: 1,
            memory_bytes: 65_536,
            max_runtime_ms: 1_000,
        },
        target_required: true,
    },
    ActionCatalogEntry {
        action: OperationalAction::ReleaseTransaction,
        maximum_reservation: ActionResourceReservation {
            slots: 1,
            memory_bytes: 65_536,
            max_runtime_ms: 1_000,
        },
        target_required: true,
    },
    ActionCatalogEntry {
        action: OperationalAction::RestartAiWorker,
        maximum_reservation: ActionResourceReservation {
            slots: 1,
            memory_bytes: 131_072,
            max_runtime_ms: 5_000,
        },
        target_required: false,
    },
    ActionCatalogEntry {
        action: OperationalAction::RestartConsensus,
        maximum_reservation: ActionResourceReservation {
            slots: 1,
            memory_bytes: 131_072,
            max_runtime_ms: 10_000,
        },
        target_required: false,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableActionBinding {
    pub schema_version: u16,
    pub operation_id: OperationId,
    pub action: OperationalAction,
    pub target: Option<String>,
    pub authorizer: ProbeAuthorizerMetadata,
    pub reservation: ActionResourceReservation,
}

impl DurableActionBinding {
    pub fn new(
        operation_id: OperationId,
        action: OperationalAction,
        target: Option<String>,
        authorizer: ProbeAuthorizerMetadata,
        reservation: ActionResourceReservation,
    ) -> Result<Self, DurableActionError> {
        let binding = Self {
            schema_version: ACTION_LOG_SCHEMA_VERSION,
            operation_id,
            action,
            target,
            authorizer,
            reservation,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), DurableActionError> {
        if self.schema_version != ACTION_LOG_SCHEMA_VERSION {
            return Err(DurableActionError::UnsupportedVersion(self.schema_version));
        }
        if self.authorizer.authority != EvidenceAuthority::DeterministicRule {
            return Err(DurableActionError::UntrustedAuthorizer);
        }
        if self.authorizer.evidence_id.is_empty()
            || !valid_hash(&self.authorizer.evidence_hash)
            || self.authorizer.rule_version.trim().is_empty()
            || self.authorizer.authorized_at_unix_ms == 0
        {
            return Err(DurableActionError::InvalidBinding);
        }
        let catalog = OPERATIONAL_ACTION_CATALOG
            .iter()
            .find(|entry| entry.action == self.action)
            .expect("complete operational action catalog");
        if catalog.target_required && self.target.as_deref().is_none_or(str::is_empty)
            || self
                .target
                .as_ref()
                .is_some_and(|target| target.is_empty() || target.len() > 128)
            || self.reservation.slots == 0
            || self.reservation.memory_bytes == 0
            || self.reservation.max_runtime_ms == 0
            || self.reservation.slots > catalog.maximum_reservation.slots
            || self.reservation.memory_bytes > catalog.maximum_reservation.memory_bytes
            || self.reservation.max_runtime_ms > catalog.maximum_reservation.max_runtime_ms
        {
            return Err(DurableActionError::InvalidReservation);
        }
        Ok(())
    }

    pub fn binding_hash(&self) -> Result<String, DurableActionError> {
        hash_json_value(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableActionState {
    AuthorizedReserved,
    ReadyForEffect,
    EffectInFlight,
    EffectApplied,
    FailedBeforeEffect,
    Uncertain,
    ReconciliationRequired,
    Completed,
}

impl DurableActionState {
    #[must_use]
    pub const fn effect_may_have_occurred(self) -> bool {
        matches!(
            self,
            Self::EffectInFlight
                | Self::EffectApplied
                | Self::Uncertain
                | Self::ReconciliationRequired
                | Self::Completed
        )
    }

    #[must_use]
    pub const fn may_execute(self) -> bool {
        matches!(self, Self::ReadyForEffect)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableActionSnapshot {
    pub binding: DurableActionBinding,
    pub binding_hash: String,
    pub state: DurableActionState,
    pub updated_at_unix_ms: u64,
    pub outcome_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionTransition {
    PreAuditSucceeded,
    PreAuditFailed,
    EffectStarted,
    EffectCompleted,
    EffectCompletionLost,
    PostAuditSucceeded,
    PostAuditFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationDecision {
    ConfirmedAndAudited,
    ConfirmedAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionAdmissionOutcome {
    Admitted(DurableActionSnapshot),
    Existing(DurableActionSnapshot),
}

#[derive(Debug)]
struct DurableActionLogState {
    path: PathBuf,
    file: File,
    options: DurableActionLogOptions,
    /// Size of the active journal file in bytes.
    file_bytes: u64,
    /// Journal entries in the active file, including its leading rotation checkpoint.
    file_entries: u64,
    /// Global entry count across all retained generations (checkpoints included).
    total_entries: u64,
    /// `entry_hash` of the most recent entry, or the genesis hash when empty.
    tail_hash: String,
    operations: BTreeMap<OperationId, DurableActionSnapshot>,
}

/// Synchronous durable state machine. Callers place it behind the bounded admission API.
#[derive(Debug)]
pub struct DurableActionLog {
    state: Mutex<DurableActionLogState>,
}

impl DurableActionLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DurableActionError> {
        Self::open_with_options(path, DurableActionLogOptions::default())
    }

    /// Opens (or creates) the log with an explicit rotation policy.
    ///
    /// Fails closed (see [`DurableActionLogOptions`]) when any retained generation or
    /// the cross-file checkpoint chain is broken. If a previous rotation was
    /// interrupted between the rename and the checkpoint write (the active file is
    /// missing or empty while generations remain), the active file is re-created with
    /// the checkpoint linking to the retained tail.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: DurableActionLogOptions,
    ) -> Result<Self, DurableActionError> {
        if options.generations == 0
            || options.rotate_max_bytes == 0
            || options.rotate_max_entries == 0
        {
            return Err(DurableActionError::InvalidOptions);
        }
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| DurableActionError::Unavailable)?;
        }
        repair_unterminated_tail::<DurableActionError>(&path)?;
        // Chronological order: oldest retained generation first, active file last.
        let mut files: Vec<PathBuf> = discover_generations(&path).into_iter().rev().collect();
        let active_bytes = if path.exists() {
            std::fs::metadata(&path)
                .map_err(|_| DurableActionError::Unavailable)?
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
                return Err(DurableActionError::BrokenJournal(total_entries));
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
            .map_err(|_| DurableActionError::Unavailable)?;
        let mut state = DurableActionLogState {
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
        let log = Self {
            state: Mutex::new(state),
        };
        log.mark_interrupted_operations()?;
        Ok(log)
    }

    pub fn admit(
        &self,
        binding: DurableActionBinding,
        now_unix_ms: u64,
    ) -> Result<ActionAdmissionOutcome, DurableActionError> {
        binding.validate()?;
        if now_unix_ms == 0 || binding.authorizer.authorized_at_unix_ms > now_unix_ms {
            return Err(DurableActionError::InvalidTimestamp);
        }
        let binding_hash = binding.binding_hash()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| DurableActionError::Unavailable)?;
        if let Some(existing) = state.operations.get(&binding.operation_id) {
            return if existing.binding_hash == binding_hash {
                Ok(ActionAdmissionOutcome::Existing(existing.clone()))
            } else {
                Err(DurableActionError::BindingConflict)
            };
        }
        let snapshot = DurableActionSnapshot {
            binding,
            binding_hash,
            state: DurableActionState::AuthorizedReserved,
            updated_at_unix_ms: now_unix_ms,
            outcome_code: "authorized_reserved".to_owned(),
        };
        append_snapshot(&mut state, snapshot.clone())?;
        Ok(ActionAdmissionOutcome::Admitted(snapshot))
    }

    pub fn transition(
        &self,
        operation_id: &OperationId,
        transition: ActionTransition,
        now_unix_ms: u64,
    ) -> Result<DurableActionSnapshot, DurableActionError> {
        if now_unix_ms == 0 {
            return Err(DurableActionError::InvalidTimestamp);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| DurableActionError::Unavailable)?;
        let current = state
            .operations
            .get(operation_id)
            .cloned()
            .ok_or(DurableActionError::NotFound)?;
        let (next, outcome_code) = next_state(current.state, transition)?;
        let mut snapshot = current;
        snapshot.state = next;
        snapshot.updated_at_unix_ms = now_unix_ms;
        outcome_code.clone_into(&mut snapshot.outcome_code);
        append_snapshot(&mut state, snapshot.clone())?;
        Ok(snapshot)
    }

    pub fn query(
        &self,
        operation_id: &OperationId,
    ) -> Result<Option<DurableActionSnapshot>, DurableActionError> {
        self.state
            .lock()
            .map(|state| state.operations.get(operation_id).cloned())
            .map_err(|_| DurableActionError::Unavailable)
    }

    pub fn reconcile(
        &self,
        operation_id: &OperationId,
        decision: ReconciliationDecision,
        now_unix_ms: u64,
    ) -> Result<DurableActionSnapshot, DurableActionError> {
        if now_unix_ms == 0 {
            return Err(DurableActionError::InvalidTimestamp);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| DurableActionError::Unavailable)?;
        let current = state
            .operations
            .get(operation_id)
            .cloned()
            .ok_or(DurableActionError::NotFound)?;
        if !matches!(
            current.state,
            DurableActionState::Uncertain | DurableActionState::ReconciliationRequired
        ) {
            return Err(DurableActionError::InvalidTransition {
                from: current.state,
            });
        }
        let (next, outcome_code) = match decision {
            ReconciliationDecision::ConfirmedAndAudited => {
                (DurableActionState::Completed, "reconciled_completed")
            }
            ReconciliationDecision::ConfirmedAbsent => {
                (DurableActionState::FailedBeforeEffect, "reconciled_absent")
            }
        };
        let mut snapshot = current;
        snapshot.state = next;
        snapshot.updated_at_unix_ms = now_unix_ms;
        outcome_code.clone_into(&mut snapshot.outcome_code);
        append_snapshot(&mut state, snapshot.clone())?;
        Ok(snapshot)
    }

    fn mark_interrupted_operations(&self) -> Result<(), DurableActionError> {
        let interrupted = {
            let state = self
                .state
                .lock()
                .map_err(|_| DurableActionError::Unavailable)?;
            state
                .operations
                .iter()
                .filter_map(|(id, snapshot)| match snapshot.state {
                    DurableActionState::EffectInFlight => {
                        Some((id.clone(), DurableActionState::Uncertain, "completion_lost"))
                    }
                    DurableActionState::EffectApplied => Some((
                        id.clone(),
                        DurableActionState::ReconciliationRequired,
                        "post_effect_audit_incomplete",
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        for (operation_id, next, outcome_code) in interrupted {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DurableActionError::Unavailable)?;
            let mut snapshot = state
                .operations
                .get(&operation_id)
                .cloned()
                .ok_or(DurableActionError::NotFound)?;
            snapshot.state = next;
            snapshot.updated_at_unix_ms = unix_time_ms();
            outcome_code.clone_into(&mut snapshot.outcome_code);
            append_snapshot(&mut state, snapshot)?;
        }
        Ok(())
    }
}

fn next_state(
    state: DurableActionState,
    transition: ActionTransition,
) -> Result<(DurableActionState, &'static str), DurableActionError> {
    match (state, transition) {
        (DurableActionState::AuthorizedReserved, ActionTransition::PreAuditSucceeded) => {
            Ok((DurableActionState::ReadyForEffect, "pre_audit_succeeded"))
        }
        (DurableActionState::AuthorizedReserved, ActionTransition::PreAuditFailed) => {
            Ok((DurableActionState::FailedBeforeEffect, "pre_audit_failed"))
        }
        (DurableActionState::ReadyForEffect, ActionTransition::EffectStarted) => {
            Ok((DurableActionState::EffectInFlight, "effect_started"))
        }
        (DurableActionState::EffectInFlight, ActionTransition::EffectCompleted) => {
            Ok((DurableActionState::EffectApplied, "effect_completed"))
        }
        (DurableActionState::EffectInFlight, ActionTransition::EffectCompletionLost) => {
            Ok((DurableActionState::Uncertain, "completion_lost"))
        }
        (DurableActionState::EffectApplied, ActionTransition::PostAuditSucceeded) => {
            Ok((DurableActionState::Completed, "post_audit_succeeded"))
        }
        (DurableActionState::EffectApplied, ActionTransition::PostAuditFailed) => Ok((
            DurableActionState::ReconciliationRequired,
            "post_audit_failed",
        )),
        (from, _) => Err(DurableActionError::InvalidTransition { from }),
    }
}

fn calculate_entry_hash(entry: &ActionJournalEntry) -> Result<String, DurableActionError> {
    // The record hash material intentionally mirrors the pre-rotation field layout
    // so journals written by earlier versions keep verifying.
    #[derive(Serialize)]
    struct RecordHashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        snapshot: &'a DurableActionSnapshot,
        previous_hash: &'a str,
    }
    #[derive(Serialize)]
    struct RotationHashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        rotation: &'a ActionRotationCheckpoint,
        previous_hash: &'a str,
    }
    match (&entry.rotation, &entry.snapshot) {
        (Some(rotation), None) => hash_json_value(&RotationHashMaterial {
            schema_version: entry.schema_version,
            sequence: entry.sequence,
            rotation,
            previous_hash: &entry.previous_hash,
        }),
        (None, Some(snapshot)) => hash_json_value(&RecordHashMaterial {
            schema_version: entry.schema_version,
            sequence: entry.sequence,
            snapshot,
            previous_hash: &entry.previous_hash,
        }),
        _ => Err(DurableActionError::Serialization),
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DurableActionError {
    #[error("unsupported durable action schema version: {0}")]
    UnsupportedVersion(u16),
    #[error("durable action binding is invalid")]
    InvalidBinding,
    #[error("durable action log rotation options are invalid")]
    InvalidOptions,
    #[error("durable action resource reservation is invalid")]
    InvalidReservation,
    #[error("model or observation authority cannot authorize a durable action")]
    UntrustedAuthorizer,
    #[error("operation ID is already bound to different content")]
    BindingConflict,
    #[error("durable action operation was not found")]
    NotFound,
    #[error("invalid durable action transition from {from:?}")]
    InvalidTransition { from: DurableActionState },
    #[error("durable action timestamp is invalid")]
    InvalidTimestamp,
    #[error("durable action log is unavailable")]
    Unavailable,
    #[error("durable action serialization failed")]
    Serialization,
    #[error("durable action journal is broken at sequence {0}")]
    BrokenJournal(u64),
}

impl StoreFileError for DurableActionError {
    fn unavailable() -> Self {
        Self::Unavailable
    }

    fn serialization() -> Self {
        Self::Serialization
    }
}

mod journal;
mod rotate;

use journal::{ActionJournalEntry, append_snapshot, read_entries, verify_entries};
use rotate::{ActionRotationCheckpoint, rotate, write_rotation_checkpoint};

#[cfg(test)]
mod tests;
