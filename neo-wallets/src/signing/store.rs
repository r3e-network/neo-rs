//! Persistence contracts for idempotency, replay protection, quotas, and audit.

use super::error::{SignError, SignResult};
use super::policy::QuotaReservation;
use super::types::{AuthenticatedContext, KeyInfo, KeyRef, SignPurpose, SignRequest, SignResponse};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// State of a persisted signing operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningRequestState {
    /// Quota is reserved and provider work may be in progress.
    Pending,
    /// A response was durably recorded.
    Succeeded,
    /// The operation failed after reservation.
    Failed,
}

/// Persisted outcome returned by an authenticated recovery query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigningRequestStatus {
    /// No request with the supplied identifier exists.
    NotFound,
    /// Provider completion remains unknown and must not be re-dispatched.
    Pending,
    /// A locally verified response was durably recorded.
    Succeeded(SignResponse),
    /// A terminal failure was durably recorded.
    Failed(SignError),
}

/// Atomic request reservation submitted to persistent storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginSigningRequest {
    /// Complete first request, persisted before provider dispatch.
    pub request: SignRequest,
    /// Complete first authenticated context, persisted before provider dispatch.
    pub context: AuthenticatedContext,
    /// Digest of the complete request binding.
    pub request_digest: [u8; 32],
    /// Provider metadata authorized before the first dispatch.
    pub key_info: KeyInfo,
    /// Atomic quota reservation.
    pub quota: QuotaReservation,
    /// Current Unix timestamp in milliseconds.
    pub now_unix_ms: i64,
}

/// Result of atomically checking idempotency, replay state, deadline, and quota.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeginSigningOutcome {
    /// The caller owns a new pending reservation and may invoke the provider.
    Acquired,
    /// A prior successful response matches the same request digest.
    Cached(SignResponse),
}

/// Complete persisted operation used for query-only restart recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedSigningOperation {
    /// Original complete request, including its immutable deadline and payload.
    pub request: SignRequest,
    /// Original authenticated authorization context.
    pub context: AuthenticatedContext,
    /// Durable digest of the complete original authorization binding.
    pub request_digest: [u8; 32],
    /// Historical key metadata authorized before the original dispatch.
    pub key_info: KeyInfo,
    /// Current persisted result state.
    pub status: SigningRequestStatus,
}

/// Audit event outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    /// Policy authorization succeeded.
    Authorized,
    /// Policy authorization failed.
    Denied,
    /// A cached idempotent response was returned.
    IdempotentReplay,
    /// Provider signing completed successfully.
    Signed,
    /// Provider dispatch timed out and may still have completed remotely.
    ResultUnknown,
    /// Processing failed after entering the service.
    Failed,
}

/// Append-only structured signing audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningAuditEvent {
    /// Audit schema version.
    pub schema_version: u16,
    /// Event timestamp in Unix milliseconds.
    pub timestamp_unix_ms: i64,
    /// Request identifier.
    pub request_id: String,
    /// Authenticated principal.
    pub principal: String,
    /// Authenticated tenant.
    pub tenant: String,
    /// Versioned key reference.
    pub key_ref: KeyRef,
    /// Network identifier.
    pub network: u32,
    /// Signature purpose.
    pub purpose: SignPurpose,
    /// Policy version presented by the request.
    pub policy_version: String,
    /// Digest of the complete request binding, when available.
    pub request_digest: Option<[u8; 32]>,
    /// Event outcome.
    pub outcome: AuditOutcome,
    /// Stable error code or explanatory reason.
    pub reason: Option<String>,
    /// Additional non-secret audit attributes.
    pub attributes: BTreeMap<String, String>,
}

/// Durable state boundary required by the signing service.
///
/// `begin` must be one atomic transaction: it checks deadline, request ID,
/// tenant-scoped idempotency key, replay nonce, and quota before inserting a
/// pending record. Production implementations must survive process restarts.
#[async_trait]
pub trait SigningStore: Send + Sync {
    /// Atomically acquires a request or returns its cached response.
    async fn begin(&self, request: BeginSigningRequest) -> SignResult<BeginSigningOutcome>;

    /// Queries persisted state and verifies the caller-supplied request digest.
    async fn status(
        &self,
        request_id: &str,
        provider: &str,
        request_digest: [u8; 32],
    ) -> SignResult<SigningRequestStatus>;

    /// Loads the original complete request and current result by operation ID.
    async fn load(&self, request_id: &str) -> SignResult<Option<PersistedSigningOperation>>;

    /// Atomically marks a pending request successful and stores its response.
    async fn complete(&self, request_id: &str, response: SignResponse) -> SignResult<()>;

    /// Marks a pending request failed while retaining replay and audit state.
    async fn fail(&self, request_id: &str, error: SignError) -> SignResult<()>;

    /// Appends an immutable audit event.
    async fn append_audit(&self, event: SigningAuditEvent) -> SignResult<()>;
}
