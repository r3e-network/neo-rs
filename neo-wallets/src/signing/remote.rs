//! Wire protocol and verification boundary for dedicated remote signers.
//!
//! These types are intentionally independent from wallet RPC. Production
//! hardware attestation requires an explicitly installed verifier; this crate
//! does not treat parsed quote fields as proof.

use super::{
    KeyInfo, KeyRef, ProviderSignature, SIGNING_SCHEMA_VERSION, SignError, SignErrorCode,
    SignPayload, SignPurpose, SignResult,
};
use async_trait::async_trait;
use neo_crypto::Crypto;
use serde::{Deserialize, Serialize};

/// Current remote signer wire protocol version.
pub const REMOTE_SIGNER_PROTOCOL_VERSION: u16 = 1;

/// Challenge generated independently for one key attestation session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationChallenge {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Cryptographically random challenge.
    pub challenge: [u8; 32],
    /// Client wall-clock time used only for freshness bounds.
    pub issued_at_unix_ms: i64,
    /// Exact key the session will authorize.
    pub key_ref: KeyRef,
    /// Expected remote service identity.
    pub service_identity: String,
    /// Exact signing policy version.
    pub policy_version: String,
}

/// Evidence and declared claims returned by a remote hardware TEE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationEvidence {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Original challenge, echoed into the verifier binding.
    pub challenge: [u8; 32],
    /// Opaque session identifier.
    pub session_id: String,
    /// P-256 public key authenticating responses in this session.
    pub session_public_key: Vec<u8>,
    /// Attested service identity.
    pub service_identity: String,
    /// Attested signing policy version.
    pub policy_version: String,
    /// Attested key and immutable key version.
    pub key_ref: KeyRef,
    /// Evidence creation time.
    pub issued_at_unix_ms: i64,
    /// Hard session expiration time.
    pub expires_at_unix_ms: i64,
    /// Enclave/code measurement declared by the evidence format.
    pub measurement: Vec<u8>,
    /// Signer/author measurement declared by the evidence format.
    pub signer_measurement: Vec<u8>,
    /// Product identifier.
    pub product_id: u16,
    /// Security version number.
    pub security_version: u16,
    /// Whether debug execution is enabled.
    pub debug: bool,
    /// Opaque quote, token, or verifier-specific proof bytes.
    pub evidence: Vec<u8>,
}

impl AttestationEvidence {
    /// Computes the required report-data binding for a quote verifier.
    pub fn binding_digest(&self) -> SignResult<[u8; 32]> {
        let bytes = serde_json::to_vec(&(
            REMOTE_SIGNER_PROTOCOL_VERSION,
            self.challenge,
            &self.session_public_key,
            &self.service_identity,
            &self.policy_version,
            &self.key_ref,
        ))
        .map_err(|error| {
            SignError::new(
                SignErrorCode::InvalidRequest,
                format!("failed to encode attestation binding: {error}"),
            )
        })?;
        Ok(Crypto::sha256(&bytes))
    }
}

/// Expected hardware identity and freshness policy supplied to a verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationExpectations {
    /// Expected challenge.
    pub challenge: [u8; 32],
    /// Earliest acceptable evidence creation time.
    pub challenge_issued_at_unix_ms: i64,
    /// Maximum challenge round-trip age.
    pub max_challenge_age_ms: u64,
    /// Maximum accepted positive or negative evidence clock skew.
    pub max_clock_skew_ms: u64,
    /// Maximum session lifetime measured from evidence creation.
    pub max_session_age_ms: u64,
    /// Expected service identity.
    pub service_identity: String,
    /// Expected policy version.
    pub policy_version: String,
    /// Expected key reference.
    pub key_ref: KeyRef,
    /// Exact accepted code measurement.
    pub measurement: Vec<u8>,
    /// Optional accepted signer measurement.
    pub signer_measurement: Option<Vec<u8>>,
    /// Exact accepted product ID.
    pub product_id: u16,
    /// Minimum accepted security version.
    pub minimum_security_version: u16,
}

/// Collateral result produced by a cryptographic evidence verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollateralStatus {
    /// Collateral is current and cryptographically valid.
    Valid,
    /// Collateral is unavailable.
    Missing,
    /// Collateral is outside its validity interval.
    Expired,
    /// Collateral is malformed or invalid.
    Invalid,
}

/// TCB result produced by a cryptographic evidence verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TcbStatus {
    /// Platform TCB is current.
    UpToDate,
    /// Platform requires configuration and is not accepted.
    ConfigurationNeeded,
    /// Platform TCB is out of date and is not accepted.
    OutOfDate,
    /// Platform or key has been revoked.
    Revoked,
    /// Verifier could not establish a result.
    Unknown,
}

/// Claims established by cryptographic quote/token verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedAttestation {
    /// Report data cryptographically covered by the evidence.
    pub report_data: [u8; 32],
    /// Verified code measurement.
    pub measurement: Vec<u8>,
    /// Verified signer measurement.
    pub signer_measurement: Vec<u8>,
    /// Verified product identifier.
    pub product_id: u16,
    /// Verified security version.
    pub security_version: u16,
    /// Verified debug attribute.
    pub debug: bool,
    /// Verified platform TCB result.
    pub tcb_status: TcbStatus,
    /// Verified collateral result.
    pub collateral_status: CollateralStatus,
    /// True if any evidence, platform, or signing key revocation applied.
    pub revoked: bool,
}

/// Pluggable cryptographic verifier for remote hardware evidence.
///
/// Implementations must authenticate the evidence signature and certificate
/// chain, validate TCB and revocation collateral, and return only claims covered
/// by that verification. Parsing untrusted quote fields is insufficient.
pub trait RemoteAttestationVerifier: Send + Sync {
    /// Verifies opaque evidence against the complete expected binding.
    fn verify(
        &self,
        evidence: &AttestationEvidence,
        expected: &AttestationExpectations,
        now_unix_ms: i64,
    ) -> SignResult<VerifiedAttestation>;
}

/// Fail-closed verifier used when no production evidence verifier is installed.
#[derive(Debug, Default)]
pub struct RejectingAttestationVerifier;

impl RemoteAttestationVerifier for RejectingAttestationVerifier {
    fn verify(
        &self,
        _evidence: &AttestationEvidence,
        _expected: &AttestationExpectations,
        _now_unix_ms: i64,
    ) -> SignResult<VerifiedAttestation> {
        Err(SignError::new(
            SignErrorCode::CapabilityUnavailable,
            "no production remote hardware attestation verifier is installed",
        ))
    }
}

/// Key metadata response authenticated by the dedicated signer transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteKeyInfoResponse {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Application service identity mapped by local configuration.
    pub service_identity: String,
    /// Exact key metadata.
    pub key_info: KeyInfo,
}

/// Authorized request sent only over the dedicated remote signer client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSignRequest {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Stable request identifier and remote idempotency key.
    pub request_id: String,
    /// Exact key reference.
    pub key_ref: KeyRef,
    /// Neo network identifier.
    pub network: u32,
    /// Explicit signature purpose.
    pub purpose: SignPurpose,
    /// Canonical payload or digest.
    pub payload: SignPayload,
    /// Digest binding the local authorization request.
    pub request_digest: [u8; 32],
    /// Local policy version.
    pub policy_version: String,
    /// Optional attested session identifier; required for remote hardware TEE.
    pub attestation_session_id: Option<String>,
}

/// Remote signature plus attested-session authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSignResponse {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Original request identifier.
    pub request_id: String,
    /// Exact signed key reference.
    pub key_ref: KeyRef,
    /// Original request binding digest.
    pub request_digest: [u8; 32],
    /// Policy version used remotely.
    pub policy_version: String,
    /// Remote application identity.
    pub service_identity: String,
    /// Provider signature returned to the local signing service.
    pub provider_signature: ProviderSignature,
    /// Attestation session identifier, when hardware attestation is required.
    pub attestation_session_id: Option<String>,
    /// P-256 signature by the attested session key over [`Self::auth_digest`].
    pub session_signature: Option<Vec<u8>>,
}

impl RemoteSignResponse {
    /// Computes the digest authenticated by the attested session key.
    pub fn auth_digest(&self) -> SignResult<[u8; 32]> {
        let bytes = serde_json::to_vec(&(
            REMOTE_SIGNER_PROTOCOL_VERSION,
            &self.request_id,
            &self.key_ref,
            self.request_digest,
            &self.policy_version,
            &self.service_identity,
            &self.provider_signature,
            &self.attestation_session_id,
        ))
        .map_err(|error| {
            SignError::new(
                SignErrorCode::InvalidRequest,
                format!("failed to encode remote response binding: {error}"),
            )
        })?;
        Ok(Crypto::sha256(&bytes))
    }
}

/// Authenticated permanent rejection of one exact remote signing request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSignRejection {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Original request identifier.
    pub request_id: String,
    /// Exact rejected key reference.
    pub key_ref: KeyRef,
    /// Original request binding digest.
    pub request_digest: [u8; 32],
    /// Policy version used remotely.
    pub policy_version: String,
    /// Remote application identity authenticated by mTLS.
    pub service_identity: String,
    /// Stable rejection code.
    pub code: String,
    /// Non-secret rejection description.
    pub message: String,
    /// Must be true; temporary or ambiguous errors are never terminal.
    pub permanent: bool,
    /// Attestation session identifier, when hardware attestation is required.
    pub attestation_session_id: Option<String>,
    /// P-256 signature by the attested session key over [`Self::auth_digest`].
    pub session_signature: Option<Vec<u8>>,
}

impl RemoteSignRejection {
    /// Computes the digest authenticating the complete rejection and request binding.
    pub fn auth_digest(&self) -> SignResult<[u8; 32]> {
        let bytes = serde_json::to_vec(&(
            REMOTE_SIGNER_PROTOCOL_VERSION,
            &self.request_id,
            &self.key_ref,
            self.request_digest,
            &self.policy_version,
            &self.service_identity,
            &self.code,
            &self.message,
            self.permanent,
            &self.attestation_session_id,
        ))
        .map_err(|error| {
            SignError::new(
                SignErrorCode::InvalidRequest,
                format!("failed to encode remote rejection binding: {error}"),
            )
        })?;
        Ok(Crypto::sha256(&bytes))
    }
}

/// Result of dispatching a remote signing request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteSignDispatchResult {
    /// The remote signer returned signature material.
    Succeeded(RemoteSignResponse),
    /// The remote signer returned an authenticated permanent rejection.
    Rejected(RemoteSignRejection),
}

/// Query for an operation whose dispatch outcome was unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOperationQuery {
    /// Protocol schema version.
    pub schema_version: u16,
    /// Stable request identifier.
    pub request_id: String,
    /// Expected request binding digest.
    pub request_digest: [u8; 32],
    /// Exact key reference.
    pub key_ref: KeyRef,
    /// Optional attested session identifier.
    pub attestation_session_id: Option<String>,
}

/// Query result for an idempotent remote operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RemoteOperationStatus {
    /// Remote service has not completed the request.
    Pending,
    /// Remote service durably completed the request.
    Succeeded {
        /// Authenticated final response.
        response: RemoteSignResponse,
    },
    /// Remote service durably and authentically rejected the exact request.
    Failed {
        /// Complete authenticated permanent rejection.
        rejection: RemoteSignRejection,
    },
    /// Remote service has no record of the request.
    NotFound,
}

/// Dedicated transport boundary, implemented by mTLS HTTPS in production.
#[async_trait]
pub trait RemoteSignerTransport: Send + Sync {
    /// Retrieves authenticated key metadata.
    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<RemoteKeyInfoResponse>;

    /// Requests fresh challenge-bound hardware evidence.
    async fn attest(&self, challenge: &AttestationChallenge) -> SignResult<AttestationEvidence>;

    /// Dispatches one idempotent signing request.
    async fn sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult>;

    /// Queries a previously dispatched operation without re-signing.
    async fn query(&self, query: &RemoteOperationQuery) -> SignResult<RemoteOperationStatus>;
}

/// Validates protocol version used by all remote responses.
pub fn validate_remote_schema(schema_version: u16) -> SignResult<()> {
    if schema_version != REMOTE_SIGNER_PROTOCOL_VERSION || schema_version != SIGNING_SCHEMA_VERSION
    {
        return Err(SignError::new(
            SignErrorCode::InvalidRequest,
            "unsupported remote signer protocol version",
        ));
    }
    Ok(())
}
