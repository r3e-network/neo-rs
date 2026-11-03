//! Durable append-only state for bounded diagnostic actions.

use neo_ai_agent::{OperationId, OperationalAction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const ACTION_STORE_SCHEMA_VERSION: u16 = 1;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionBinding {
    schema_version: u16,
    pub(crate) operation_id: OperationId,
    pub(crate) action: OperationalAction,
    pub(crate) target: Option<String>,
    pub(crate) evidence_ids: Vec<String>,
    pub(crate) evidence_hashes: Vec<String>,
    pub(crate) rule_version: String,
}

impl ActionBinding {
    pub(crate) fn new(
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
pub(crate) enum ActionState {
    Admitted,
    Running,
    Completed,
    Uncertain,
    ReconciliationRequired,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionRecord {
    pub(crate) binding: ActionBinding,
    pub(crate) binding_hash: String,
    pub(crate) state: ActionState,
    pub(crate) updated_at_unix_ms: u64,
    pub(crate) outcome_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    schema_version: u16,
    sequence: u64,
    record: ActionRecord,
    previous_hash: String,
    entry_hash: String,
}

#[derive(Debug)]
struct StoreState {
    file: File,
    entries: Vec<JournalEntry>,
    operations: BTreeMap<OperationId, ActionRecord>,
}

#[derive(Debug)]
pub(crate) struct DurableActionStore {
    state: Mutex<StoreState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdmitOutcome {
    Admitted(ActionRecord),
    Existing(ActionRecord),
}

impl DurableActionStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, ActionStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| ActionStoreError::Unavailable)?;
        }
        repair_unterminated_tail(path)?;
        let entries = read_entries(path)?;
        verify_journal(&entries)?;
        let operations = entries
            .iter()
            .map(|entry| {
                (
                    entry.record.binding.operation_id.clone(),
                    entry.record.clone(),
                )
            })
            .collect();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|_| ActionStoreError::Unavailable)?;
        let store = Self {
            state: Mutex::new(StoreState {
                file,
                entries,
                operations,
            }),
        };
        store.recover_running()?;
        Ok(store)
    }

    pub(crate) fn admit(
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

    pub(crate) fn mark_running(
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

    pub(crate) fn mark_completed(
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

    pub(crate) fn mark_uncertain(
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

    pub(crate) fn mark_reconciliation_required(
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

    pub(crate) fn reject(
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

    pub(crate) fn query(
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
        record.outcome_code = outcome_code.to_owned();
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

fn append(state: &mut StoreState, record: ActionRecord) -> Result<(), ActionStoreError> {
    let previous_hash = state
        .entries
        .last()
        .map_or_else(|| GENESIS_HASH.to_owned(), |entry| entry.entry_hash.clone());
    let mut entry = JournalEntry {
        schema_version: ACTION_STORE_SCHEMA_VERSION,
        sequence: state.entries.len() as u64,
        record,
        previous_hash,
        entry_hash: String::new(),
    };
    entry.entry_hash = entry_hash(&entry)?;
    let encoded = serde_json::to_vec(&entry).map_err(|_| ActionStoreError::Serialization)?;
    state
        .file
        .write_all(&encoded)
        .and_then(|()| state.file.write_all(b"\n"))
        .and_then(|()| state.file.sync_data())
        .map_err(|_| ActionStoreError::Unavailable)?;
    state.operations.insert(
        entry.record.binding.operation_id.clone(),
        entry.record.clone(),
    );
    state.entries.push(entry);
    Ok(())
}

fn read_entries(path: &Path) -> Result<Vec<JournalEntry>, ActionStoreError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    std::fs::read(path)
        .map_err(|_| ActionStoreError::Unavailable)?
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).map_err(|_| ActionStoreError::Serialization))
        .collect()
}

fn verify_journal(entries: &[JournalEntry]) -> Result<(), ActionStoreError> {
    let mut previous_hash = GENESIS_HASH;
    let mut operations = BTreeMap::<OperationId, ActionRecord>::new();
    for (expected_sequence, entry) in entries.iter().enumerate() {
        if entry.schema_version != ACTION_STORE_SCHEMA_VERSION {
            return Err(ActionStoreError::UnsupportedVersion(entry.schema_version));
        }
        entry.record.binding.validate()?;
        if entry.sequence != expected_sequence as u64
            || entry.previous_hash != previous_hash
            || entry.record.binding_hash != entry.record.binding.content_hash()?
            || entry.record.updated_at_unix_ms == 0
            || entry.record.outcome_code.is_empty()
            || entry.entry_hash != entry_hash(entry)?
        {
            return Err(ActionStoreError::BrokenJournal(entry.sequence));
        }
        let id = entry.record.binding.operation_id.clone();
        if let Some(previous) = operations.get(&id) {
            if previous.binding != entry.record.binding
                || previous.binding_hash != entry.record.binding_hash
                || previous.updated_at_unix_ms > entry.record.updated_at_unix_ms
                || !valid_transition(
                    previous.state,
                    entry.record.state,
                    &entry.record.outcome_code,
                )
            {
                return Err(ActionStoreError::BrokenJournal(entry.sequence));
            }
        } else if entry.record.state != ActionState::Admitted
            || entry.record.outcome_code != "admitted"
        {
            return Err(ActionStoreError::BrokenJournal(entry.sequence));
        }
        operations.insert(id, entry.record.clone());
        previous_hash = &entry.entry_hash;
    }
    Ok(())
}

fn valid_transition(from: ActionState, to: ActionState, outcome: &str) -> bool {
    matches!(
        (from, to, outcome),
        (ActionState::Admitted, ActionState::Running, "running")
            | (
                ActionState::Admitted | ActionState::Running,
                ActionState::Rejected,
                "pre_audit_failed" | "effect_rejected"
            )
            | (ActionState::Running, ActionState::Completed, "completed")
            | (
                ActionState::Running,
                ActionState::Uncertain,
                "completion_lost"
            )
            | (
                ActionState::Running,
                ActionState::ReconciliationRequired,
                "post_effect_audit_failed"
            )
    )
}

fn entry_hash(entry: &JournalEntry) -> Result<String, ActionStoreError> {
    #[derive(Serialize)]
    struct HashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        record: &'a ActionRecord,
        previous_hash: &'a str,
    }
    hash_value(&HashMaterial {
        schema_version: entry.schema_version,
        sequence: entry.sequence,
        record: &entry.record,
        previous_hash: &entry.previous_hash,
    })
}

fn hash_value(value: &impl Serialize) -> Result<String, ActionStoreError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ActionStoreError::Serialization)?;
    let digest = Sha256::digest(bytes);
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !valid_hash(&hash) {
        return Err(ActionStoreError::Serialization);
    }
    Ok(hash)
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_timestamp(now_unix_ms: u64) -> Result<(), ActionStoreError> {
    if now_unix_ms == 0 {
        Err(ActionStoreError::InvalidTimestamp)
    } else {
        Ok(())
    }
}

fn repair_unterminated_tail(path: &Path) -> Result<(), ActionStoreError> {
    if !path.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|_| ActionStoreError::Unavailable)?;
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
        .map_err(|_| ActionStoreError::Unavailable)?;
    file.set_len(valid_length as u64)
        .and_then(|()| file.sync_data())
        .map_err(|_| ActionStoreError::Unavailable)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(1)
        .max(1)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub(crate) enum ActionStoreError {
    #[error("unsupported action store schema version: {0}")]
    UnsupportedVersion(u16),
    #[error("action binding is invalid")]
    InvalidBinding,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "neo-node-action-{label}-{}-{}.jsonl",
            std::process::id(),
            PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn binding(id: &str) -> ActionBinding {
        ActionBinding::new(
            OperationId::new(id).unwrap(),
            OperationalAction::IncreaseSampling,
            None,
            vec!["deterministic-rule".to_owned()],
            vec!["ab".repeat(32)],
            "task-23-v1".to_owned(),
        )
        .unwrap()
    }

    #[test]
    fn state_serialization_covers_required_vocabulary() {
        for state in [
            ActionState::Admitted,
            ActionState::Running,
            ActionState::Completed,
            ActionState::Uncertain,
            ActionState::ReconciliationRequired,
            ActionState::Rejected,
        ] {
            let encoded = serde_json::to_string(&state).unwrap();
            assert_eq!(
                serde_json::from_str::<ActionState>(&encoded).unwrap(),
                state
            );
        }
    }

    #[test]
    fn same_id_same_binding_is_idempotent_and_changed_binding_is_rejected() {
        let path = path("idempotency");
        let store = DurableActionStore::open(&path).unwrap();
        assert!(matches!(
            store.admit(binding("operation-1"), 10),
            Ok(AdmitOutcome::Admitted(_))
        ));
        assert!(matches!(
            store.admit(binding("operation-1"), 11),
            Ok(AdmitOutcome::Existing(_))
        ));
        let mut changed = binding("operation-1");
        changed.rule_version = "task-23-v2".to_owned();
        assert_eq!(
            store.admit(changed, 12),
            Err(ActionStoreError::BindingConflict)
        );
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reopen_recovers_completed_state() {
        let path = path("reopen-completed");
        let id = OperationId::new("operation-2").unwrap();
        {
            let store = DurableActionStore::open(&path).unwrap();
            store.admit(binding(id.as_str()), 10).unwrap();
            store.mark_running(&id, 11).unwrap();
            store.mark_completed(&id, 12).unwrap();
        }
        let reopened = DurableActionStore::open(&path).unwrap();
        assert_eq!(
            reopened.query(&id).unwrap().unwrap().state,
            ActionState::Completed
        );
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reopen_converts_lost_running_completion_to_uncertain() {
        let path = path("lost-completion");
        let id = OperationId::new("operation-3").unwrap();
        {
            let store = DurableActionStore::open(&path).unwrap();
            store.admit(binding(id.as_str()), 10).unwrap();
            store.mark_running(&id, 11).unwrap();
        }
        let reopened = DurableActionStore::open(&path).unwrap();
        assert_eq!(
            reopened.query(&id).unwrap().unwrap().state,
            ActionState::Uncertain
        );
        assert!(matches!(
            reopened.mark_running(&id, 12),
            Err(ActionStoreError::InvalidTransition(ActionState::Uncertain))
        ));
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pre_audit_failure_rejects_without_effect() {
        let path = path("pre-audit");
        let id = OperationId::new("operation-4").unwrap();
        let effect = AtomicBool::new(false);
        let store = DurableActionStore::open(&path).unwrap();
        store.admit(binding(id.as_str()), 10).unwrap();
        store.reject(&id, "pre_audit_failed", 11).unwrap();
        assert!(!effect.load(Ordering::Acquire));
        assert_eq!(
            store.query(&id).unwrap().unwrap().state,
            ActionState::Rejected
        );
        assert!(store.mark_running(&id, 12).is_err());
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn post_effect_audit_failure_requires_reconciliation() {
        let path = path("post-audit");
        let id = OperationId::new("operation-5").unwrap();
        let effect = AtomicBool::new(false);
        let store = DurableActionStore::open(&path).unwrap();
        store.admit(binding(id.as_str()), 10).unwrap();
        store.mark_running(&id, 11).unwrap();
        effect.store(true, Ordering::Release);
        store.mark_reconciliation_required(&id, 12).unwrap();
        assert!(effect.load(Ordering::Acquire));
        assert_eq!(
            store.query(&id).unwrap().unwrap().state,
            ActionState::ReconciliationRequired
        );
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
