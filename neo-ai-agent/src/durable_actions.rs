use crate::OperationalAction;
use crate::evidence::EvidenceAuthority;
use crate::probe::{OperationId, ProbeAuthorizerMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const ACTION_LOG_SCHEMA_VERSION: u16 = 1;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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
    snapshot: DurableActionSnapshot,
    previous_hash: String,
    entry_hash: String,
}

#[derive(Debug)]
struct DurableActionLogState {
    file: File,
    entries: Vec<ActionJournalEntry>,
    operations: BTreeMap<OperationId, DurableActionSnapshot>,
}

/// Synchronous durable state machine. Callers place it behind the bounded admission API.
#[derive(Debug)]
pub struct DurableActionLog {
    state: Mutex<DurableActionLogState>,
}

impl DurableActionLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DurableActionError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| DurableActionError::Unavailable)?;
        }
        repair_unterminated_tail(path)?;
        let mut entries = Vec::new();
        if path.exists() {
            let bytes = std::fs::read(path).map_err(|_| DurableActionError::Unavailable)?;
            for line in bytes.split(|byte| *byte == b'\n') {
                if line.is_empty() {
                    continue;
                }
                entries.push(
                    serde_json::from_slice(line).map_err(|_| DurableActionError::Serialization)?,
                );
            }
        }
        verify_journal(&entries)?;
        let mut operations = BTreeMap::new();
        for entry in &entries {
            operations.insert(
                entry.snapshot.binding.operation_id.clone(),
                entry.snapshot.clone(),
            );
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|_| DurableActionError::Unavailable)?;
        let log = Self {
            state: Mutex::new(DurableActionLogState {
                file,
                entries,
                operations,
            }),
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
    let previous_hash = state
        .entries
        .last()
        .map_or_else(|| GENESIS_HASH.to_owned(), |entry| entry.entry_hash.clone());
    let mut entry = ActionJournalEntry {
        schema_version: ACTION_LOG_SCHEMA_VERSION,
        sequence: state.entries.len() as u64,
        snapshot,
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
    state.operations.insert(
        entry.snapshot.binding.operation_id.clone(),
        entry.snapshot.clone(),
    );
    state.entries.push(entry);
    Ok(())
}

fn verify_journal(entries: &[ActionJournalEntry]) -> Result<(), DurableActionError> {
    let mut previous_hash = GENESIS_HASH;
    let mut operations = BTreeMap::<OperationId, DurableActionSnapshot>::new();
    for (expected_sequence, entry) in entries.iter().enumerate() {
        if entry.schema_version != ACTION_LOG_SCHEMA_VERSION {
            return Err(DurableActionError::UnsupportedVersion(entry.schema_version));
        }
        entry.snapshot.binding.validate()?;
        if entry.sequence != expected_sequence as u64
            || entry.previous_hash != previous_hash
            || entry.snapshot.binding_hash != entry.snapshot.binding.binding_hash()?
            || entry.snapshot.updated_at_unix_ms == 0
            || entry.snapshot.outcome_code.is_empty()
            || calculate_entry_hash(entry)? != entry.entry_hash
        {
            return Err(DurableActionError::BrokenJournal(entry.sequence));
        }
        let operation_id = entry.snapshot.binding.operation_id.clone();
        if let Some(current) = operations.get(&operation_id) {
            if current.binding != entry.snapshot.binding
                || current.binding_hash != entry.snapshot.binding_hash
                || current.updated_at_unix_ms > entry.snapshot.updated_at_unix_ms
                || !valid_replayed_transition(
                    current.state,
                    entry.snapshot.state,
                    &entry.snapshot.outcome_code,
                )
            {
                return Err(DurableActionError::BrokenJournal(entry.sequence));
            }
        } else if entry.snapshot.state != DurableActionState::AuthorizedReserved
            || entry.snapshot.outcome_code != "authorized_reserved"
        {
            return Err(DurableActionError::BrokenJournal(entry.sequence));
        }
        operations.insert(operation_id, entry.snapshot.clone());
        previous_hash = &entry.entry_hash;
    }
    Ok(())
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
    #[derive(Serialize)]
    struct HashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        snapshot: &'a DurableActionSnapshot,
        previous_hash: &'a str,
    }
    hash_value(&HashMaterial {
        schema_version: entry.schema_version,
        sequence: entry.sequence,
        snapshot: &entry.snapshot,
        previous_hash: &entry.previous_hash,
    })
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
            snapshot: impossible,
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
}
