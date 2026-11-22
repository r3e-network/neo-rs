use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionJournalEntry {
    pub(super) schema_version: u16,
    pub(super) sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) rotation: Option<ActionRotationCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) snapshot: Option<DurableActionSnapshot>,
    pub(super) previous_hash: String,
    pub(super) entry_hash: String,
}
pub(super) fn append_snapshot(
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
    state.tail_hash.clone_from(&entry.entry_hash);
    state.operations.insert(
        operation_id,
        entry.snapshot.expect("record entries carry a snapshot"),
    );
    Ok(())
}
/// Reads the JSONL entries of a single journal file (empty when it does not exist).
pub(super) fn read_entries(path: &Path) -> Result<Vec<ActionJournalEntry>, DurableActionError> {
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
pub(super) fn verify_entries(
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
        tail_hash.clone_from(&entry.entry_hash);
        *total_entries = entry.sequence + 1;
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
