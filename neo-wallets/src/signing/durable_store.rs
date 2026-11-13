//! Durable append-only signing state for production integrations.

use super::error::{SignError, SignErrorCode, SignResult};
use super::store::{
    BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation, SigningAuditEvent,
    SigningRequestState, SigningRequestStatus, SigningStore,
};
use super::types::{AuthenticatedContext, KeyInfo, SignRequest, SignResponse};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct DurableRecord {
    request: SignRequest,
    context: AuthenticatedContext,
    provider: String,
    digest: [u8; 32],
    key_info: KeyInfo,
    state: SigningRequestState,
    response: Option<SignResponse>,
    error: Option<SignError>,
}

#[derive(Debug, Default)]
struct DurableState {
    records: HashMap<String, DurableRecord>,
    idempotency: HashMap<(String, String), String>,
    nonces: HashMap<(String, String), String>,
    quotas: HashMap<(String, i64), u64>,
}

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
    state: DurableState,
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

    fn existing_outcome(
        record: &DurableRecord,
        digest: [u8; 32],
        request_id: &str,
    ) -> SignResult<BeginSigningOutcome> {
        if record.digest != digest {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "request identifier or idempotency key was reused for different content",
            )
            .for_request(request_id.to_string()));
        }
        match record.state {
            SigningRequestState::Succeeded => record
                .response
                .clone()
                .map(BeginSigningOutcome::Cached)
                .ok_or_else(|| corrupt(request_id, "successful signing record has no response")),
            SigningRequestState::Pending => Err(SignError::new(
                SignErrorCode::RequestInProgress,
                "matching signing request is pending or has an unknown result",
            )
            .for_request(request_id.to_string())
            .retryable(true)),
            SigningRequestState::Failed => Err(record
                .error
                .clone()
                .unwrap_or_else(|| corrupt(request_id, "failed signing record has no error"))),
        }
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
        let request_id = request.request.request_id.clone();
        let provider = request.request.key_ref.provider.clone();
        let tenant = request.context.tenant.clone();
        let idempotency_key = request.request.idempotency.key.clone();
        let replay_nonce = request.request.idempotency.replay_nonce.clone();
        let deadline_unix_ms = request.request.deadline_unix_ms;
        if provider.trim().is_empty() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing request provider must not be empty",
            )
            .for_request(request_id.clone()));
        }
        if deadline_unix_ms <= request.now_unix_ms {
            return Err(SignError::new(
                SignErrorCode::DeadlineExceeded,
                "signing request expired before storage reservation",
            )
            .for_request(request_id.clone()));
        }
        if request.quota.window_ms <= 0 {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing quota window must be positive",
            )
            .for_request(request_id.clone()));
        }

        let mut inner = self.inner.lock();
        if let Some(record) = inner.state.records.get(&request_id) {
            return Self::existing_outcome(record, request.request_digest, &request_id);
        }

        let idempotency_scope = (tenant.clone(), idempotency_key);
        if let Some(existing_id) = inner.state.idempotency.get(&idempotency_scope) {
            let record = inner.state.records.get(existing_id).ok_or_else(|| {
                storage_error("idempotency index references a missing signing request")
            })?;
            return Self::existing_outcome(record, request.request_digest, &request_id);
        }

        let nonce_scope = (tenant, replay_nonce);
        if inner.state.nonces.contains_key(&nonce_scope) {
            return Err(SignError::new(
                SignErrorCode::ReplayDetected,
                "tenant replay nonce has already been consumed",
            )
            .for_request(request_id.clone()));
        }

        let window_start =
            request.now_unix_ms.div_euclid(request.quota.window_ms) * request.quota.window_ms;
        let quota_key = (request.quota.scope.clone(), window_start);
        let used = inner.state.quotas.get(&quota_key).copied().unwrap_or(0);
        let next = used.checked_add(request.quota.units).ok_or_else(|| {
            SignError::new(
                SignErrorCode::LimitExceeded,
                "signing quota counter overflow",
            )
            .for_request(request_id.clone())
        })?;
        if next > request.quota.limit {
            return Err(SignError::new(
                SignErrorCode::LimitExceeded,
                "signing quota would be exceeded",
            )
            .for_request(request_id.clone())
            .with_detail("quota_limit_units", request.quota.limit.to_string()));
        }

        Self::append(
            &mut inner,
            &JournalEvent::Begin {
                request: request.clone(),
            },
        )?;
        apply_begin(&mut inner.state, request, quota_key, next);
        Ok(BeginSigningOutcome::Acquired)
    }

    async fn status(
        &self,
        request_id: &str,
        provider: &str,
        request_digest: [u8; 32],
    ) -> SignResult<SigningRequestStatus> {
        let inner = self.inner.lock();
        let Some(record) = inner.state.records.get(request_id) else {
            return Ok(SigningRequestStatus::NotFound);
        };
        if record.provider != provider {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "recovery provider does not match persisted signing state",
            )
            .for_request(request_id.to_string()));
        }
        if record.digest != request_digest {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "recovery request digest does not match persisted signing state",
            )
            .for_request(request_id.to_string()));
        }
        match record.state {
            SigningRequestState::Pending => Ok(SigningRequestStatus::Pending),
            SigningRequestState::Succeeded => record
                .response
                .clone()
                .map(SigningRequestStatus::Succeeded)
                .ok_or_else(|| corrupt(request_id, "successful signing record has no response")),
            SigningRequestState::Failed => record
                .error
                .clone()
                .map(SigningRequestStatus::Failed)
                .ok_or_else(|| corrupt(request_id, "failed signing record has no error")),
        }
    }

    async fn load(&self, request_id: &str) -> SignResult<Option<PersistedSigningOperation>> {
        let inner = self.inner.lock();
        let Some(record) = inner.state.records.get(request_id) else {
            return Ok(None);
        };
        let status = match record.state {
            SigningRequestState::Pending => SigningRequestStatus::Pending,
            SigningRequestState::Succeeded => {
                SigningRequestStatus::Succeeded(record.response.clone().ok_or_else(|| {
                    corrupt(request_id, "successful signing record has no response")
                })?)
            }
            SigningRequestState::Failed => SigningRequestStatus::Failed(
                record
                    .error
                    .clone()
                    .ok_or_else(|| corrupt(request_id, "failed signing record has no error"))?,
            ),
        };
        Ok(Some(PersistedSigningOperation {
            request: record.request.clone(),
            context: record.context.clone(),
            request_digest: record.digest,
            key_info: record.key_info.clone(),
            status,
        }))
    }

    async fn complete(&self, request_id: &str, response: SignResponse) -> SignResult<()> {
        let mut inner = self.inner.lock();
        let record =
            inner.state.records.get(request_id).ok_or_else(|| {
                corrupt(request_id, "cannot complete an unreserved signing request")
            })?;
        if record.state != SigningRequestState::Pending {
            return Err(corrupt(
                request_id,
                "only pending signing requests may complete",
            ));
        }
        if !response_matches(record, &response) {
            return Err(corrupt(
                request_id,
                "signing response does not match the complete persisted request binding",
            ));
        }
        Self::append(
            &mut inner,
            &JournalEvent::Complete {
                request_id: request_id.to_string(),
                response: response.clone(),
            },
        )?;
        let record = inner
            .state
            .records
            .get_mut(request_id)
            .expect("checked above");
        record.state = SigningRequestState::Succeeded;
        record.response = Some(response);
        Ok(())
    }

    async fn fail(&self, request_id: &str, error: SignError) -> SignResult<()> {
        let mut inner = self.inner.lock();
        if !inner.state.records.contains_key(request_id) {
            return Err(corrupt(
                request_id,
                "cannot fail an unreserved signing request",
            ));
        }
        Self::append(
            &mut inner,
            &JournalEvent::Fail {
                request_id: request_id.to_string(),
                error: error.clone(),
            },
        )?;
        let record = inner
            .state
            .records
            .get_mut(request_id)
            .expect("checked above");
        record.state = SigningRequestState::Failed;
        record.error = Some(error);
        Ok(())
    }

    async fn append_audit(&self, event: SigningAuditEvent) -> SignResult<()> {
        let mut inner = self.inner.lock();
        Self::append(&mut inner, &JournalEvent::Audit { record: event })
    }
}

fn replay_journal(bytes: &[u8]) -> SignResult<(DurableState, usize)> {
    let mut state = DurableState::default();
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
                if state.records.contains_key(&request.request.request_id) {
                    return Err(storage_error(
                        "signing journal contains a duplicate begin event",
                    ));
                }
                let window_start = request.now_unix_ms.div_euclid(request.quota.window_ms)
                    * request.quota.window_ms;
                let quota_key = (request.quota.scope.clone(), window_start);
                let next = state
                    .quotas
                    .get(&quota_key)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(request.quota.units)
                    .ok_or_else(|| storage_error("signing journal quota counter overflow"))?;
                apply_begin(&mut state, request, quota_key, next);
            }
            JournalEvent::Complete {
                request_id,
                response,
            } => {
                let record = state
                    .records
                    .get_mut(&request_id)
                    .ok_or_else(|| storage_error("signing journal completes an unknown request"))?;
                if record.state != SigningRequestState::Pending
                    || !response_matches(record, &response)
                {
                    return Err(storage_error(
                        "signing journal contains a mismatched completion binding",
                    ));
                }
                record.state = SigningRequestState::Succeeded;
                record.response = Some(response);
            }
            JournalEvent::Fail { request_id, error } => {
                let record = state
                    .records
                    .get_mut(&request_id)
                    .ok_or_else(|| storage_error("signing journal fails an unknown request"))?;
                record.state = SigningRequestState::Failed;
                record.error = Some(error);
            }
            JournalEvent::Audit { .. } => {}
        }
    }
    Ok((state, valid_prefix_len))
}

fn response_matches(record: &DurableRecord, response: &SignResponse) -> bool {
    response.schema_version == record.request.schema_version
        && response.request_id == record.request.request_id
        && response.key_ref == record.request.key_ref
        && response.algorithm == record.key_info.algorithm
        && response.signed_digest == record.request.payload.signing_digest()
        && response.request_digest == record.digest
        && response.policy_version == record.request.policy_version
}

fn apply_begin(
    state: &mut DurableState,
    request: BeginSigningRequest,
    quota_key: (String, i64),
    quota_value: u64,
) {
    state.quotas.insert(quota_key, quota_value);
    state.idempotency.insert(
        (
            request.context.tenant.clone(),
            request.request.idempotency.key.clone(),
        ),
        request.request.request_id.clone(),
    );
    state.nonces.insert(
        (
            request.context.tenant.clone(),
            request.request.idempotency.replay_nonce.clone(),
        ),
        request.request.request_id.clone(),
    );
    state.records.insert(
        request.request.request_id.clone(),
        DurableRecord {
            provider: request.request.key_ref.provider.clone(),
            request: request.request,
            context: request.context,
            digest: request.request_digest,
            key_info: request.key_info,
            state: SigningRequestState::Pending,
            response: None,
            error: None,
        },
    );
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

fn corrupt(request_id: &str, message: &str) -> SignError {
    SignError::new(SignErrorCode::CorruptState, message).for_request(request_id.to_string())
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
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 2);
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
