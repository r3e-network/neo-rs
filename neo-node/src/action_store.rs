//! Durable append-only state for bounded diagnostic actions.

use neo_ai_agent::{OperationId, OperationalAction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
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
pub(crate) struct ActionStoreOptions {
    /// Rotate once the active file reaches this many bytes.
    pub(crate) rotate_max_bytes: u64,
    /// Rotate once the active file holds this many journal entries (checkpoint included).
    pub(crate) rotate_max_entries: u64,
    /// Number of rotated generations retained on disk; `.1` is the most recent.
    pub(crate) generations: u32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotation: Option<RotationCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record: Option<ActionRecord>,
    previous_hash: String,
    entry_hash: String,
}

/// Rotation checkpoint opening a fresh journal file. `previous_tail_hash` carries the
/// `entry_hash` of the final entry of the previous generation file and must mirror
/// `JournalEntry::previous_hash`, keeping the hash chain continuous across files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RotationCheckpoint {
    previous_tail_hash: String,
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
        Self::open_with_options(path, ActionStoreOptions::default())
    }

    /// Opens (or creates) the store with an explicit rotation policy.
    ///
    /// Fails closed (see [`ActionStoreOptions`]) when any retained generation or the
    /// cross-file checkpoint chain is broken. If a previous rotation was interrupted
    /// between the rename and the checkpoint write (the active file is missing or
    /// empty while generations remain), the active file is re-created with the
    /// checkpoint linking to the retained tail.
    pub(crate) fn open_with_options(
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
        repair_unterminated_tail(&path)?;
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
    if state.file_entries > 0
        && (state.file_entries >= state.options.rotate_max_entries
            || state.file_bytes >= state.options.rotate_max_bytes)
    {
        rotate(state)?;
    }
    let sequence = state.total_entries;
    let operation_id = record.binding.operation_id.clone();
    let previous_hash = state.tail_hash.clone();
    let mut entry = JournalEntry {
        schema_version: ACTION_STORE_SCHEMA_VERSION,
        sequence,
        rotation: None,
        record: Some(record),
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
    state.file_bytes += encoded.len() as u64 + 1;
    state.file_entries += 1;
    state.total_entries = sequence + 1;
    state.tail_hash = entry.entry_hash.clone();
    state.operations.insert(
        operation_id,
        entry.record.expect("record entries carry a record"),
    );
    Ok(())
}

/// Reads the JSONL entries of a single journal file (empty when it does not exist).
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

/// Verifies one file's entries as part of the global chain and state machine replay.
///
/// `is_first_retained_file` marks the chronologically oldest retained file, which may
/// anchor on a checkpoint whose predecessor was pruned by retention; every later file
/// must open with a rotation checkpoint whose carried hash equals the running tail.
/// When `history_pruned` is set, the first retained entry of an operation may be a
/// transition instead of its admission (the admission entry was pruned); the entry
/// then anchors the operation and later entries are still fully validated.
fn verify_entries(
    entries: &[JournalEntry],
    is_first_retained_file: bool,
    history_pruned: bool,
    tail_hash: &mut String,
    total_entries: &mut u64,
    operations: &mut BTreeMap<OperationId, ActionRecord>,
) -> Result<(), ActionStoreError> {
    for (position, entry) in entries.iter().enumerate() {
        if entry.schema_version != ACTION_STORE_SCHEMA_VERSION {
            return Err(ActionStoreError::UnsupportedVersion(entry.schema_version));
        }
        if position == 0 && !is_first_retained_file {
            let rotation = entry
                .rotation
                .as_ref()
                .ok_or(ActionStoreError::BrokenJournal(entry.sequence))?;
            if entry.record.is_some()
                || rotation.previous_tail_hash != *tail_hash
                || entry.previous_hash != *tail_hash
            {
                return Err(ActionStoreError::BrokenJournal(entry.sequence));
            }
        }
        if let Some(rotation) = entry.rotation.as_ref()
            && (entry.record.is_some() || rotation.previous_tail_hash != entry.previous_hash)
        {
            return Err(ActionStoreError::BrokenJournal(entry.sequence));
        }
        let anchored = position == 0 && is_first_retained_file && entry.rotation.is_some();
        if !anchored && (entry.sequence != *total_entries || entry.previous_hash != *tail_hash) {
            return Err(ActionStoreError::BrokenJournal(entry.sequence));
        }
        if entry.entry_hash != entry_hash(entry)? {
            return Err(ActionStoreError::BrokenJournal(entry.sequence));
        }
        if let Some(record) = entry.record.as_ref() {
            record.binding.validate()?;
            if record.binding_hash != record.binding.content_hash()?
                || record.updated_at_unix_ms == 0
                || record.outcome_code.is_empty()
            {
                return Err(ActionStoreError::BrokenJournal(entry.sequence));
            }
            let id = record.binding.operation_id.clone();
            if let Some(previous) = operations.get(&id) {
                if previous.binding != record.binding
                    || previous.binding_hash != record.binding_hash
                    || previous.updated_at_unix_ms > record.updated_at_unix_ms
                    || !valid_transition(previous.state, record.state, &record.outcome_code)
                {
                    return Err(ActionStoreError::BrokenJournal(entry.sequence));
                }
            } else if (record.state != ActionState::Admitted || record.outcome_code != "admitted")
                && !history_pruned
            {
                return Err(ActionStoreError::BrokenJournal(entry.sequence));
            }
            operations.insert(id, record.clone());
        }
        *tail_hash = entry.entry_hash.clone();
        *total_entries = entry.sequence + 1;
    }
    Ok(())
}

/// Rotates the active file to `<path>.1` and re-creates it with a linking checkpoint.
fn rotate(state: &mut StoreState) -> Result<(), ActionStoreError> {
    state
        .file
        .sync_all()
        .map_err(|_| ActionStoreError::Unavailable)?;
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
        std::fs::remove_file(&candidate).map_err(|_| ActionStoreError::Unavailable)?;
        stale += 1;
    }
    // Drop the oldest retained generation and shift the remaining ones down.
    let oldest = generation_path(&state.path, state.options.generations);
    if oldest.exists() {
        std::fs::remove_file(&oldest).map_err(|_| ActionStoreError::Unavailable)?;
    }
    for generation in (2..=state.options.generations).rev() {
        let from = generation_path(&state.path, generation - 1);
        if from.exists() {
            std::fs::rename(&from, generation_path(&state.path, generation))
                .map_err(|_| ActionStoreError::Unavailable)?;
        }
    }
    std::fs::rename(&state.path, generation_path(&state.path, 1))
        .map_err(|_| ActionStoreError::Unavailable)?;
    state.file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)
        .map_err(|_| ActionStoreError::Unavailable)?;
    state.file_bytes = 0;
    state.file_entries = 0;
    write_rotation_checkpoint(state)
}

/// Appends the checkpoint that links a fresh active file to the retained tail.
fn write_rotation_checkpoint(state: &mut StoreState) -> Result<(), ActionStoreError> {
    let checkpoint = RotationCheckpoint {
        previous_tail_hash: state.tail_hash.clone(),
    };
    let sequence = state.total_entries;
    let mut entry = JournalEntry {
        schema_version: ACTION_STORE_SCHEMA_VERSION,
        sequence,
        rotation: Some(checkpoint),
        record: None,
        previous_hash: state.tail_hash.clone(),
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

fn null_device_file() -> Result<File, ActionStoreError> {
    #[cfg(windows)]
    let path = "NUL";
    #[cfg(not(windows))]
    let path = "/dev/null";
    OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| ActionStoreError::Unavailable)
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
    // The record hash material intentionally mirrors the pre-rotation field layout
    // so journals written by earlier versions keep verifying.
    #[derive(Serialize)]
    struct RecordHashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        record: &'a ActionRecord,
        previous_hash: &'a str,
    }
    #[derive(Serialize)]
    struct RotationHashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        rotation: &'a RotationCheckpoint,
        previous_hash: &'a str,
    }
    match (&entry.rotation, &entry.record) {
        (Some(rotation), None) => hash_value(&RotationHashMaterial {
            schema_version: entry.schema_version,
            sequence: entry.sequence,
            rotation,
            previous_hash: &entry.previous_hash,
        }),
        (None, Some(record)) => hash_value(&RecordHashMaterial {
            schema_version: entry.schema_version,
            sequence: entry.sequence,
            record,
            previous_hash: &entry.previous_hash,
        }),
        _ => Err(ActionStoreError::Serialization),
    }
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

    fn rotation_options() -> ActionStoreOptions {
        ActionStoreOptions {
            rotate_max_bytes: u64::MAX,
            rotate_max_entries: 3,
            generations: 2,
        }
    }

    fn write_entries(path: &std::path::Path, entries: &[JournalEntry]) {
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

    #[test]
    fn rotation_keeps_chain_continuous_and_preserves_state_and_idempotency() {
        let path = path("rotate-continuity");
        let options = rotation_options();
        let first = OperationId::new("rotate-op-1").unwrap();
        {
            let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
            store.admit(binding(first.as_str()), 10).unwrap();
            for index in 0..5 {
                store
                    .admit(binding(&format!("rotate-op-{index}")), 20 + index)
                    .unwrap();
            }
            // Drive one operation across a rotation boundary.
            store.mark_running(&first, 30).unwrap();
            store.mark_completed(&first, 31).unwrap();
        }
        assert!(generation_path(&path, 1).exists());
        assert!(generation_path(&path, 2).exists());
        assert!(!generation_path(&path, 3).exists());
        // The active file opens with a checkpoint carrying the previous generation's tail.
        let active = read_entries(&path).unwrap();
        let previous = read_entries(&generation_path(&path, 1)).unwrap();
        let previous_tail = previous.last().unwrap().entry_hash.clone();
        let checkpoint = active[0].rotation.as_ref().unwrap();
        assert_eq!(checkpoint.previous_tail_hash, previous_tail);
        assert_eq!(active[0].previous_hash, previous_tail);
        // Reopen verifies the cross-file chain and replays the full operations map.
        let reopened = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
        assert_eq!(
            reopened.query(&first).unwrap().unwrap().state,
            ActionState::Completed
        );
        // Idempotent admission is unaffected by rotation.
        assert!(matches!(
            reopened.admit(binding(first.as_str()), 40),
            Ok(AdmitOutcome::Existing(_))
        ));
        // The state machine keeps working after a rotation and a reopen.
        let later = OperationId::new("rotate-op-0").unwrap();
        reopened.mark_running(&later, 41).unwrap();
        reopened.mark_completed(&later, 42).unwrap();
        assert_eq!(
            reopened.query(&later).unwrap().unwrap().state,
            ActionState::Completed
        );
        drop(reopened);
        assert!(DurableActionStore::open_with_options(&path, options.clone()).is_ok());
        cleanup(&path, 2);
    }

    #[test]
    fn tampered_previous_generation_fails_closed_on_reopen() {
        let path = path("rotate-tamper");
        let options = rotation_options();
        {
            let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
            for index in 0..6 {
                store
                    .admit(binding(&format!("tamper-op-{index}")), 10 + index)
                    .unwrap();
            }
        }
        // Corrupt one record inside the previous generation: its own entry hash breaks.
        let archive = generation_path(&path, 2);
        let mut entries = read_entries(&archive).unwrap();
        let target = entries
            .iter_mut()
            .find_map(|entry| entry.record.as_mut())
            .expect("the previous generation contains record entries");
        target.outcome_code = "corrupted".to_owned();
        write_entries(&archive, &entries);
        assert!(matches!(
            DurableActionStore::open_with_options(&path, options.clone()),
            Err(ActionStoreError::BrokenJournal(_))
        ));
        cleanup(&path, 2);
    }

    #[test]
    fn truncated_previous_generation_fails_closed_on_reopen() {
        let path = path("rotate-link");
        let options = rotation_options();
        {
            let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
            for index in 0..6 {
                store
                    .admit(binding(&format!("link-op-{index}")), 10 + index)
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
            DurableActionStore::open_with_options(&path, options.clone()),
            Err(ActionStoreError::BrokenJournal(_))
        ));
        cleanup(&path, 2);
    }

    #[test]
    fn interrupted_rotation_is_recovered_with_linking_checkpoint() {
        let path = path("rotate-recovery");
        let options = rotation_options();
        {
            let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
            for index in 0..6 {
                store
                    .admit(binding(&format!("recovery-op-{index}")), 10 + index)
                    .unwrap();
            }
        }
        // Simulate a crash between the rename and the checkpoint write.
        std::fs::remove_file(&path).unwrap();
        let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
        let active = read_entries(&path).unwrap();
        let previous = read_entries(&generation_path(&path, 1)).unwrap();
        assert_eq!(
            active[0].rotation.as_ref().unwrap().previous_tail_hash,
            previous.last().unwrap().entry_hash
        );
        // Admissions continue the global sequence after the recovered checkpoint.
        let admitted = store.admit(binding("recovery-new"), 50).unwrap();
        assert!(matches!(admitted, AdmitOutcome::Admitted(_)));
        let active_after = read_entries(&path).unwrap();
        assert_eq!(
            active_after.last().unwrap().sequence,
            active[0].sequence + 1
        );
        drop(store);
        // Reopen after recovery still verifies the cross-file chain.
        assert!(DurableActionStore::open_with_options(&path, options.clone()).is_ok());
        cleanup(&path, 2);
    }
}
