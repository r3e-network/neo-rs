use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;

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

/// Hash-chained JSONL audit log that synchronizes each append before success.
#[derive(Debug)]
pub struct FileAuditLog {
    state: Mutex<FileAuditState>,
}

#[derive(Debug)]
struct FileAuditState {
    file: File,
    events: Vec<AuditEvent>,
}

impl FileAuditLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| AuditError::Unavailable)?;
        }
        let mut events = Vec::new();
        if path.exists() {
            let reader = BufReader::new(File::open(path).map_err(|_| AuditError::Unavailable)?);
            for line in reader.lines() {
                let line = line.map_err(|_| AuditError::Unavailable)?;
                if !line.trim().is_empty() {
                    events
                        .push(serde_json::from_str(&line).map_err(|_| AuditError::Serialization)?);
                }
            }
            verify_audit_chain(&events)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|_| AuditError::Unavailable)?;
        Ok(Self {
            state: Mutex::new(FileAuditState { file, events }),
        })
    }
}

#[async_trait]
impl AuditLog for FileAuditLog {
    async fn append(&self, input: AuditInput) -> Result<AuditEvent, AuditError> {
        validate_input(&input)?;
        let mut state = self.state.lock().map_err(|_| AuditError::Unavailable)?;
        let sequence = state.events.len() as u64;
        let previous_hash = state
            .events
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
        let encoded = serde_json::to_vec(&event).map_err(|_| AuditError::Serialization)?;
        state
            .file
            .write_all(&encoded)
            .and_then(|()| state.file.write_all(b"\n"))
            .and_then(|()| state.file.sync_data())
            .map_err(|_| AuditError::Unavailable)?;
        state.events.push(event.clone());
        Ok(event)
    }

    async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError> {
        self.state
            .lock()
            .map(|state| state.events.clone())
            .map_err(|_| AuditError::Unavailable)
    }
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
            .any(|reference| reference.is_empty())
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
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuditError {
    #[error("audit input is invalid")]
    InvalidInput,
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
}
