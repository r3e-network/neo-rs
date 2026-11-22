//! Policy-enforcing signing provider and service abstractions.

use super::error::{SignError, SignErrorCode, SignResult};
use super::policy::SigningPolicy;
use super::store::{
    AuditOutcome, BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation,
    SigningAuditEvent, SigningRequestStatus, SigningStore,
};
use super::types::{
    ALGORITHM_ECDSA_P256_SHA256, AuthenticatedContext, FrozenSignIntent, KeyInfo, KeyRef,
    ProtocolContext, ProviderCapabilities, ProviderSignature, SIGNING_SCHEMA_VERSION, SignPayload,
    SignPurpose, SignRequest, SignResponse, SigningKeyResolver,
};
use async_trait::async_trait;
use neo_crypto::Secp256r1Crypto;
use neo_primitives::time_provider::{TimeProvider, TimeSource};
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

/// Provider request that can only be constructed after service authorization.
///
/// External provider implementations can inspect this value but cannot create
/// it to bypass request validation, policy checks, persistence, or audit.
#[derive(Debug, Clone)]
pub struct AuthorizedSignRequest {
    request_id: String,
    key_ref: KeyRef,
    network: u32,
    purpose: SignPurpose,
    protocol_context: ProtocolContext,
    payload: SignPayload,
    signed_digest: [u8; 32],
    request_digest: [u8; 32],
    policy_version: String,
}

impl AuthorizedSignRequest {
    /// Returns the request identifier.
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Returns the exact key and key version authorized by policy.
    pub fn key_ref(&self) -> &KeyRef {
        &self.key_ref
    }

    /// Returns the authorized network identifier.
    pub fn network(&self) -> u32 {
        self.network
    }

    /// Returns the authorized signature purpose.
    pub fn purpose(&self) -> &SignPurpose {
        &self.purpose
    }

    /// Returns the protocol domain context.
    pub fn protocol_context(&self) -> &ProtocolContext {
        &self.protocol_context
    }

    /// Returns the exact payload authorized for provider execution.
    pub fn payload(&self) -> &SignPayload {
        &self.payload
    }

    /// Returns the payload digest authorized for signing.
    pub fn signed_digest(&self) -> &[u8; 32] {
        &self.signed_digest
    }

    /// Returns the digest binding the complete request and caller context.
    pub fn request_digest(&self) -> &[u8; 32] {
        &self.request_digest
    }

    /// Returns the policy version that authorized this request.
    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }
}

/// Provider-side result of querying an already dispatched signing operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOperationStatus {
    /// The provider has not established a terminal result.
    Pending,
    /// The provider returned signature material for the original request.
    Succeeded(ProviderSignature),
    /// The provider durably rejected or failed the original request.
    Failed(SignError),
    /// The provider has no record of the request; local state remains pending.
    NotFound,
}

/// Asynchronous boundary implemented by HSM, wallet, or remote key providers.
///
/// There is intentionally no public arbitrary-byte or blind-sign method.
#[async_trait]
pub trait SignerProvider: Send + Sync {
    /// Returns immutable, explicitly declared provider capabilities.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Returns metadata for an exact key version.
    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo>;

    /// Signs a service-authorized digest and protocol context.
    async fn sign_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature>;

    /// Queries an already dispatched request without signing it again.
    async fn query_authorized(
        &self,
        _request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderOperationStatus> {
        Ok(ProviderOperationStatus::Pending)
    }
}

/// Immutable registry that routes keys to explicitly named providers.
///
/// The registry never falls back to another provider: `KeyRef::provider` must
/// name an exact registered provider and duplicate names are rejected.
#[derive(Default)]
pub struct SignerProviderRegistry {
    providers: BTreeMap<String, Arc<dyn SignerProvider>>,
}

impl SignerProviderRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a provider under its declared capability name.
    pub fn register(&mut self, provider: Arc<dyn SignerProvider>) -> SignResult<()> {
        let name = provider.capabilities().provider;
        if name.trim().is_empty() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signer provider name must not be empty",
            ));
        }
        if self.providers.contains_key(&name) {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                format!("duplicate signer provider registration: {name}"),
            ));
        }
        self.providers.insert(name, provider);
        Ok(())
    }

    /// Returns the exact provider named by a key reference.
    pub fn provider_for(&self, key_ref: &KeyRef) -> SignResult<Arc<dyn SignerProvider>> {
        self.providers
            .get(&key_ref.provider)
            .cloned()
            .ok_or_else(|| {
                SignError::new(
                    SignErrorCode::CapabilityUnavailable,
                    format!("signer provider '{}' is not registered", key_ref.provider),
                )
            })
    }

    /// Returns declared capabilities for startup diagnostics and validation.
    #[must_use]
    pub fn capabilities(&self) -> Vec<ProviderCapabilities> {
        self.providers
            .values()
            .map(|provider| provider.capabilities())
            .collect()
    }
}

/// Asynchronous production signing service abstraction.
#[async_trait]
pub trait SignerService: Send + Sync {
    /// Validates, authorizes, reserves, audits, and signs a bound request.
    async fn sign(
        &self,
        request: SignRequest,
        context: AuthenticatedContext,
    ) -> SignResult<SignResponse>;

    /// Loads and validates an operation before either querying it or creating it once.
    async fn resume_or_start(
        &self,
        operation_id: &str,
        frozen_intent: FrozenSignIntent,
        context: AuthenticatedContext,
        request_timeout: Duration,
    ) -> SignResult<SignResponse>;

    /// Recovers a previously dispatched request without re-signing.
    ///
    /// The default is an explicit fail-closed boundary: services that cannot
    /// answer recovery queries surface [`SignErrorCode::CapabilityUnavailable`]
    /// instead of silently reporting success or inventing state. Implementors
    /// with durable operation stores override this — [`DefaultSignerService`]
    /// provides the production implementation (persisted-state load, binding
    /// digest check, provider query, signature re-verification).
    async fn recover(
        &self,
        _request: SignRequest,
        _context: AuthenticatedContext,
    ) -> SignResult<SigningRequestStatus> {
        Err(SignError::new(
            SignErrorCode::CapabilityUnavailable,
            "signing recovery is not implemented by this service",
        ))
    }

    /// Loads the original persisted request by operation ID and queries only.
    ///
    /// Same fail-closed contract as [`SignerService::recover`]: the default
    /// rejects callers with [`SignErrorCode::CapabilityUnavailable`] and only
    /// services with durable operation stores should override it.
    /// [`DefaultSignerService`] provides the production implementation.
    async fn recover_operation(
        &self,
        _request_id: &str,
        _context: AuthenticatedContext,
    ) -> SignResult<SigningRequestStatus> {
        Err(SignError::new(
            SignErrorCode::CapabilityUnavailable,
            "operation-id signing recovery is not implemented by this service",
        ))
    }
}

mod default_service;
mod trusted_client;

pub use default_service::DefaultSignerService;
pub use trusted_client::{SigningDeadlineRegistry, TrustedSignerClient};
