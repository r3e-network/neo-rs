use crate::OperationalAction;
use crate::evidence::EvidenceAuthority;
use crate::probe::{OperationId, ProbeAuthorizerMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
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
        hash_value(self)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionJournalEntry {
    schema_version: u16,
    sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotation: Option<ActionRotationCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot: Option<DurableActionSnapshot>,
    previous_hash: String,
    entry_hash: String,
}

/// Rotation checkpoint opening a fresh journal file. `previous_tail_hash` carries the
/// `entry_hash` of the final entry of the previous generation file and must mirror
/// `ActionJournalEntry::previous_hash`, keeping the hash chain continuous across files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionRotationCheckpoint {
    previous_tail_hash: String,
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
        repair_unterminated_tail(&path)?;
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
        snapshot.outcome_code = outcome_code.to_owned();
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
        snapshot.outcome_code = outcome_code.to_owned();
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
            snapshot.outcome_code = outcome_code.to_owned();
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

fn append_snapshot(
    state: &mut DurableActionLogState,
    snapshot: DurableActionSnapshot,
) -> Result<(), DurableActionError> {
    if state.file_entries > 0
        && (state.file_entries >= state.options.rotate_max_entries
            || state.file_bytes >= state.options.rotate_max_bytes)
    {
        rotate(state)?;
    }
    let sequence = state.total_entries;
    let operation_id = snapshot.binding.operation_id.clone();
    let previous_hash = state.tail_hash.clone();
    let mut entry = ActionJournalEntry {
        schema_version: ACTION_LOG_SCHEMA_VERSION,
        sequence,
        rotation: None,
        snapshot: Some(snapshot),
        previous_hash,
        entry_hash: String::new(),
    };
    entry.entry_hash = calculate_entry_hash(&entry)?;
    let encoded = serde_json::to_vec(&entry).map_err(|_| DurableActionError::Serialization)?;
    state
        .file
        .write_all(&encoded)
        .and_then(|()| state.file.write_all(b"\n"))
        .and_then(|()| state.file.sync_data())
        .map_err(|_| DurableActionError::Unavailable)?;
    state.file_bytes += encoded.len() as u64 + 1;
    state.file_entries += 1;
    state.total_entries = sequence + 1;
    state.tail_hash = entry.entry_hash.clone();
    state.operations.insert(
        operation_id,
        entry.snapshot.expect("record entries carry a snapshot"),
    );
    Ok(())
}

/// Reads the JSONL entries of a single journal file (empty when it does not exist).
fn read_entries(path: &Path) -> Result<Vec<ActionJournalEntry>, DurableActionError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path).map_err(|_| DurableActionError::Unavailable)?;
    let mut entries = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        entries.push(serde_json::from_slice(line).map_err(|_| DurableActionError::Serialization)?);
    }
    Ok(entries)
}

/// Verifies one file's entries as part of the global chain and state machine replay.
///
/// `is_first_retained_file` marks the chronologically oldest retained file, which may
/// anchor on a checkpoint whose predecessor was pruned by retention; every later file
/// must open with a rotation checkpoint whose carried hash equals the running tail.
/// When `history_pruned` is set, the first retained entry of an operation may be a
/// transition instead of its admission (the admission entry was pruned); the entry
/// then anchors the operation and later entries are still fully validated.
fn verify_entries(
    entries: &[ActionJournalEntry],
    is_first_retained_file: bool,
    history_pruned: bool,
    tail_hash: &mut String,
    total_entries: &mut u64,
    operations: &mut BTreeMap<OperationId, DurableActionSnapshot>,
) -> Result<(), DurableActionError> {
    for (position, entry) in entries.iter().enumerate() {
        if entry.schema_version != ACTION_LOG_SCHEMA_VERSION {
            return Err(DurableActionError::UnsupportedVersion(entry.schema_version));
        }
        if position == 0 && !is_first_retained_file {
            let rotation = entry
                .rotation
                .as_ref()
                .ok_or(DurableActionError::BrokenJournal(entry.sequence))?;
            if entry.snapshot.is_some()
                || rotation.previous_tail_hash != *tail_hash
                || entry.previous_hash != *tail_hash
            {
                return Err(DurableActionError::BrokenJournal(entry.sequence));
            }
        }
        if let Some(rotation) = entry.rotation.as_ref()
            && (entry.snapshot.is_some() || rotation.previous_tail_hash != entry.previous_hash)
        {
            return Err(DurableActionError::BrokenJournal(entry.sequence));
        }
        let anchored = position == 0 && is_first_retained_file && entry.rotation.is_some();
        if !anchored && (entry.sequence != *total_entries || entry.previous_hash != *tail_hash) {
            return Err(DurableActionError::BrokenJournal(entry.sequence));
        }
        if calculate_entry_hash(entry)? != entry.entry_hash {
            return Err(DurableActionError::BrokenJournal(entry.sequence));
        }
        if let Some(snapshot) = entry.snapshot.as_ref() {
            snapshot.binding.validate()?;
            if snapshot.binding_hash != snapshot.binding.binding_hash()?
                || snapshot.updated_at_unix_ms == 0
                || snapshot.outcome_code.is_empty()
            {
                return Err(DurableActionError::BrokenJournal(entry.sequence));
            }
            let operation_id = snapshot.binding.operation_id.clone();
            if let Some(current) = operations.get(&operation_id) {
                if current.binding != snapshot.binding
                    || current.binding_hash != snapshot.binding_hash
                    || current.updated_at_unix_ms > snapshot.updated_at_unix_ms
                    || !valid_replayed_transition(
                        current.state,
                        snapshot.state,
                        &snapshot.outcome_code,
                    )
                {
                    return Err(DurableActionError::BrokenJournal(entry.sequence));
                }
            } else if (snapshot.state != DurableActionState::AuthorizedReserved
                || snapshot.outcome_code != "authorized_reserved")
                && !history_pruned
            {
                return Err(DurableActionError::BrokenJournal(entry.sequence));
            }
            // The admission entry was pruned by retention: this entry anchors the
            // operation and all of its later entries are still validated below.
            operations.insert(operation_id, snapshot.clone());
        }
        *tail_hash = entry.entry_hash.clone();
        *total_entries = entry.sequence + 1;
    }
    Ok(())
}

/// Rotates the active file to `<path>.1` and re-creates it with a linking checkpoint.
fn rotate(state: &mut DurableActionLogState) -> Result<(), DurableActionError> {
    state
        .file
        .sync_all()
        .map_err(|_| DurableActionError::Unavailable)?;
    // Close the active handle so the rename is portable across platforms.
    let closed = std::mem::replace(&mut state.file, null_device_file()?);
    drop(closed);
    // Drop generations beyond the retention count (e.g. after the policy shrank).
    let mut stale = state.options.generations + 1;
    loop {
        let candidate = generation_path(&state.path, stale);
        if !candidate.exists() {
            break;
        }
        std::fs::remove_file(&candidate).map_err(|_| DurableActionError::Unavailable)?;
        stale += 1;
    }
    // Drop the oldest retained generation and shift the remaining ones down.
    let oldest = generation_path(&state.path, state.options.generations);
    if oldest.exists() {
        std::fs::remove_file(&oldest).map_err(|_| DurableActionError::Unavailable)?;
    }
    for generation in (2..=state.options.generations).rev() {
        let from = generation_path(&state.path, generation - 1);
        if from.exists() {
            std::fs::rename(&from, generation_path(&state.path, generation))
                .map_err(|_| DurableActionError::Unavailable)?;
        }
    }
    std::fs::rename(&state.path, generation_path(&state.path, 1))
        .map_err(|_| DurableActionError::Unavailable)?;
    state.file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)
        .map_err(|_| DurableActionError::Unavailable)?;
    state.file_bytes = 0;
    state.file_entries = 0;
    write_rotation_checkpoint(state)
}

/// Appends the checkpoint that links a fresh active file to the retained tail.
fn write_rotation_checkpoint(state: &mut DurableActionLogState) -> Result<(), DurableActionError> {
    let checkpoint = ActionRotationCheckpoint {
        previous_tail_hash: state.tail_hash.clone(),
    };
    let sequence = state.total_entries;
    let mut entry = ActionJournalEntry {
        schema_version: ACTION_LOG_SCHEMA_VERSION,
        sequence,
        rotation: Some(checkpoint),
        snapshot: None,
        previous_hash: state.tail_hash.clone(),
        entry_hash: String::new(),
    };
    entry.entry_hash = calculate_entry_hash(&entry)?;
    let encoded = serde_json::to_vec(&entry).map_err(|_| DurableActionError::Serialization)?;
    state
        .file
        .write_all(&encoded)
        .and_then(|()| state.file.write_all(b"\n"))
        .and_then(|()| state.file.sync_data())
        .map_err(|_| DurableActionError::Unavailable)?;
    state.file_bytes = encoded.len() as u64 + 1;
    state.file_entries = 1;
    state.total_entries = sequence + 1;
    state.tail_hash = entry.entry_hash.clone();
    Ok(())
}

/// Path of a rotated generation, where generation 1 is the most recent archive.
fn generation_path(path: &Path, generation: u32) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{generation}"));
    PathBuf::from(name)
}

/// Discovers retained generations (`.1` newest first) with a defensive scan cap.
fn discover_generations(path: &Path) -> Vec<PathBuf> {
    const MAX_GENERATION_SCAN: u32 = 10_000;
    let mut generations = Vec::new();
    for generation in 1..=MAX_GENERATION_SCAN {
        let candidate = generation_path(path, generation);
        if !candidate.exists() {
            break;
        }
        generations.push(candidate);
    }
    generations
}

fn null_device_file() -> Result<File, DurableActionError> {
    #[cfg(windows)]
    let path = "NUL";
    #[cfg(not(windows))]
    let path = "/dev/null";
    OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| DurableActionError::Unavailable)
}

fn valid_replayed_transition(
    from: DurableActionState,
    to: DurableActionState,
    outcome_code: &str,
) -> bool {
    matches!(
        (from, to, outcome_code),
        (
            DurableActionState::AuthorizedReserved,
            DurableActionState::ReadyForEffect,
            "pre_audit_succeeded"
        ) | (
            DurableActionState::AuthorizedReserved,
            DurableActionState::FailedBeforeEffect,
            "pre_audit_failed"
        ) | (
            DurableActionState::ReadyForEffect,
            DurableActionState::EffectInFlight,
            "effect_started"
        ) | (
            DurableActionState::EffectInFlight,
            DurableActionState::EffectApplied,
            "effect_completed"
        ) | (
            DurableActionState::EffectInFlight,
            DurableActionState::Uncertain,
            "completion_lost"
        ) | (
            DurableActionState::EffectApplied,
            DurableActionState::Completed,
            "post_audit_succeeded"
        ) | (
            DurableActionState::EffectApplied,
            DurableActionState::ReconciliationRequired,
            "post_audit_failed" | "post_effect_audit_incomplete"
        ) | (
            DurableActionState::Uncertain | DurableActionState::ReconciliationRequired,
            DurableActionState::Completed,
            "reconciled_completed"
        ) | (
            DurableActionState::Uncertain | DurableActionState::ReconciliationRequired,
            DurableActionState::FailedBeforeEffect,
            "reconciled_absent"
        )
    )
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
        (Some(rotation), None) => hash_value(&RotationHashMaterial {
            schema_version: entry.schema_version,
            sequence: entry.sequence,
            rotation,
            previous_hash: &entry.previous_hash,
        }),
        (None, Some(snapshot)) => hash_value(&RecordHashMaterial {
            schema_version: entry.schema_version,
            sequence: entry.sequence,
            snapshot,
            previous_hash: &entry.previous_hash,
        }),
        _ => Err(DurableActionError::Serialization),
    }
}

fn hash_value(value: &impl Serialize) -> Result<String, DurableActionError> {
    let encoded = serde_json::to_vec(value).map_err(|_| DurableActionError::Serialization)?;
    let digest = Sha256::digest(encoded);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn repair_unterminated_tail(path: &Path) -> Result<(), DurableActionError> {
    if !path.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|_| DurableActionError::Unavailable)?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let valid_length = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| DurableActionError::Unavailable)?;
    file.set_len(valid_length as u64)
        .and_then(|()| file.sync_data())
        .map_err(|_| DurableActionError::Unavailable)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(1)
        .max(1)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "neo-ai-agent-{label}-{}-{}.jsonl",
            std::process::id(),
            PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn authorizer(authority: EvidenceAuthority) -> ProbeAuthorizerMetadata {
        ProbeAuthorizerMetadata {
            evidence_id: "deterministic-rule-1".to_owned(),
            evidence_hash: "ab".repeat(32),
            authority,
            rule_version: "task-23-v1".to_owned(),
            authorized_at_unix_ms: 10,
        }
    }

    fn binding(id: &str) -> DurableActionBinding {
        DurableActionBinding::new(
            OperationId::new(id).unwrap(),
            OperationalAction::StartProbeRequest,
            None,
            authorizer(EvidenceAuthority::DeterministicRule),
            ActionResourceReservation {
                slots: 1,
                memory_bytes: 32_768,
                max_runtime_ms: 1_000,
            },
        )
        .unwrap()
    }

    fn rotation_options() -> DurableActionLogOptions {
        DurableActionLogOptions {
            rotate_max_bytes: u64::MAX,
            rotate_max_entries: 3,
            generations: 2,
        }
    }

    fn write_entries(path: &std::path::Path, entries: &[ActionJournalEntry]) {
        let mut buffer = Vec::new();
        for entry in entries {
            serde_json::to_writer(&mut buffer, entry).unwrap();
            buffer.push(b'\n');
        }
        std::fs::write(path, buffer).unwrap();
    }

    fn cleanup(path: &std::path::Path, generations: u32) {
        let _ = std::fs::remove_file(path);
        for generation in 1..=generations {
            let _ = std::fs::remove_file(generation_path(path, generation));
        }
    }

    #[test]
    fn same_operation_binding_is_idempotent_and_different_binding_is_rejected() {
        let path = path("idempotency");
        let log = DurableActionLog::open(&path).unwrap();
        let admitted = log.admit(binding("operation-1"), 20).unwrap();
        assert!(matches!(admitted, ActionAdmissionOutcome::Admitted(_)));
        let existing = log.admit(binding("operation-1"), 21).unwrap();
        assert!(matches!(existing, ActionAdmissionOutcome::Existing(_)));
        let mut changed = binding("operation-1");
        changed.reservation.memory_bytes += 1;
        assert_eq!(
            log.admit(changed, 22),
            Err(DurableActionError::BindingConflict)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn snapshot_serialization_is_strict_and_replay_rejects_impossible_first_state() {
        let action_binding = binding("serialization-operation");
        let snapshot = DurableActionSnapshot {
            binding_hash: action_binding.binding_hash().unwrap(),
            binding: action_binding,
            state: DurableActionState::AuthorizedReserved,
            updated_at_unix_ms: 20,
            outcome_code: "authorized_reserved".to_owned(),
        };
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(
            serde_json::from_str::<DurableActionSnapshot>(&encoded).unwrap(),
            snapshot
        );
        let mut with_extra = serde_json::to_value(&snapshot).unwrap();
        with_extra["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<DurableActionSnapshot>(with_extra).is_err());

        let path = path("invalid-replay-transition");
        let mut impossible = snapshot;
        impossible.state = DurableActionState::EffectInFlight;
        impossible.outcome_code = "effect_started".to_owned();
        let mut entry = ActionJournalEntry {
            schema_version: ACTION_LOG_SCHEMA_VERSION,
            sequence: 0,
            rotation: None,
            snapshot: Some(impossible),
            previous_hash: GENESIS_HASH.to_owned(),
            entry_hash: String::new(),
        };
        entry.entry_hash = calculate_entry_hash(&entry).unwrap();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        serde_json::to_writer(&mut file, &entry).unwrap();
        file.write_all(b"\n").unwrap();
        file.sync_data().unwrap();
        assert!(matches!(
            DurableActionLog::open(&path),
            Err(DurableActionError::BrokenJournal(0))
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn crash_reopen_marks_lost_completion_uncertain_without_retrying() {
        let path = path("crash-reopen");
        let operation_id = OperationId::new("operation-2").unwrap();
        {
            let log = DurableActionLog::open(&path).unwrap();
            log.admit(binding(operation_id.as_str()), 20).unwrap();
            log.transition(&operation_id, ActionTransition::PreAuditSucceeded, 21)
                .unwrap();
            log.transition(&operation_id, ActionTransition::EffectStarted, 22)
                .unwrap();
        }
        let reopened = DurableActionLog::open(&path).unwrap();
        let snapshot = reopened.query(&operation_id).unwrap().unwrap();
        assert_eq!(snapshot.state, DurableActionState::Uncertain);
        assert!(!snapshot.state.may_execute());
        assert_eq!(
            reopened.transition(&operation_id, ActionTransition::EffectStarted, 23),
            Err(DurableActionError::InvalidTransition {
                from: DurableActionState::Uncertain,
            })
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audit_failures_fail_closed_and_require_post_effect_reconciliation() {
        let path = path("audit-failures");
        let log = DurableActionLog::open(&path).unwrap();
        let pre_id = OperationId::new("pre-audit").unwrap();
        log.admit(binding(pre_id.as_str()), 20).unwrap();
        let failed = log
            .transition(&pre_id, ActionTransition::PreAuditFailed, 21)
            .unwrap();
        assert_eq!(failed.state, DurableActionState::FailedBeforeEffect);
        assert!(!failed.state.effect_may_have_occurred());
        assert!(matches!(
            log.transition(&pre_id, ActionTransition::EffectStarted, 22),
            Err(DurableActionError::InvalidTransition { .. })
        ));

        let post_id = OperationId::new("post-audit").unwrap();
        log.admit(binding(post_id.as_str()), 30).unwrap();
        log.transition(&post_id, ActionTransition::PreAuditSucceeded, 31)
            .unwrap();
        log.transition(&post_id, ActionTransition::EffectStarted, 32)
            .unwrap();
        log.transition(&post_id, ActionTransition::EffectCompleted, 33)
            .unwrap();
        let reconcile = log
            .transition(&post_id, ActionTransition::PostAuditFailed, 34)
            .unwrap();
        assert_eq!(reconcile.state, DurableActionState::ReconciliationRequired);
        assert!(reconcile.state.effect_may_have_occurred());
        let completed = log
            .reconcile(&post_id, ReconciliationDecision::ConfirmedAndAudited, 35)
            .unwrap();
        assert_eq!(completed.state, DurableActionState::Completed);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn model_authority_and_unterminated_tail_are_rejected_or_repaired() {
        let model_binding = DurableActionBinding::new(
            OperationId::new("model-operation").unwrap(),
            OperationalAction::StartProbeRequest,
            None,
            authorizer(EvidenceAuthority::Model),
            ActionResourceReservation {
                slots: 1,
                memory_bytes: 1,
                max_runtime_ms: 1,
            },
        );
        assert_eq!(model_binding, Err(DurableActionError::UntrustedAuthorizer));

        let path = path("tail-repair");
        let operation_id = OperationId::new("tail-operation").unwrap();
        {
            let log = DurableActionLog::open(&path).unwrap();
            log.admit(binding(operation_id.as_str()), 20).unwrap();
        }
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"partial\":").unwrap();
        file.sync_data().unwrap();
        let reopened = DurableActionLog::open(&path).unwrap();
        assert_eq!(
            reopened.query(&operation_id).unwrap().unwrap().state,
            DurableActionState::AuthorizedReserved
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rotation_keeps_chain_continuous_and_preserves_state_and_idempotency() {
        let path = path("rotate-continuity");
        let options = rotation_options();
        let first = OperationId::new("rotate-op-1").unwrap();
        {
            let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
            log.admit(binding(first.as_str()), 10).unwrap();
            for index in 0..5 {
                log.admit(binding(&format!("rotate-op-{index}")), 20 + index)
                    .unwrap();
            }
            // Drive one operation across a rotation boundary (and past pruning of
            // its admission entry) to a terminal, non-interrupted state.
            log.transition(&first, ActionTransition::PreAuditSucceeded, 30)
                .unwrap();
            log.transition(&first, ActionTransition::EffectStarted, 31)
                .unwrap();
            log.transition(&first, ActionTransition::EffectCompleted, 32)
                .unwrap();
            log.transition(&first, ActionTransition::PostAuditSucceeded, 33)
                .unwrap();
        }
        assert!(generation_path(&path, 1).exists());
        assert!(generation_path(&path, 2).exists());
        assert!(!generation_path(&path, 3).exists());
        // The active file opens with a checkpoint carrying the previous generation's tail.
        let active = read_entries(&path).unwrap();
        let previous = read_entries(&generation_path(&path, 1)).unwrap();
        let previous_tail = previous.last().unwrap().entry_hash.clone();
        assert_eq!(
            active[0].rotation.as_ref().unwrap().previous_tail_hash,
            previous_tail
        );
        assert_eq!(active[0].previous_hash, previous_tail);
        // Reopen verifies the cross-file chain and replays the full operations map;
        // the operation whose admission entry was pruned anchors on its first
        // retained transition entry.
        let reopened = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
        assert_eq!(
            reopened.query(&first).unwrap().unwrap().state,
            DurableActionState::Completed
        );
        // Idempotent admission is unaffected by rotation.
        assert!(matches!(
            reopened.admit(binding(first.as_str()), 40),
            Ok(ActionAdmissionOutcome::Existing(_))
        ));
        // The state machine keeps working after a rotation and a reopen. Note that
        // operations whose every entry was pruned by retention (e.g. "rotate-op-0")
        // are forgotten: their idempotency window is bounded by the retained files.
        let later = OperationId::new("rotate-op-3").unwrap();
        reopened
            .transition(&later, ActionTransition::PreAuditSucceeded, 41)
            .unwrap();
        assert_eq!(
            reopened.query(&later).unwrap().unwrap().state,
            DurableActionState::ReadyForEffect
        );
        drop(reopened);
        assert!(DurableActionLog::open_with_options(&path, options.clone()).is_ok());
        cleanup(&path, 2);
    }

    #[test]
    fn tampered_previous_generation_fails_closed_on_reopen() {
        let path = path("rotate-tamper");
        let options = rotation_options();
        {
            let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
            for index in 0..6 {
                log.admit(binding(&format!("tamper-op-{index}")), 10 + index)
                    .unwrap();
            }
        }
        // Corrupt one snapshot inside the previous generation: its own entry hash breaks.
        let archive = generation_path(&path, 2);
        let mut entries = read_entries(&archive).unwrap();
        let target = entries
            .iter_mut()
            .find_map(|entry| entry.snapshot.as_mut())
            .expect("the previous generation contains record entries");
        target.outcome_code = "corrupted".to_owned();
        write_entries(&archive, &entries);
        assert!(matches!(
            DurableActionLog::open_with_options(&path, options.clone()),
            Err(DurableActionError::BrokenJournal(_))
        ));
        cleanup(&path, 2);
    }

    #[test]
    fn truncated_previous_generation_fails_closed_on_reopen() {
        let path = path("rotate-link");
        let options = rotation_options();
        {
            let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
            for index in 0..6 {
                log.admit(binding(&format!("link-op-{index}")), 10 + index)
                    .unwrap();
            }
        }
        // Truncate the previous generation: its new tail no longer matches the
        // checkpoint that opens the next file.
        let archive = generation_path(&path, 2);
        let mut entries = read_entries(&archive).unwrap();
        entries.pop();
        write_entries(&archive, &entries);
        assert!(matches!(
            DurableActionLog::open_with_options(&path, options.clone()),
            Err(DurableActionError::BrokenJournal(_))
        ));
        cleanup(&path, 2);
    }

    #[test]
    fn interrupted_rotation_is_recovered_with_linking_checkpoint() {
        let path = path("rotate-recovery");
        let options = rotation_options();
        {
            let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
            for index in 0..6 {
                log.admit(binding(&format!("recovery-op-{index}")), 10 + index)
                    .unwrap();
            }
        }
        // Simulate a crash between the rename and the checkpoint write.
        std::fs::remove_file(&path).unwrap();
        let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
        let active = read_entries(&path).unwrap();
        let previous = read_entries(&generation_path(&path, 1)).unwrap();
        assert_eq!(
            active[0].rotation.as_ref().unwrap().previous_tail_hash,
            previous.last().unwrap().entry_hash
        );
        // Admissions continue the global sequence after the recovered checkpoint.
        assert!(matches!(
            log.admit(binding("recovery-new"), 50),
            Ok(ActionAdmissionOutcome::Admitted(_))
        ));
        let active_after = read_entries(&path).unwrap();
        assert_eq!(
            active_after.last().unwrap().sequence,
            active[0].sequence + 1
        );
        drop(log);
        // Reopen after recovery still verifies the cross-file chain.
        assert!(DurableActionLog::open_with_options(&path, options.clone()).is_ok());
        cleanup(&path, 2);
    }
}
