use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JournalEntry {
    pub(super) schema_version: u16,
    pub(super) sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) rotation: Option<RotationCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) record: Option<ActionRecord>,
    pub(super) previous_hash: String,
    pub(super) entry_hash: String,
}
/// Rotation checkpoint opening a fresh journal file. `previous_tail_hash` carries the
/// `entry_hash` of the final entry of the previous generation file and must mirror
/// `JournalEntry::previous_hash`, keeping the hash chain continuous across files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RotationCheckpoint {
    pub(super) previous_tail_hash: String,
}
pub(super) fn append(state: &mut StoreState, record: ActionRecord) -> Result<(), ActionStoreError> {
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
    state.tail_hash.clone_from(&entry.entry_hash);
    state.operations.insert(
        operation_id,
        entry.record.expect("record entries carry a record"),
    );
    Ok(())
}
/// Reads the JSONL entries of a single journal file (empty when it does not exist).
pub(super) fn read_entries(path: &Path) -> Result<Vec<JournalEntry>, ActionStoreError> {
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
pub(super) fn verify_entries(
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
        tail_hash.clone_from(&entry.entry_hash);
        *total_entries = entry.sequence + 1;
    }
    Ok(())
}
/// Rotates the active file to `<path>.1` and re-creates it with a linking checkpoint.
fn rotate(state: &mut StoreState) -> Result<(), ActionStoreError> {
    rotate_active_file(state)?;
    write_rotation_checkpoint(state)
}
impl RotatedActiveFile for StoreState {
    type Error = ActionStoreError;

    fn active_path(&self) -> &Path {
        &self.path
    }

    fn active_file(&mut self) -> &mut File {
        &mut self.file
    }

    fn retained_generations(&self) -> u32 {
        self.options.generations
    }

    fn reset_active_file(&mut self, file: File) {
        self.file = file;
        self.file_bytes = 0;
        self.file_entries = 0;
    }
}
/// Appends the checkpoint that links a fresh active file to the retained tail.
pub(super) fn write_rotation_checkpoint(state: &mut StoreState) -> Result<(), ActionStoreError> {
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
    state.tail_hash.clone_from(&entry.entry_hash);
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
