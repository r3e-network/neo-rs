//! Test-only in-memory implementation of signing persistence.

use super::error::{SignError, SignErrorCode, SignResult};
use super::store::{
    BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation, SigningAuditEvent,
    SigningRequestState, SigningRequestStatus, SigningStore,
};
use super::types::{AuthenticatedContext, KeyInfo, SignRequest, SignResponse};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct MemoryRecord {
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
struct MemoryState {
    records: HashMap<String, MemoryRecord>,
    idempotency: HashMap<(String, String), String>,
    nonces: HashMap<(String, String), String>,
    quotas: HashMap<(String, i64), u64>,
    audits: Vec<SigningAuditEvent>,
}

/// In-memory state store available only to this crate's unit tests.
#[derive(Debug, Default)]
pub(super) struct InMemorySigningStore {
    state: Mutex<MemoryState>,
}

impl InMemorySigningStore {
    pub(super) fn audits(&self) -> Vec<SigningAuditEvent> {
        self.state.lock().audits.clone()
    }

    fn existing_outcome(
        record: &MemoryRecord,
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
                .ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::CorruptState,
                        "successful idempotency record has no response",
                    )
                    .for_request(request_id.to_string())
                }),
            SigningRequestState::Pending => Err(SignError::new(
                SignErrorCode::RequestInProgress,
                "matching signing request is still in progress",
            )
            .for_request(request_id.to_string())
            .retryable(true)),
            SigningRequestState::Failed => Err(record.error.clone().unwrap_or_else(|| {
                SignError::new(
                    SignErrorCode::CorruptState,
                    "failed idempotency record has no error",
                )
                .for_request(request_id.to_string())
            })),
        }
    }
}

#[async_trait]
impl SigningStore for InMemorySigningStore {
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
        let mut state = self.state.lock();
        if let Some(record) = state.records.get(&request_id) {
            return Self::existing_outcome(record, request.request_digest, &request_id);
        }

        let idempotency_scope = (tenant.clone(), idempotency_key);
        if let Some(existing_id) = state.idempotency.get(&idempotency_scope) {
            let record = state.records.get(existing_id).ok_or_else(|| {
                SignError::new(
                    SignErrorCode::CorruptState,
                    "idempotency index references a missing request",
                )
            })?;
            return Self::existing_outcome(record, request.request_digest, &request_id);
        }

        let nonce_scope = (tenant, replay_nonce);
        if state.nonces.contains_key(&nonce_scope) {
            return Err(SignError::new(
                SignErrorCode::ReplayDetected,
                "tenant replay nonce has already been consumed",
            )
            .for_request(request_id.clone()));
        }

        let window_start =
            request.now_unix_ms.div_euclid(request.quota.window_ms) * request.quota.window_ms;
        let quota_key = (request.quota.scope.clone(), window_start);
        let used = state.quotas.get(&quota_key).copied().unwrap_or(0);
        let next = used.checked_add(request.quota.units).ok_or_else(|| {
            SignError::new(SignErrorCode::LimitExceeded, "quota counter overflow")
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

        state.quotas.insert(quota_key, next);
        state
            .idempotency
            .insert(idempotency_scope, request_id.clone());
        state.nonces.insert(nonce_scope, request_id.clone());
        state.records.insert(
            request_id,
            MemoryRecord {
                provider,
                request: request.request,
                context: request.context,
                digest: request.request_digest,
                key_info: request.key_info,
                state: SigningRequestState::Pending,
                response: None,
                error: None,
            },
        );
        Ok(BeginSigningOutcome::Acquired)
    }

    async fn status(
        &self,
        request_id: &str,
        provider: &str,
        request_digest: [u8; 32],
    ) -> SignResult<SigningRequestStatus> {
        let state = self.state.lock();
        let Some(record) = state.records.get(request_id) else {
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
                .ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::CorruptState,
                        "successful signing record has no response",
                    )
                    .for_request(request_id.to_string())
                }),
            SigningRequestState::Failed => record
                .error
                .clone()
                .map(SigningRequestStatus::Failed)
                .ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::CorruptState,
                        "failed signing record has no error",
                    )
                    .for_request(request_id.to_string())
                }),
        }
    }

    async fn load(&self, request_id: &str) -> SignResult<Option<PersistedSigningOperation>> {
        let state = self.state.lock();
        let Some(record) = state.records.get(request_id) else {
            return Ok(None);
        };
        let status = match record.state {
            SigningRequestState::Pending => SigningRequestStatus::Pending,
            SigningRequestState::Succeeded => {
                SigningRequestStatus::Succeeded(record.response.clone().ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::CorruptState,
                        "successful signing record has no response",
                    )
                    .for_request(request_id.to_string())
                })?)
            }
            SigningRequestState::Failed => {
                SigningRequestStatus::Failed(record.error.clone().ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::CorruptState,
                        "failed signing record has no error",
                    )
                    .for_request(request_id.to_string())
                })?)
            }
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
        let mut state = self.state.lock();
        let record = state.records.get_mut(request_id).ok_or_else(|| {
            SignError::new(
                SignErrorCode::CorruptState,
                "cannot complete an unreserved signing request",
            )
            .for_request(request_id.to_string())
        })?;
        if record.state != SigningRequestState::Pending {
            return Err(SignError::new(
                SignErrorCode::CorruptState,
                "only pending signing requests may complete",
            )
            .for_request(request_id.to_string()));
        }
        if response.schema_version != record.request.schema_version
            || response.request_id != request_id
            || response.key_ref != record.request.key_ref
            || response.algorithm != record.key_info.algorithm
            || response.signed_digest != record.request.payload.signing_digest()
            || response.request_digest != record.digest
            || response.policy_version != record.request.policy_version
        {
            return Err(SignError::new(
                SignErrorCode::CorruptState,
                "signing response does not match the complete persisted request binding",
            )
            .for_request(request_id.to_string()));
        }
        record.state = SigningRequestState::Succeeded;
        record.response = Some(response);
        Ok(())
    }

    async fn fail(&self, request_id: &str, error: SignError) -> SignResult<()> {
        let mut state = self.state.lock();
        let record = state.records.get_mut(request_id).ok_or_else(|| {
            SignError::new(
                SignErrorCode::CorruptState,
                "cannot fail an unreserved signing request",
            )
            .for_request(request_id.to_string())
        })?;
        record.state = SigningRequestState::Failed;
        record.error = Some(error);
        Ok(())
    }

    async fn append_audit(&self, event: SigningAuditEvent) -> SignResult<()> {
        self.state.lock().audits.push(event);
        Ok(())
    }
}
