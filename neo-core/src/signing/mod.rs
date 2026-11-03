//! Unified, policy-bound signing service contracts.
//!
//! This module separates authenticated authorization, persistent replay and
//! quota state, and provider execution. It intentionally exposes no production
//! shortcut for arbitrary blind signing.

mod durable_store;
mod error;
mod policy;
mod remote;
mod service;
mod store;
mod types;

#[cfg(test)]
mod memory_store;
#[cfg(test)]
mod tests;

pub use durable_store::DurableSigningStore;
pub use error::{SIGN_ERROR_SCHEMA_VERSION, SignError, SignErrorCode, SignResult};
pub use policy::{PolicyDecision, PolicyLimits, PolicySelector, QuotaReservation, SigningPolicy};
pub use remote::{
    AttestationChallenge, AttestationEvidence, AttestationExpectations, CollateralStatus,
    REMOTE_SIGNER_PROTOCOL_VERSION, RejectingAttestationVerifier, RemoteAttestationVerifier,
    RemoteKeyInfoResponse, RemoteOperationQuery, RemoteOperationStatus, RemoteSignDispatchResult,
    RemoteSignRejection, RemoteSignRequest, RemoteSignResponse, RemoteSignerTransport, TcbStatus,
    VerifiedAttestation, validate_remote_schema,
};
pub use service::{
    AuthorizedSignRequest, DefaultSignerService, ProviderOperationStatus, SignerProvider,
    SignerProviderRegistry, SignerService, SigningDeadlineRegistry, TrustedSignerClient,
};
pub use store::{
    AuditOutcome, BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation,
    SigningAuditEvent, SigningRequestState, SigningRequestStatus, SigningStore,
};
pub use types::{
    ALGORITHM_ECDSA_P256_SHA256, AuthenticatedContext, DigestAlgorithm, FrozenSignIntent,
    IdempotencyInfo, KeyInfo, KeyRef, ProtocolContext, ProviderCapabilities,
    ProviderOutcomeProvenance, ProviderSignature, SIGNING_SCHEMA_VERSION, SecurityLevel,
    SignPayload, SignPurpose, SignRequest, SignResponse, SigningKeyBinding, SigningKeyResolver,
};
