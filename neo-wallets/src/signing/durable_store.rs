//! Durable append-only signing state for production integrations.

use super::error::{SignError, SignErrorCode, SignResult};
use super::state::{BeginCheck, SigningState};
use super::store::{
    BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation, SigningAuditEvent,
    SigningRequestState, SigningRequestStatus, SigningStore,
};
use super::types::SignResponse;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum JournalEvent {
    Begin {
        request: BeginSigningRequest,
    },
    Complete {
        request_id: String,
        response: SignResponse,
    },
    Fail {
        request_id: String,
        error: SignError,
    },
    Audit {
        record: SigningAuditEvent,
    },
}

#[derive(Debug)]
struct DurableInner {
    file: File,
    state: SigningState,
}

/// Append-only signing store that synchronizes every state transition before returning.
///
/// A companion process lock is held for the lifetime of the store. If a process exits
/// uncleanly the lock remains, deliberately requiring operator verification before the
/// signer can be restarted. This conservative behavior prevents accidental dual-active
/// use of one signing journal.
#[derive(Debug)]
pub struct DurableSigningStore {
    inner: Mutex<DurableInner>,
    lock_path: PathBuf,
    _lock_file: File,
}

impl DurableSigningStore {
    /// Opens or creates a durable signing journal and exclusively fences its path.
    pub fn open(path: impl AsRef<Path>) -> SignResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|error| {
                storage_error(format!("failed to create signing state directory: {error}"))
            })?;
        }

        let lock_path = path.with_extension("lock");
        let lock_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| {
                storage_error(format!(
                    "failed to acquire exclusive signing store fence '{}': {error}; verify no signer process is active before removing a stale fence",
                    lock_path.display()
                ))
            })?;

        let result = Self::open_fenced(path, lock_path.clone(), lock_file);
        if result.is_err() {
            let _ = fs::remove_file(lock_path);
        }
        result
    }

    fn open_fenced(path: &Path, lock_path: PathBuf, lock_file: File) -> SignResult<Self> {
        let mut bytes = Vec::new();
        if path.exists() {
            File::open(path)
                .and_then(|mut file| file.read_to_end(&mut bytes))
                .map_err(|error| {
                    storage_error(format!("failed to read signing journal: {error}"))
                })?;
        }
        let (state, valid_prefix_len) = replay_journal(&bytes)?;
        if valid_prefix_len != bytes.len() {
            let repair = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)
                .and_then(|file| {
                    file.set_len(valid_prefix_len as u64)?;
                    file.sync_all()
                })
                .map_err(|error| {
                    storage_error(format!(
                        "failed to truncate partial signing journal tail: {error}"
                    ))
                });
            repair?;
            sync_parent_directory(path)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| storage_error(format!("failed to open signing journal: {error}")))?;

        Ok(Self {
            inner: Mutex::new(DurableInner { file, state }),
            lock_path,
            _lock_file: lock_file,
        })
    }

    fn append(inner: &mut DurableInner, event: &JournalEvent) -> SignResult<()> {
        let mut encoded = serde_json::to_vec(event)
            .map_err(|error| storage_error(format!("failed to encode signing state: {error}")))?;
        encoded.push(b'\n');
        inner
            .file
            .write_all(&encoded)
            .and_then(|()| inner.file.sync_all())
            .map_err(|error| {
                storage_error(format!("failed to durably append signing state: {error}"))
            })
    }
}

impl Drop for DurableSigningStore {
    fn drop(&mut self) {
        let _ = self._lock_file.sync_all();
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[async_trait]
impl SigningStore for DurableSigningStore {
    async fn begin(&self, request: BeginSigningRequest) -> SignResult<BeginSigningOutcome> {
        let mut inner = self.inner.lock();
        match inner.state.check_begin(&request)? {
            BeginCheck::Existing(outcome) => Ok(outcome),
            BeginCheck::Reserve(plan) => {
                // The journal record must be durable before memory claims the
                // reservation, so a crash cannot lose a consumed quota unit.
                Self::append(
                    &mut inner,
                    &JournalEvent::Begin {
                        request: request.clone(),
                    },
                )?;
                inner.state.apply_begin(request, plan);
                Ok(BeginSigningOutcome::Acquired)
            }
        }
    }

    async fn status(
        &self,
        request_id: &str,
        provider: &str,
        request_digest: [u8; 32],
    ) -> SignResult<SigningRequestStatus> {
        self.inner
            .lock()
            .state
            .status(request_id, provider, request_digest)
    }

    async fn load(&self, request_id: &str) -> SignResult<Option<PersistedSigningOperation>> {
        self.inner.lock().state.load(request_id)
    }

    async fn complete(&self, request_id: &str, response: SignResponse) -> SignResult<()> {
        let mut inner = self.inner.lock();
        inner.state.check_completion(request_id, &response)?;
        Self::append(
            &mut inner,
            &JournalEvent::Complete {
                request_id: request_id.to_string(),
                response: response.clone(),
            },
        )?;
        inner.state.apply_completion(request_id, response);
        Ok(())
    }

    async fn fail(&self, request_id: &str, error: SignError) -> SignResult<()> {
        let mut inner = self.inner.lock();
        inner.state.check_failure(request_id)?;
        Self::append(
            &mut inner,
            &JournalEvent::Fail {
                request_id: request_id.to_string(),
                error: error.clone(),
            },
        )?;
        inner.state.apply_failure(request_id, error);
        Ok(())
    }

    async fn append_audit(&self, event: SigningAuditEvent) -> SignResult<()> {
        let mut inner = self.inner.lock();
        Self::append(&mut inner, &JournalEvent::Audit { record: event })
    }
}

fn replay_journal(bytes: &[u8]) -> SignResult<(SigningState, usize)> {
    let mut state = SigningState::default();
    let mut valid_prefix_len = 0usize;
    for raw_line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if !raw_line.ends_with(b"\n") {
            break;
        }
        valid_prefix_len = valid_prefix_len.saturating_add(raw_line.len());
        let line = &raw_line[..raw_line.len() - 1];
        if line.is_empty() {
            continue;
        }
        let event: JournalEvent = serde_json::from_slice(line).map_err(|error| {
            storage_error(format!("signing journal contains a corrupt event: {error}"))
        })?;
        match event {
            JournalEvent::Begin { request } => {
                if request.quota.window_ms <= 0 {
                    return Err(storage_error(
                        "signing journal contains an invalid quota window",
                    ));
                }
                if state.record(&request.request.request_id).is_some() {
                    return Err(storage_error(
                        "signing journal contains a duplicate begin event",
                    ));
                }
                let plan = state
                    .replay_plan(&request)
                    .ok_or_else(|| storage_error("signing journal quota counter overflow"))?;
                state.apply_begin(request, plan);
            }
            JournalEvent::Complete {
                request_id,
                response,
            } => {
                let Some(record) = state.record(&request_id) else {
                    return Err(storage_error(
                        "signing journal completes an unknown request",
                    ));
                };
                if record.state != SigningRequestState::Pending
                    || !record.response_matches(&response)
                {
                    return Err(storage_error(
                        "signing journal contains a mismatched completion binding",
                    ));
                }
                state.apply_completion(&request_id, response);
            }
            JournalEvent::Fail { request_id, error } => {
                if state.record(&request_id).is_none() {
                    return Err(storage_error("signing journal fails an unknown request"));
                }
                state.apply_failure(&request_id, error);
            }
            JournalEvent::Audit { .. } => {}
        }
    }
    Ok((state, valid_prefix_len))
}

fn sync_parent_directory(path: &Path) -> SignResult<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    #[cfg(windows)]
    let directory = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(parent)
    };
    #[cfg(not(windows))]
    let directory = File::open(parent);
    match directory.and_then(|directory| directory.sync_all()) {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Windows commonly rejects FlushFileBuffers for directory handles even
            // with FILE_FLAG_BACKUP_SEMANTICS. The journal file itself has already
            // been synchronized, which durably commits this in-place truncation.
            Ok(())
        }
        Err(error) => Err(storage_error(format!(
            "failed to synchronize signing journal directory: {error}"
        ))),
    }
}

fn storage_error(message: impl Into<String>) -> SignError {
    SignError::new(SignErrorCode::StorageFailure, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ALGORITHM_ECDSA_P256_SHA256, AuthenticatedContext, IdempotencyInfo, KeyInfo, KeyRef,
        ProtocolContext, QuotaReservation, SIGNING_SCHEMA_VERSION, SecurityLevel, SignPayload,
        SignPurpose, SignRequest,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use tempfile::tempdir;

    fn begin_request(id: &str) -> BeginSigningRequest {
        BeginSigningRequest {
            request: SignRequest {
                schema_version: SIGNING_SCHEMA_VERSION,
                request_id: id.to_string(),
                network: 860_833_102,
                purpose: SignPurpose::Transaction,
                key_ref: KeyRef::new("wallet", "key", 2),
                payload: SignPayload::Canonical {
                    encoding: "neo-n3-sign-data-v1".to_string(),
                    bytes: vec![1, 2, 3],
                },
                protocol_context: ProtocolContext {
                    schema_version: SIGNING_SCHEMA_VERSION,
                    protocol: "neo-n3-transaction".to_string(),
                    protocol_version: "1".to_string(),
                    fields: BTreeMap::from([("operation".to_string(), id.to_string())]),
                },
                policy_version: "policy-v1".to_string(),
                deadline_unix_ms: 2_000,
                idempotency: IdempotencyInfo {
                    schema_version: SIGNING_SCHEMA_VERSION,
                    key: format!("idem-{id}"),
                    replay_nonce: format!("nonce-{id}"),
                },
                fee_units: 0,
                quota_units: 1,
                account: Some("account".to_string()),
                targets: BTreeSet::new(),
            },
            context: AuthenticatedContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                principal: "service".to_string(),
                tenant: "node".to_string(),
                authentication_method: "local".to_string(),
                scopes: BTreeSet::from(["sign:transaction".to_string()]),
                claims: BTreeMap::new(),
            },
            request_digest: [7; 32],
            key_info: KeyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key_ref: KeyRef::new("wallet", "key", 2),
                algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                public_key: vec![2; 33],
                security_level: SecurityLevel::Software,
                enabled: true,
                attributes: BTreeMap::new(),
            },
            quota: QuotaReservation {
                scope: "scope".to_string(),
                units: 1,
                limit: 10,
                window_ms: 10_000,
            },
            now_unix_ms: 1_000,
        }
    }

    fn response(id: &str) -> SignResponse {
        SignResponse {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: id.to_string(),
            key_ref: KeyRef::new("wallet", "key", 2),
            algorithm: "ecdsa-p256-sha256".to_string(),
            signature: vec![1; 64],
            signed_digest: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: vec![1, 2, 3],
            }
            .signing_digest(),
            request_digest: [7; 32],
            policy_version: "policy-v1".to_string(),
            completed_at_unix_ms: 1_100,
            idempotent_replay: false,
        }
    }

    #[tokio::test]
    async fn completed_response_survives_reopen() {
        let directory = tempdir().expect("temp dir");
        let path = directory.path().join("signing.jsonl");
        {
            let store = DurableSigningStore::open(&path).expect("open");
            assert_eq!(
                store.begin(begin_request("one")).await.expect("begin"),
                BeginSigningOutcome::Acquired
            );
            store
                .complete("one", response("one"))
                .await
                .expect("complete");
        }
        let store = DurableSigningStore::open(&path).expect("reopen");
        let cached = store.begin(begin_request("one")).await.expect("cached");
        assert!(matches!(cached, BeginSigningOutcome::Cached(_)));
    }

    #[tokio::test]
    async fn pending_response_remains_unknown_after_reopen() {
        let directory = tempdir().expect("temp dir");
        let path = directory.path().join("signing.jsonl");
        {
            let store = DurableSigningStore::open(&path).expect("open");
            store.begin(begin_request("pending")).await.expect("begin");
        }
        let store = DurableSigningStore::open(&path).expect("reopen");
        assert_eq!(
            store
                .status("pending", "wallet", [7; 32])
                .await
                .expect("persisted status"),
            SigningRequestStatus::Pending
        );
        let mismatch = store
            .status("pending", "other-provider", [7; 32])
            .await
            .expect_err("persisted provider binding must be enforced");
        assert_eq!(mismatch.code, SignErrorCode::IdempotencyConflict);
        let error = store
            .begin(begin_request("pending"))
            .await
            .expect_err("pending must remain fail closed");
        assert_eq!(error.code, SignErrorCode::RequestInProgress);
    }

    #[tokio::test]
    async fn completion_rejects_changed_key_version_binding() {
        let directory = tempdir().expect("temp dir");
        let path = directory.path().join("signing.jsonl");
        let store = DurableSigningStore::open(&path).expect("open");
        store.begin(begin_request("bound")).await.expect("begin");
        let mut changed = response("bound");
        changed.key_ref = KeyRef::new("wallet", "key", 3);
        let error = store
            .complete("bound", changed)
            .await
            .expect_err("completion must retain the exact historical key version");
        assert_eq!(error.code, SignErrorCode::CorruptState);
        assert_eq!(
            store
                .status("bound", "wallet", [7; 32])
                .await
                .expect("pending status"),
            SigningRequestStatus::Pending
        );
    }

    #[tokio::test]
    async fn unterminated_tail_is_truncated_before_next_durable_append() {
        let directory = tempdir().expect("temp dir");
        let path = directory.path().join("signing.jsonl");
        {
            let store = DurableSigningStore::open(&path).expect("open");
            store
                .begin(begin_request("first"))
                .await
                .expect("first begin");
        }
        {
            let mut file = OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open partial tail");
            file.write_all(b"{\"event\":\"begin\"")
                .expect("write partial tail");
            file.sync_all().expect("sync partial tail");
        }
        {
            let store = DurableSigningStore::open(&path).expect("repair and reopen");
            let loaded = store
                .load("first")
                .await
                .expect("load first")
                .expect("persisted first request");
            assert_eq!(loaded.request.deadline_unix_ms, 2_000);
            assert_eq!(loaded.context.principal, "service");
            store
                .begin(begin_request("second"))
                .await
                .expect("append after repair");
        }
        let bytes = fs::read(&path).expect("read repaired journal");
        assert!(bytes.ends_with(b"\n"));
        // Not the naive filter-count idiom: the workspace has no bytecount dep.
        assert_eq!(
            bytes
                .iter()
                .map(|byte| usize::from(*byte == b'\n'))
                .sum::<usize>(),
            2
        );
        let store = DurableSigningStore::open(&path).expect("repaired journal replays");
        assert!(store.load("first").await.expect("first load").is_some());
        assert!(store.load("second").await.expect("second load").is_some());
    }

    #[test]
    fn second_active_store_is_fenced() {
        let directory = tempdir().expect("temp dir");
        let path = directory.path().join("signing.jsonl");
        let _first = DurableSigningStore::open(&path).expect("first");
        let error = DurableSigningStore::open(&path).expect_err("second must fail");
        assert_eq!(error.code, SignErrorCode::StorageFailure);
    }
}
