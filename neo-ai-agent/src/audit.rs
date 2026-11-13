use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const AUDIT_SCHEMA_VERSION: u16 = 1;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    DetectionCompleted,
    ModelRequested,
    DiagnosisAccepted,
    DiagnosisRejected,
    ProviderFailed,
    /// A fixed operational action was evaluated but not executed.
    OperationalActionEvaluated,
    /// A fixed operational action was executed by the node runtime.
    OperationalActionExecuted,
    /// Rotation checkpoint written as the first event of a fresh journal file after
    /// rotation. Its `previous_hash` carries the `event_hash` of the final event of
    /// the previous generation file, keeping the hash chain continuous across files.
    RotationCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditInput {
    pub timestamp_unix_ms: u64,
    pub kind: AuditEventKind,
    pub observed_height: u32,
    pub evidence_refs: Vec<String>,
    pub evidence_hashes: Vec<String>,
    pub action: Option<String>,
    pub target_hash: Option<String>,
    pub rule_version: Option<String>,
    pub outcome_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub schema_version: u16,
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub kind: AuditEventKind,
    pub observed_height: u32,
    pub evidence_refs: Vec<String>,
    pub evidence_hashes: Vec<String>,
    pub action: Option<String>,
    pub target_hash: Option<String>,
    pub rule_version: Option<String>,
    pub outcome_code: String,
    pub previous_hash: String,
    pub event_hash: String,
}

#[async_trait]
pub trait AuditLog: Send + Sync {
    async fn append(&self, input: AuditInput) -> Result<AuditEvent, AuditError>;
    /// Returns recent audit events. Implementations may bound the returned history;
    /// see [`FileAuditLog`] for the windowed semantics of the file-backed log.
    async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError>;
}

#[derive(Debug, Default)]
pub struct MemoryAuditLog {
    events: Mutex<Vec<AuditEvent>>,
}

#[async_trait]
impl AuditLog for MemoryAuditLog {
    async fn append(&self, input: AuditInput) -> Result<AuditEvent, AuditError> {
        validate_input(&input)?;
        let mut events = self.events.lock().map_err(|_| AuditError::Unavailable)?;
        let sequence = events.len() as u64;
        let previous_hash = events
            .last()
            .map_or_else(|| GENESIS_HASH.to_owned(), |event| event.event_hash.clone());
        let mut event = AuditEvent {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence,
            timestamp_unix_ms: input.timestamp_unix_ms,
            kind: input.kind,
            observed_height: input.observed_height,
            evidence_refs: input.evidence_refs,
            evidence_hashes: input.evidence_hashes,
            action: input.action,
            target_hash: input.target_hash,
            rule_version: input.rule_version,
            outcome_code: input.outcome_code,
            previous_hash,
            event_hash: String::new(),
        };
        event.event_hash = calculate_hash(&event)?;
        events.push(event.clone());
        Ok(event)
    }

    async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError> {
        self.events
            .lock()
            .map(|events| events.clone())
            .map_err(|_| AuditError::Unavailable)
    }
}

/// Default rotation threshold in bytes for the active journal file (16 MiB).
pub const AUDIT_ROTATE_DEFAULT_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Default rotation threshold in events for the active journal file.
pub const AUDIT_ROTATE_DEFAULT_MAX_EVENTS: u64 = 100_000;
/// Default number of rotated generations retained on disk (`.1` is the most recent).
pub const AUDIT_ROTATE_DEFAULT_GENERATIONS: u32 = 2;
/// Default number of most recent events retained in memory for [`AuditLog::snapshot`].
pub const AUDIT_MEMORY_WINDOW_DEFAULT: usize = 10_000;

/// Rotation and memory-window policy for [`FileAuditLog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAuditLogOptions {
    /// Rotate once the active file reaches this many bytes.
    pub rotate_max_bytes: u64,
    /// Rotate once the active file holds this many events (checkpoint included).
    pub rotate_max_events: u64,
    /// Number of rotated generations retained on disk; `.1` is the most recent.
    pub generations: u32,
    /// Number of most recent events kept in memory and returned by `snapshot()`.
    pub memory_window: usize,
}

impl Default for FileAuditLogOptions {
    fn default() -> Self {
        Self {
            rotate_max_bytes: AUDIT_ROTATE_DEFAULT_MAX_BYTES,
            rotate_max_events: AUDIT_ROTATE_DEFAULT_MAX_EVENTS,
            generations: AUDIT_ROTATE_DEFAULT_GENERATIONS,
            memory_window: AUDIT_MEMORY_WINDOW_DEFAULT,
        }
    }
}

/// Hash-chained JSONL audit log that synchronizes each append before success.
///
/// # Rotation, checkpoints, and bounded memory
///
/// The journal rotates once the active file reaches the configured byte or event
/// threshold (16 MiB or 100_000 events by default). On rotation the active file is
/// renamed to `<path>.1` — up to `generations` (default 2) rotated copies are
/// retained, where `.1` is the most recent — and the new active file begins with a
/// *rotation checkpoint* event ([`AuditEventKind::RotationCheckpoint`]). The
/// checkpoint's `previous_hash` carries the `event_hash` of the final event of the
/// previous generation file, keeping the SHA-256 hash chain continuous across files.
///
/// Opening the log verifies every retained generation in chronological order and
/// fails closed on corruption: a tampered or truncated generation, or a checkpoint
/// whose carried hash does not match the previous generation's tail, is rejected with
/// [`AuditError::BrokenChain`] (malformed lines fail with [`AuditError::Serialization`]).
/// Only the chronologically oldest retained file may anchor on an unverifiable
/// checkpoint when older generations have already been pruned by retention.
///
/// Memory is bounded: opening keeps only the `memory_window` (default 10_000) most
/// recent events plus the chain tail hash and the global event count, so a
/// long-running node no longer holds the full history in memory.
#[derive(Debug)]
pub struct FileAuditLog {
    state: Mutex<FileAuditState>,
}

#[derive(Debug)]
struct FileAuditState {
    path: PathBuf,
    file: File,
    options: FileAuditLogOptions,
    /// Size of the active journal file in bytes.
    file_bytes: u64,
    /// Events in the active file, including its leading rotation checkpoint.
    file_events: u64,
    /// Global event count across all retained generations (checkpoints included).
    total_events: u64,
    /// `event_hash` of the most recent event, or the genesis hash when empty.
    tail_hash: String,
    /// Bounded window of the most recent events retained in memory.
    window: VecDeque<AuditEvent>,
}

impl FileAuditLog {
    /// Opens (or creates) the audit log with the default rotation policy.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        Self::open_with_options(path, FileAuditLogOptions::default())
    }

    /// Opens (or creates) the audit log with an explicit rotation policy.
    ///
    /// Fails closed (see the type-level documentation) when any retained generation
    /// or the cross-file checkpoint chain is broken. If a previous rotation was
    /// interrupted between the rename and the checkpoint write (the active file is
    /// missing or empty while generations remain), the active file is re-created with
    /// the checkpoint linking to the retained tail.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: FileAuditLogOptions,
    ) -> Result<Self, AuditError> {
        if options.generations == 0
            || options.memory_window == 0
            || options.rotate_max_bytes == 0
            || options.rotate_max_events == 0
        {
            return Err(AuditError::InvalidOptions);
        }
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| AuditError::Unavailable)?;
        }
        // Chronological order: oldest retained generation first, active file last.
        let mut files: Vec<PathBuf> = discover_generations(&path).into_iter().rev().collect();
        let active_bytes = if path.exists() {
            std::fs::metadata(&path)
                .map_err(|_| AuditError::Unavailable)?
                .len()
        } else {
            0
        };
        if active_bytes > 0 {
            files.push(path.clone());
        }
        let mut window: VecDeque<AuditEvent> = VecDeque::new();
        let mut tail_hash = GENESIS_HASH.to_owned();
        let mut total_events = 0u64;
        let mut active_file_events = 0u64;
        for (index, file_path) in files.iter().enumerate() {
            let events = read_events(file_path)?;
            if events.is_empty() {
                // Retained generation files are never empty: they were fully synced
                // before being rotated. An empty file indicates corruption.
                return Err(AuditError::BrokenChain(total_events));
            }
            if index + 1 == files.len() && active_bytes > 0 {
                active_file_events = events.len() as u64;
            }
            for (position, event) in events.iter().enumerate() {
                if event.schema_version != AUDIT_SCHEMA_VERSION {
                    return Err(AuditError::UnsupportedVersion(event.schema_version));
                }
                if position == 0 && index > 0 && event.kind != AuditEventKind::RotationCheckpoint {
                    // Chain discontinuity: a rotated file must open with a checkpoint.
                    return Err(AuditError::BrokenChain(event.sequence));
                }
                // The oldest retained file may anchor on a checkpoint whose
                // predecessor was pruned by retention; its link cannot be re-verified.
                let anchored =
                    position == 0 && index == 0 && event.kind == AuditEventKind::RotationCheckpoint;
                if !anchored && (event.sequence != total_events || event.previous_hash != tail_hash)
                {
                    return Err(AuditError::BrokenChain(event.sequence));
                }
                if calculate_hash(event)? != event.event_hash {
                    return Err(AuditError::BrokenChain(event.sequence));
                }
                tail_hash.clone_from(&event.event_hash);
                total_events = event.sequence + 1;
                window.push_back(event.clone());
                if window.len() > options.memory_window {
                    window.pop_front();
                }
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|_| AuditError::Unavailable)?;
        let mut state = FileAuditState {
            path,
            file,
            options,
            file_bytes: active_bytes,
            file_events: active_file_events,
            total_events,
            tail_hash,
            window,
        };
        if active_bytes == 0 && !files.is_empty() {
            // A rotation was interrupted between the rename and the checkpoint
            // write: re-create the active file with the linking checkpoint.
            write_rotation_checkpoint(&mut state)?;
        }
        Ok(Self {
            state: Mutex::new(state),
        })
    }
}

#[async_trait]
impl AuditLog for FileAuditLog {
    async fn append(&self, input: AuditInput) -> Result<AuditEvent, AuditError> {
        validate_input(&input)?;
        let mut state = self.state.lock().map_err(|_| AuditError::Unavailable)?;
        if state.file_events > 0
            && (state.file_events >= state.options.rotate_max_events
                || state.file_bytes >= state.options.rotate_max_bytes)
        {
            rotate(&mut state)?;
        }
        let sequence = state.total_events;
        let previous_hash = state.tail_hash.clone();
        let mut event = AuditEvent {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence,
            timestamp_unix_ms: input.timestamp_unix_ms,
            kind: input.kind,
            observed_height: input.observed_height,
            evidence_refs: input.evidence_refs,
            evidence_hashes: input.evidence_hashes,
            action: input.action,
            target_hash: input.target_hash,
            rule_version: input.rule_version,
            outcome_code: input.outcome_code,
            previous_hash,
            event_hash: String::new(),
        };
        event.event_hash = calculate_hash(&event)?;
        let encoded = serde_json::to_vec(&event).map_err(|_| AuditError::Serialization)?;
        state
            .file
            .write_all(&encoded)
            .and_then(|()| state.file.write_all(b"\n"))
            .and_then(|()| state.file.sync_data())
            .map_err(|_| AuditError::Unavailable)?;
        state.file_bytes += encoded.len() as u64 + 1;
        state.file_events += 1;
        state.total_events = sequence + 1;
        state.tail_hash.clone_from(&event.event_hash);
        state.window.push_back(event.clone());
        if state.window.len() > state.options.memory_window {
            state.window.pop_front();
        }
        Ok(event)
    }

    /// Returns the bounded in-memory window of the journal.
    ///
    /// API semantics (changed with rotation support): at most `memory_window`
    /// (default 10_000) most recent events are returned — including the leading
    /// rotation checkpoint of the active file — instead of the full history. The
    /// window is a *suffix* of the global hash chain, so its first element generally
    /// does not start at sequence 0; older events remain on disk and are verified
    /// when the log is opened, but are no longer exposed through this API.
    async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError> {
        self.state
            .lock()
            .map(|state| state.window.iter().cloned().collect())
            .map_err(|_| AuditError::Unavailable)
    }
}

/// Rotates the active file to `<path>.1` and re-creates it with a linking checkpoint.
fn rotate(state: &mut FileAuditState) -> Result<(), AuditError> {
    state.file.sync_all().map_err(|_| AuditError::Unavailable)?;
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
        std::fs::remove_file(&candidate).map_err(|_| AuditError::Unavailable)?;
        stale += 1;
    }
    // Drop the oldest retained generation and shift the remaining ones down.
    let oldest = generation_path(&state.path, state.options.generations);
    if oldest.exists() {
        std::fs::remove_file(&oldest).map_err(|_| AuditError::Unavailable)?;
    }
    for generation in (2..=state.options.generations).rev() {
        let from = generation_path(&state.path, generation - 1);
        if from.exists() {
            std::fs::rename(&from, generation_path(&state.path, generation))
                .map_err(|_| AuditError::Unavailable)?;
        }
    }
    std::fs::rename(&state.path, generation_path(&state.path, 1))
        .map_err(|_| AuditError::Unavailable)?;
    state.file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)
        .map_err(|_| AuditError::Unavailable)?;
    state.file_bytes = 0;
    state.file_events = 0;
    write_rotation_checkpoint(state)
}

/// Appends the checkpoint that links a fresh active file to the retained tail.
///
/// Checkpoints are internally generated (they do not pass input validation) and are
/// regular chain events: they consume a sequence number and their `previous_hash`
/// carries — hash-protected — the previous generation's final `event_hash`.
fn write_rotation_checkpoint(state: &mut FileAuditState) -> Result<(), AuditError> {
    let mut event = AuditEvent {
        schema_version: AUDIT_SCHEMA_VERSION,
        sequence: state.total_events,
        timestamp_unix_ms: unix_time_ms(),
        kind: AuditEventKind::RotationCheckpoint,
        observed_height: 0,
        evidence_refs: Vec::new(),
        evidence_hashes: Vec::new(),
        action: None,
        target_hash: None,
        rule_version: None,
        outcome_code: "rotation_checkpoint".to_owned(),
        previous_hash: state.tail_hash.clone(),
        event_hash: String::new(),
    };
    event.event_hash = calculate_hash(&event)?;
    let encoded = serde_json::to_vec(&event).map_err(|_| AuditError::Serialization)?;
    state
        .file
        .write_all(&encoded)
        .and_then(|()| state.file.write_all(b"\n"))
        .and_then(|()| state.file.sync_data())
        .map_err(|_| AuditError::Unavailable)?;
    state.file_bytes = encoded.len() as u64 + 1;
    state.file_events = 1;
    state.total_events = event.sequence + 1;
    state.tail_hash.clone_from(&event.event_hash);
    state.window.push_back(event);
    if state.window.len() > state.options.memory_window {
        state.window.pop_front();
    }
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

fn read_events(path: &Path) -> Result<Vec<AuditEvent>, AuditError> {
    let reader = BufReader::new(File::open(path).map_err(|_| AuditError::Unavailable)?);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|_| AuditError::Unavailable)?;
        if !line.trim().is_empty() {
            events.push(serde_json::from_str(&line).map_err(|_| AuditError::Serialization)?);
        }
    }
    Ok(events)
}

fn null_device_file() -> Result<File, AuditError> {
    #[cfg(windows)]
    let path = "NUL";
    #[cfg(not(windows))]
    let path = "/dev/null";
    OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| AuditError::Unavailable)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
        .max(1)
}

#[async_trait]
impl<T: AuditLog + ?Sized> AuditLog for std::sync::Arc<T> {
    async fn append(&self, input: AuditInput) -> Result<AuditEvent, AuditError> {
        (**self).append(input).await
    }

    async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError> {
        (**self).snapshot().await
    }
}

pub fn verify_audit_chain(events: &[AuditEvent]) -> Result<(), AuditError> {
    let mut previous_hash = GENESIS_HASH;
    for (expected_sequence, event) in events.iter().enumerate() {
        if event.schema_version != AUDIT_SCHEMA_VERSION {
            return Err(AuditError::UnsupportedVersion(event.schema_version));
        }
        if event.sequence != expected_sequence as u64 || event.previous_hash != previous_hash {
            return Err(AuditError::BrokenChain(event.sequence));
        }
        if calculate_hash(event)? != event.event_hash {
            return Err(AuditError::BrokenChain(event.sequence));
        }
        previous_hash = &event.event_hash;
    }
    Ok(())
}

fn validate_input(input: &AuditInput) -> Result<(), AuditError> {
    if input.outcome_code.is_empty()
        || input.outcome_code.len() > 128
        || !input
            .outcome_code
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        || input
            .evidence_refs
            .iter()
            .any(std::string::String::is_empty)
        || input.evidence_refs.len() != input.evidence_hashes.len()
        || input
            .evidence_hashes
            .iter()
            .any(|hash| !valid_evidence_hash(hash))
        || input
            .action
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
        || input
            .target_hash
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
        || input
            .rule_version
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
    {
        return Err(AuditError::InvalidInput);
    }
    Ok(())
}

fn valid_evidence_hash(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn calculate_hash(event: &AuditEvent) -> Result<String, AuditError> {
    #[derive(Serialize)]
    struct HashMaterial<'a> {
        schema_version: u16,
        sequence: u64,
        timestamp_unix_ms: u64,
        kind: AuditEventKind,
        observed_height: u32,
        evidence_refs: &'a [String],
        evidence_hashes: &'a [String],
        action: &'a Option<String>,
        target_hash: &'a Option<String>,
        rule_version: &'a Option<String>,
        outcome_code: &'a str,
        previous_hash: &'a str,
    }

    let material = HashMaterial {
        schema_version: event.schema_version,
        sequence: event.sequence,
        timestamp_unix_ms: event.timestamp_unix_ms,
        kind: event.kind,
        observed_height: event.observed_height,
        evidence_refs: &event.evidence_refs,
        evidence_hashes: &event.evidence_hashes,
        action: &event.action,
        target_hash: &event.target_hash,
        rule_version: &event.rule_version,
        outcome_code: &event.outcome_code,
        previous_hash: &event.previous_hash,
    };
    let encoded = serde_json::to_vec(&material).map_err(|_| AuditError::Serialization)?;
    let digest = Sha256::digest(encoded);
    Ok(digest.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("formatting hex into a String cannot fail");
        output
    }))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuditError {
    #[error("audit input is invalid")]
    InvalidInput,
    #[error("audit log options are invalid")]
    InvalidOptions,
    #[error("audit log is unavailable")]
    Unavailable,
    #[error("audit event serialization failed")]
    Serialization,
    #[error("unsupported audit schema version: {0}")]
    UnsupportedVersion(u16),
    #[error("audit hash chain is broken at sequence {0}")]
    BrokenChain(u64),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "neo-ai-agent-audit-{label}-{}-{}.jsonl",
            std::process::id(),
            PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn input(sequence: u64) -> AuditInput {
        AuditInput {
            timestamp_unix_ms: sequence + 1,
            kind: AuditEventKind::DetectionCompleted,
            observed_height: 10,
            evidence_refs: vec!["detector".to_owned()],
            evidence_hashes: vec!["ab".repeat(32)],
            action: None,
            target_hash: None,
            rule_version: None,
            outcome_code: format!("stalled-{sequence}"),
        }
    }

    fn rotation_options() -> FileAuditLogOptions {
        FileAuditLogOptions {
            rotate_max_bytes: u64::MAX,
            rotate_max_events: 3,
            generations: 2,
            memory_window: 100,
        }
    }

    /// Verifies every retained file in chronological order: global sequences stay
    /// contiguous, every file after the oldest opens with a rotation checkpoint whose
    /// carried hash equals the previous file's tail, and every event's self-hash holds.
    /// Returns the global event count and the overall tail hash.
    fn verify_retained_files(path: &Path, generations: u32) -> (u64, String) {
        let mut files: Vec<PathBuf> = (1..=generations)
            .rev()
            .map(|generation| generation_path(path, generation))
            .filter(|file| file.exists())
            .collect();
        files.push(path.to_path_buf());
        assert!(files.iter().all(|file| file.exists()));
        let mut previous_tail = GENESIS_HASH.to_owned();
        let mut expected_sequence = 0u64;
        for (file_index, file) in files.iter().enumerate() {
            let events = read_events(file).unwrap();
            assert!(!events.is_empty(), "retained files must not be empty");
            for (position, event) in events.iter().enumerate() {
                if file_index > 0 && position == 0 {
                    assert_eq!(event.kind, AuditEventKind::RotationCheckpoint);
                    assert_eq!(event.previous_hash, previous_tail);
                    assert_eq!(
                        event.sequence, expected_sequence,
                        "sequence must stay global across files"
                    );
                }
                let anchored = file_index == 0
                    && position == 0
                    && event.kind == AuditEventKind::RotationCheckpoint;
                if !anchored {
                    assert_eq!(event.sequence, expected_sequence);
                    assert_eq!(event.previous_hash, previous_tail);
                }
                assert_eq!(calculate_hash(event).unwrap(), event.event_hash);
                expected_sequence = event.sequence + 1;
                previous_tail = event.event_hash.clone();
            }
        }
        (expected_sequence, previous_tail)
    }

    fn cleanup(path: &Path, generations: u32) {
        let _ = std::fs::remove_file(path);
        for generation in 1..=generations {
            let _ = std::fs::remove_file(generation_path(path, generation));
        }
    }

    #[tokio::test]
    async fn detects_tampering_in_append_only_chain() {
        let log = MemoryAuditLog::default();
        for sequence in 0..2 {
            log.append(AuditInput {
                timestamp_unix_ms: sequence,
                kind: AuditEventKind::DetectionCompleted,
                observed_height: 10,
                evidence_refs: vec!["detector".to_owned()],
                evidence_hashes: vec!["ab".repeat(32)],
                action: None,
                target_hash: None,
                rule_version: None,
                outcome_code: "stalled".to_owned(),
            })
            .await
            .unwrap();
        }
        let mut events = log.snapshot().await.unwrap();
        verify_audit_chain(&events).unwrap();
        events[0].outcome_code = "healthy".to_owned();
        assert_eq!(verify_audit_chain(&events), Err(AuditError::BrokenChain(0)));
    }

    #[tokio::test]
    async fn rejects_zero_and_non_hex_evidence_hashes() {
        let log = MemoryAuditLog::default();
        for invalid_hash in ["00".repeat(32), "zz".repeat(32)] {
            let result = log
                .append(AuditInput {
                    timestamp_unix_ms: 1,
                    kind: AuditEventKind::DetectionCompleted,
                    observed_height: 10,
                    evidence_refs: vec!["detector".to_owned()],
                    evidence_hashes: vec![invalid_hash],
                    action: None,
                    target_hash: None,
                    rule_version: None,
                    outcome_code: "stalled".to_owned(),
                })
                .await;
            assert_eq!(result, Err(AuditError::InvalidInput));
        }
    }

    #[tokio::test]
    async fn file_log_rotates_and_keeps_the_hash_chain_continuous_across_files() {
        let path = path("rotate-chain");
        let options = rotation_options();
        {
            let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
            for sequence in 0..12 {
                log.append(input(sequence)).await.unwrap();
            }
        }
        assert!(path.exists());
        assert!(generation_path(&path, 1).exists());
        assert!(generation_path(&path, 2).exists());
        assert!(!generation_path(&path, 3).exists());
        let (total, tail) = verify_retained_files(&path, 2);
        // Reopen: verification across all retained generations passes and appends
        // continue the global chain exactly where it left off.
        let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
        let event = log.append(input(99)).await.unwrap();
        assert_eq!(event.sequence, total);
        assert_eq!(event.previous_hash, tail);
        drop(log);
        let (total_after, _) = verify_retained_files(&path, 2);
        assert_eq!(total_after, total + 1);
        cleanup(&path, 2);
    }

    #[tokio::test]
    async fn file_log_rotates_on_the_byte_threshold() {
        let path = path("rotate-bytes");
        let options = FileAuditLogOptions {
            rotate_max_bytes: 1,
            rotate_max_events: u64::MAX,
            generations: 2,
            memory_window: 100,
        };
        {
            let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
            for sequence in 0..3 {
                log.append(input(sequence)).await.unwrap();
            }
        }
        assert!(generation_path(&path, 1).exists());
        assert!(generation_path(&path, 2).exists());
        verify_retained_files(&path, 2);
        // Reopen still verifies the byte-threshold rotation layout.
        assert!(FileAuditLog::open_with_options(&path, options.clone()).is_ok());
        cleanup(&path, 2);
    }

    #[tokio::test]
    async fn file_log_fails_closed_when_a_retained_generation_is_tampered() {
        let path = path("rotate-tamper");
        let options = rotation_options();
        {
            let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
            for sequence in 0..12 {
                log.append(input(sequence)).await.unwrap();
            }
        }
        // Corrupt an event inside the previous generation: its own hash breaks.
        let archive = generation_path(&path, 2);
        let mut events = read_events(&archive).unwrap();
        events[0].outcome_code = "healthy".to_owned();
        let mut buffer = Vec::new();
        for event in &events {
            serde_json::to_writer(&mut buffer, event).unwrap();
            buffer.push(b'\n');
        }
        std::fs::write(&archive, buffer).unwrap();
        assert!(matches!(
            FileAuditLog::open_with_options(&path, options.clone()),
            Err(AuditError::BrokenChain(_))
        ));
        cleanup(&path, 2);
    }

    #[tokio::test]
    async fn file_log_fails_closed_when_the_cross_file_checkpoint_link_is_broken() {
        let path = path("rotate-link");
        let options = rotation_options();
        {
            let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
            for sequence in 0..12 {
                log.append(input(sequence)).await.unwrap();
            }
        }
        // Truncate the previous generation: its new tail no longer matches the
        // checkpoint that opens the next file.
        let archive = generation_path(&path, 2);
        let mut events = read_events(&archive).unwrap();
        events.pop();
        let mut buffer = Vec::new();
        for event in &events {
            serde_json::to_writer(&mut buffer, event).unwrap();
            buffer.push(b'\n');
        }
        std::fs::write(&archive, buffer).unwrap();
        assert!(matches!(
            FileAuditLog::open_with_options(&path, options.clone()),
            Err(AuditError::BrokenChain(_))
        ));
        cleanup(&path, 2);
    }

    #[tokio::test]
    async fn file_log_snapshot_returns_the_bounded_memory_window() {
        let path = path("window");
        let options = FileAuditLogOptions {
            rotate_max_bytes: u64::MAX,
            rotate_max_events: u64::MAX,
            generations: 2,
            memory_window: 5,
        };
        {
            let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
            for sequence in 0..12 {
                log.append(input(sequence)).await.unwrap();
            }
            let snapshot = log.snapshot().await.unwrap();
            assert_eq!(snapshot.len(), 5);
            assert_eq!(snapshot[0].sequence, 7);
            assert_eq!(snapshot[4].sequence, 11);
        }
        // The window survives reopen: only the most recent events are retained.
        let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
        let snapshot = log.snapshot().await.unwrap();
        assert_eq!(snapshot.len(), 5);
        assert_eq!(snapshot[0].sequence, 7);
        assert_eq!(snapshot[4].sequence, 11);
        drop(log);
        cleanup(&path, 2);
    }

    #[tokio::test]
    async fn file_log_recovers_when_rotation_was_interrupted_before_the_checkpoint() {
        let path = path("rotate-recovery");
        let options = rotation_options();
        {
            let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
            for sequence in 0..12 {
                log.append(input(sequence)).await.unwrap();
            }
        }
        // Simulate a crash between the rename and the new-file creation.
        std::fs::remove_file(&path).unwrap();
        let log = FileAuditLog::open_with_options(&path, options.clone()).unwrap();
        let active = read_events(&path).unwrap();
        assert_eq!(active[0].kind, AuditEventKind::RotationCheckpoint);
        let previous = read_events(&generation_path(&path, 1)).unwrap();
        assert_eq!(active[0].previous_hash, previous.last().unwrap().event_hash);
        // Appends continue the global sequence after the recovered checkpoint.
        let event = log.append(input(50)).await.unwrap();
        assert_eq!(event.sequence, active[0].sequence + 1);
        assert_eq!(event.previous_hash, active[0].event_hash);
        drop(log);
        verify_retained_files(&path, 2);
        // The empty-active-file variant of the same interruption also recovers.
        std::fs::write(&path, b"").unwrap();
        assert!(FileAuditLog::open_with_options(&path, options.clone()).is_ok());
        cleanup(&path, 2);
    }
}
