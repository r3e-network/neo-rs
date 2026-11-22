use super::journal::ActionJournalEntry;
use super::*;
/// Rotation checkpoint opening a fresh journal file. `previous_tail_hash` carries the
/// `entry_hash` of the final entry of the previous generation file and must mirror
/// `ActionJournalEntry::previous_hash`, keeping the hash chain continuous across files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionRotationCheckpoint {
    pub(super) previous_tail_hash: String,
}
/// Rotates the active file to `<path>.1` and re-creates it with a linking checkpoint.
pub(super) fn rotate(state: &mut DurableActionLogState) -> Result<(), DurableActionError> {
    rotate_active_file(state)?;
    write_rotation_checkpoint(state)
}
impl RotatedActiveFile for DurableActionLogState {
    type Error = DurableActionError;

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
pub(super) fn write_rotation_checkpoint(
    state: &mut DurableActionLogState,
) -> Result<(), DurableActionError> {
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
    state.tail_hash.clone_from(&entry.entry_hash);
    Ok(())
}
