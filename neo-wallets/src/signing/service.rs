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

/// Process-local registry that preserves the first deadline assigned to an operation.
///
/// The durable signing store still binds the deadline in the request digest, so a
/// retry after process restart cannot extend an authorization without producing an
/// idempotency conflict.
#[derive(Debug, Clone, Default)]
pub struct SigningDeadlineRegistry {
    deadlines: Arc<Mutex<HashMap<String, i64>>>,
}

impl SigningDeadlineRegistry {
    /// Returns the first deadline assigned to `operation_id`, never refreshing it.
    pub fn resolve(
        &self,
        operation_id: &str,
        now_unix_ms: i64,
        timeout: Duration,
    ) -> SignResult<i64> {
        if operation_id.trim().is_empty() || timeout.is_zero() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "stable signing deadline requires an operation id and positive timeout",
            ));
        }
        let mut deadlines = self.deadlines.lock();
        if let Some(deadline) = deadlines.get(operation_id) {
            return Ok(*deadline);
        }
        let deadline = now_unix_ms.saturating_add(timeout.as_millis().min(i64::MAX as u128) as i64);
        deadlines.insert(operation_id.to_string(), deadline);
        Ok(deadline)
    }
}

/// Immutable signer client assembled by a trusted local integration layer.
///
/// Business services receive this client rather than constructing caller
/// identities or retaining access to wallet private keys.
#[derive(Clone)]
pub struct TrustedSignerClient {
    service: Arc<dyn SignerService>,
    default_provider: String,
    policy_version: String,
    request_timeout: Duration,
    context: AuthenticatedContext,
    key_resolver: Arc<SigningKeyResolver>,
    deadlines: SigningDeadlineRegistry,
}

impl TrustedSignerClient {
    /// Creates a client with an immutable authenticated service identity.
    pub fn new(
        service: Arc<dyn SignerService>,
        default_provider: impl Into<String>,
        policy_version: impl Into<String>,
        request_timeout: Duration,
        context: AuthenticatedContext,
        key_resolver: Arc<SigningKeyResolver>,
    ) -> SignResult<Self> {
        let default_provider = default_provider.into();
        let policy_version = policy_version.into();
        if default_provider.trim().is_empty()
            || policy_version.trim().is_empty()
            || request_timeout.is_zero()
            || context.schema_version != SIGNING_SCHEMA_VERSION
            || context.principal.trim().is_empty()
            || context.tenant.trim().is_empty()
            || context.authentication_method.trim().is_empty()
        {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "trusted signer client requires a provider, policy, timeout, and authenticated identity",
            ));
        }
        Ok(Self {
            service,
            default_provider,
            policy_version,
            request_timeout,
            context,
            key_resolver,
            deadlines: SigningDeadlineRegistry::default(),
        })
    }

    /// Signs through the policy service using the startup-bound identity.
    pub async fn sign(&self, request: SignRequest) -> SignResult<SignResponse> {
        self.service.sign(request, self.context.clone()).await
    }

    /// Resumes the exact durable operation or creates its immutable request once.
    pub async fn resume_or_start(
        &self,
        operation_id: &str,
        frozen_intent: FrozenSignIntent,
    ) -> SignResult<SignResponse> {
        self.service
            .resume_or_start(
                operation_id,
                frozen_intent,
                self.context.clone(),
                self.request_timeout,
            )
            .await
    }

    /// Recovers a timed-out request through persisted state and provider query.
    pub async fn recover(&self, request: SignRequest) -> SignResult<SigningRequestStatus> {
        self.service.recover(request, self.context.clone()).await
    }

    /// Loads and queries an original persisted request without rebuilding or re-signing it.
    pub async fn recover_operation(&self, request_id: &str) -> SignResult<SigningRequestStatus> {
        self.service
            .recover_operation(request_id, self.context.clone())
            .await
    }

    /// Resolves an account and purpose to its configured immutable key reference.
    pub fn resolve_key(&self, account: &str, purpose: &SignPurpose) -> SignResult<KeyRef> {
        self.key_resolver.resolve(account, purpose)
    }

    /// Returns the exact default provider selected at startup.
    pub fn default_provider(&self) -> &str {
        &self.default_provider
    }

    /// Returns the immutable policy version selected at startup.
    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }

    /// Returns the per-request timeout.
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// Returns the stable first deadline for an operation without refreshing it.
    pub fn deadline_for(&self, operation_id: &str, now_unix_ms: i64) -> SignResult<i64> {
        self.deadlines
            .resolve(operation_id, now_unix_ms, self.request_timeout)
    }
}

const DEFAULT_RECOVERY_QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Default fail-closed implementation of [`SignerService`].
pub struct DefaultSignerService {
    providers: Arc<SignerProviderRegistry>,
    store: Arc<dyn SigningStore>,
    policy: Arc<SigningPolicy>,
    clock: Arc<dyn TimeSource>,
}

impl DefaultSignerService {
    /// Creates a service using the current process time provider.
    pub fn new(
        provider: Arc<dyn SignerProvider>,
        store: Arc<dyn SigningStore>,
        policy: Arc<SigningPolicy>,
    ) -> Self {
        Self::with_clock(provider, store, policy, TimeProvider::current())
    }

    /// Creates a service with an explicit clock for deterministic integration.
    pub fn with_clock(
        provider: Arc<dyn SignerProvider>,
        store: Arc<dyn SigningStore>,
        policy: Arc<SigningPolicy>,
        clock: Arc<dyn TimeSource>,
    ) -> Self {
        let mut registry = SignerProviderRegistry::new();
        registry
            .register(provider)
            .expect("single provider registration must be valid");
        Self::with_registry_and_clock(Arc::new(registry), store, policy, clock)
    }

    /// Creates a service backed by a strict multi-provider registry.
    pub fn with_registry(
        providers: Arc<SignerProviderRegistry>,
        store: Arc<dyn SigningStore>,
        policy: Arc<SigningPolicy>,
    ) -> Self {
        Self::with_registry_and_clock(providers, store, policy, TimeProvider::current())
    }

    /// Creates a registry-backed service with an explicit deterministic clock.
    pub fn with_registry_and_clock(
        providers: Arc<SignerProviderRegistry>,
        store: Arc<dyn SigningStore>,
        policy: Arc<SigningPolicy>,
        clock: Arc<dyn TimeSource>,
    ) -> Self {
        Self {
            providers,
            store,
            policy,
            clock,
        }
    }

    fn remaining_timeout(&self, request: &SignRequest) -> SignResult<Duration> {
        let now = self.clock.utc_now_timestamp_millis();
        let remaining = request.deadline_unix_ms.saturating_sub(now);
        if remaining <= 0 {
            return Err(SignError::new(
                SignErrorCode::DeadlineExceeded,
                "signing request deadline has elapsed",
            )
            .for_request(request.request_id.clone()));
        }
        Ok(Duration::from_millis(remaining as u64))
    }

    fn verify_provider_signature(
        request: &SignRequest,
        key_info: &KeyInfo,
        signature: &ProviderSignature,
    ) -> SignResult<()> {
        if signature.algorithm != key_info.algorithm
            || signature.algorithm != ALGORITHM_ECDSA_P256_SHA256
            || signature.signature.len() != 64
        {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "provider returned empty, mismatched, or unsupported signature material",
            )
            .for_request(request.request_id.clone()));
        }
        let raw_signature: [u8; 64] = signature.signature.as_slice().try_into().map_err(|_| {
            SignError::new(
                SignErrorCode::VerificationFailed,
                "provider returned an invalid P-256 signature length",
            )
            .for_request(request.request_id.clone())
        })?;
        let verified = match &request.payload {
            SignPayload::Canonical { bytes, .. } => {
                let digest = neo_crypto::Crypto::sha256(bytes);
                Secp256r1Crypto::verify_prehash(&digest, &raw_signature, &key_info.public_key)
            }
            SignPayload::Digest { digest, .. } => {
                Secp256r1Crypto::verify_prehash(digest, &raw_signature, &key_info.public_key)
            }
        }
        .unwrap_or(false);
        if !verified {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "provider signature failed cryptographic verification",
            )
            .for_request(request.request_id.clone()));
        }
        Ok(())
    }

    async fn audit(
        &self,
        request: &SignRequest,
        context: &AuthenticatedContext,
        request_digest: Option<[u8; 32]>,
        outcome: AuditOutcome,
        reason: Option<String>,
    ) -> SignResult<()> {
        self.store
            .append_audit(SigningAuditEvent {
                schema_version: SIGNING_SCHEMA_VERSION,
                timestamp_unix_ms: self.clock.utc_now_timestamp_millis(),
                request_id: request.request_id.clone(),
                principal: context.principal.clone(),
                tenant: context.tenant.clone(),
                key_ref: request.key_ref.clone(),
                network: request.network,
                purpose: request.purpose.clone(),
                policy_version: request.policy_version.clone(),
                request_digest,
                outcome,
                reason,
                attributes: BTreeMap::new(),
            })
            .await
    }

    async fn recover_loaded(
        &self,
        operation: PersistedSigningOperation,
        context: &AuthenticatedContext,
        query_timeout: Duration,
    ) -> SignResult<SigningRequestStatus> {
        if query_timeout.is_zero() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing recovery query timeout must be positive",
            )
            .for_request(operation.request.request_id));
        }
        let persisted_digest = operation.request.binding_digest(&operation.context)?;
        if persisted_digest != operation.request_digest {
            return Err(SignError::new(
                SignErrorCode::CorruptState,
                "persisted signing request no longer matches its durable authorization digest",
            )
            .for_request(operation.request.request_id));
        }
        if &operation.context != context {
            return Err(SignError::new(
                SignErrorCode::Unauthenticated,
                "recovery identity and scopes do not match the original authorization",
            )
            .for_request(operation.request.request_id));
        }
        match operation.status {
            SigningRequestStatus::Succeeded(response) => {
                return Ok(SigningRequestStatus::Succeeded(response));
            }
            SigningRequestStatus::Failed(error) => {
                return Ok(SigningRequestStatus::Failed(error));
            }
            SigningRequestStatus::NotFound => return Ok(SigningRequestStatus::NotFound),
            SigningRequestStatus::Pending => {}
        }

        let request = operation.request;
        let request_digest = operation.request_digest;
        let key_info = operation.key_info;
        if key_info.schema_version != SIGNING_SCHEMA_VERSION
            || key_info.key_ref != request.key_ref
            || !key_info.enabled
        {
            return Err(SignError::new(
                SignErrorCode::CorruptState,
                "persisted historical key binding is incomplete or mismatched",
            )
            .for_request(request.request_id));
        }
        let provider = self.providers.provider_for(&request.key_ref)?;
        let capabilities = provider.capabilities();
        if capabilities.provider != request.key_ref.provider
            || capabilities.security_level != key_info.security_level
        {
            return Err(SignError::new(
                SignErrorCode::CapabilityUnavailable,
                "persisted request provider or security level no longer matches the registered provider",
            )
            .for_request(request.request_id));
        }
        let authorized = AuthorizedSignRequest {
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            network: request.network,
            purpose: request.purpose.clone(),
            protocol_context: request.protocol_context.clone(),
            payload: request.payload.clone(),
            signed_digest: request.payload.signing_digest(),
            request_digest,
            policy_version: request.policy_version.clone(),
        };
        let queried =
            match tokio::time::timeout(query_timeout, provider.query_authorized(&authorized)).await
            {
                Ok(result) => {
                    result.map_err(|error| error.for_request(request.request_id.clone()))?
                }
                Err(_) => {
                    let error = SignError::new(
                        SignErrorCode::ResultUnknown,
                        "provider status query timed out; the original operation remains pending",
                    )
                    .for_request(request.request_id.clone())
                    .retryable(true);
                    self.audit(
                        &request,
                        context,
                        Some(request_digest),
                        AuditOutcome::ResultUnknown,
                        Some("recovery_query_timeout".to_string()),
                    )
                    .await?;
                    return Err(error);
                }
            };
        match queried {
            ProviderOperationStatus::Pending | ProviderOperationStatus::NotFound => {
                Ok(SigningRequestStatus::Pending)
            }
            ProviderOperationStatus::Failed(error) => {
                let error = error.for_request(request.request_id.clone());
                self.store.fail(&request.request_id, error.clone()).await?;
                Ok(SigningRequestStatus::Failed(error))
            }
            ProviderOperationStatus::Succeeded(signature) => {
                if let Err(error) = Self::verify_provider_signature(&request, &key_info, &signature)
                {
                    if capabilities.has_remote_outcome_uncertainty() {
                        self.audit(
                            &request,
                            context,
                            Some(request_digest),
                            AuditOutcome::ResultUnknown,
                            Some("recovered_remote_signature_unverified".to_string()),
                        )
                        .await?;
                        return Ok(SigningRequestStatus::Pending);
                    }
                    self.store.fail(&request.request_id, error.clone()).await?;
                    return Ok(SigningRequestStatus::Failed(error));
                }
                let response = SignResponse {
                    schema_version: SIGNING_SCHEMA_VERSION,
                    request_id: request.request_id.clone(),
                    key_ref: request.key_ref.clone(),
                    algorithm: signature.algorithm,
                    signature: signature.signature,
                    signed_digest: request.payload.signing_digest(),
                    request_digest,
                    policy_version: request.policy_version.clone(),
                    completed_at_unix_ms: self.clock.utc_now_timestamp_millis(),
                    idempotent_replay: false,
                };
                self.store
                    .complete(&request.request_id, response.clone())
                    .await?;
                self.audit(
                    &request,
                    context,
                    Some(request_digest),
                    AuditOutcome::Signed,
                    Some("recovered_provider_result".to_string()),
                )
                .await?;
                Ok(SigningRequestStatus::Succeeded(response))
            }
        }
    }
}

#[async_trait]
impl SignerService for DefaultSignerService {
    async fn sign(
        &self,
        request: SignRequest,
        context: AuthenticatedContext,
    ) -> SignResult<SignResponse> {
        request.validate(&context)?;
        let now = self.clock.utc_now_timestamp_millis();
        if request.deadline_unix_ms <= now {
            let error = SignError::new(
                SignErrorCode::DeadlineExceeded,
                "signing request deadline has elapsed",
            )
            .for_request(request.request_id.clone());
            self.audit(
                &request,
                &context,
                None,
                AuditOutcome::Failed,
                Some("deadline_exceeded".to_string()),
            )
            .await?;
            return Err(error);
        }

        let request_digest = request.binding_digest(&context)?;
        let provider = match self.providers.provider_for(&request.key_ref) {
            Ok(provider) => provider,
            Err(error) => {
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::Denied,
                    Some("provider_not_registered".to_string()),
                )
                .await?;
                return Err(error.for_request(request.request_id.clone()));
            }
        };
        let capabilities = provider.capabilities();
        if capabilities.provider != request.key_ref.provider {
            let error = SignError::new(
                SignErrorCode::CapabilityUnavailable,
                "request selected a provider that is not registered for the key reference",
            )
            .for_request(request.request_id.clone());
            self.audit(
                &request,
                &context,
                Some(request_digest),
                AuditOutcome::Denied,
                Some("provider_mismatch".to_string()),
            )
            .await?;
            return Err(error);
        }
        let key_info_result = tokio::time::timeout(
            self.remaining_timeout(&request)?,
            provider.key_info(&request.key_ref),
        )
        .await;
        let key_info = match key_info_result {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => {
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::Failed,
                    Some(format!("{:?}", error.code)),
                )
                .await?;
                return Err(error.for_request(request.request_id.clone()));
            }
            Err(_) => {
                let error = SignError::new(
                    SignErrorCode::DeadlineExceeded,
                    "provider key lookup exceeded the request deadline before signing dispatch",
                )
                .for_request(request.request_id.clone());
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::Failed,
                    Some("key_lookup_timeout".to_string()),
                )
                .await?;
                return Err(error);
            }
        };
        if key_info.security_level != capabilities.security_level
            || !capabilities.supports(&request.purpose, &request.payload, &key_info.algorithm)
        {
            let error = SignError::new(
                SignErrorCode::CapabilityUnavailable,
                "selected provider does not explicitly support the requested purpose, encoding, algorithm, and security level",
            )
            .for_request(request.request_id.clone());
            self.audit(
                &request,
                &context,
                Some(request_digest),
                AuditOutcome::Denied,
                Some("capability_mismatch".to_string()),
            )
            .await?;
            return Err(error);
        }
        let decision = match self.policy.evaluate(&request, &context, &key_info) {
            Ok(decision) => decision,
            Err(error) => {
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::Denied,
                    Some(format!("{:?}", error.code)),
                )
                .await?;
                return Err(error);
            }
        };
        self.audit(
            &request,
            &context,
            Some(request_digest),
            AuditOutcome::Authorized,
            None,
        )
        .await?;

        let begin = self
            .store
            .begin(BeginSigningRequest {
                request: request.clone(),
                context: context.clone(),
                request_digest,
                key_info: key_info.clone(),
                quota: decision.quota,
                now_unix_ms: now,
            })
            .await?;
        if let BeginSigningOutcome::Cached(mut response) = begin {
            response.idempotent_replay = true;
            self.audit(
                &request,
                &context,
                Some(request_digest),
                AuditOutcome::IdempotentReplay,
                None,
            )
            .await?;
            return Ok(response);
        }

        let signed_digest = request.payload.signing_digest();
        let authorized = AuthorizedSignRequest {
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            network: request.network,
            purpose: request.purpose.clone(),
            protocol_context: request.protocol_context.clone(),
            payload: request.payload.clone(),
            signed_digest,
            request_digest,
            policy_version: decision.policy_version.clone(),
        };
        let sign_result = tokio::time::timeout(
            self.remaining_timeout(&request)?,
            provider.sign_authorized(&authorized),
        )
        .await;
        let provider_signature = match sign_result {
            Ok(Ok(signature)) => signature,
            Ok(Err(error)) => {
                let error = error.for_request(request.request_id.clone());
                if error.code == SignErrorCode::ResultUnknown {
                    self.audit(
                        &request,
                        &context,
                        Some(request_digest),
                        AuditOutcome::ResultUnknown,
                        Some("provider_result_unknown".to_string()),
                    )
                    .await?;
                    return Err(error.retryable(true));
                }
                self.store.fail(&request.request_id, error.clone()).await?;
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::Failed,
                    Some(format!("{:?}", error.code)),
                )
                .await?;
                return Err(error);
            }
            Err(_) => {
                let error = SignError::new(
                    SignErrorCode::ResultUnknown,
                    "provider signing exceeded the deadline; the signing result is unknown",
                )
                .for_request(request.request_id.clone())
                .retryable(true);
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::ResultUnknown,
                    Some("provider_timeout".to_string()),
                )
                .await?;
                return Err(error);
            }
        };

        if let Err(error) =
            Self::verify_provider_signature(&request, &key_info, &provider_signature)
        {
            if capabilities.has_remote_outcome_uncertainty() {
                let unknown = SignError::new(
                    SignErrorCode::ResultUnknown,
                    format!(
                        "remote provider returned signature material that could not be locally verified: {}",
                        error.message
                    ),
                )
                .for_request(request.request_id.clone())
                .retryable(true);
                self.audit(
                    &request,
                    &context,
                    Some(request_digest),
                    AuditOutcome::ResultUnknown,
                    Some("remote_signature_unverified".to_string()),
                )
                .await?;
                return Err(unknown);
            }
            self.store.fail(&request.request_id, error.clone()).await?;
            self.audit(
                &request,
                &context,
                Some(request_digest),
                AuditOutcome::Failed,
                Some("signature_verification_failed".to_string()),
            )
            .await?;
            return Err(error);
        }

        let response = SignResponse {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            algorithm: provider_signature.algorithm,
            signature: provider_signature.signature,
            signed_digest,
            request_digest,
            policy_version: decision.policy_version,
            completed_at_unix_ms: self.clock.utc_now_timestamp_millis(),
            idempotent_replay: false,
        };
        self.store
            .complete(&request.request_id, response.clone())
            .await?;
        self.audit(
            &request,
            &context,
            Some(request_digest),
            AuditOutcome::Signed,
            None,
        )
        .await?;
        Ok(response)
    }

    async fn resume_or_start(
        &self,
        operation_id: &str,
        frozen_intent: FrozenSignIntent,
        context: AuthenticatedContext,
        request_timeout: Duration,
    ) -> SignResult<SignResponse> {
        if operation_id.trim().is_empty() || request_timeout.is_zero() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "resume-or-start requires an operation id and positive query timeout",
            ));
        }

        if let Some(operation) = self.store.load(operation_id).await? {
            frozen_intent.verify_existing(operation_id, &operation.request)?;
            return match self
                .recover_loaded(operation, &context, request_timeout)
                .await?
            {
                SigningRequestStatus::Succeeded(response) => Ok(response),
                SigningRequestStatus::Failed(error) => Err(error),
                SigningRequestStatus::Pending => Err(SignError::new(
                    SignErrorCode::RequestInProgress,
                    "the original signing operation remains pending after provider query",
                )
                .for_request(operation_id.to_string())
                .retryable(true)),
                SigningRequestStatus::NotFound => Err(SignError::new(
                    SignErrorCode::CorruptState,
                    "a loaded signing operation unexpectedly became missing",
                )
                .for_request(operation_id.to_string())),
            };
        }

        let now = self.clock.utc_now_timestamp_millis();
        let timeout_ms = request_timeout.as_millis().min(i64::MAX as u128) as i64;
        let request = frozen_intent
            .clone()
            .into_request(operation_id, now.saturating_add(timeout_ms))?;
        match self.sign(request, context.clone()).await {
            Ok(response) => Ok(response),
            Err(error)
                if matches!(
                    error.code,
                    SignErrorCode::RequestInProgress | SignErrorCode::IdempotencyConflict
                ) =>
            {
                let operation = self.store.load(operation_id).await?.ok_or(error)?;
                frozen_intent.verify_existing(operation_id, &operation.request)?;
                match self
                    .recover_loaded(operation, &context, request_timeout)
                    .await?
                {
                    SigningRequestStatus::Succeeded(response) => Ok(response),
                    SigningRequestStatus::Failed(error) => Err(error),
                    SigningRequestStatus::Pending => Err(SignError::new(
                        SignErrorCode::RequestInProgress,
                        "the original signing operation remains pending after provider query",
                    )
                    .for_request(operation_id.to_string())
                    .retryable(true)),
                    SigningRequestStatus::NotFound => Err(SignError::new(
                        SignErrorCode::CorruptState,
                        "a raced signing operation unexpectedly became missing",
                    )
                    .for_request(operation_id.to_string())),
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn recover(
        &self,
        request: SignRequest,
        context: AuthenticatedContext,
    ) -> SignResult<SigningRequestStatus> {
        request.validate(&context)?;
        let Some(operation) = self.store.load(&request.request_id).await? else {
            return Ok(SigningRequestStatus::NotFound);
        };
        if operation.request != request || operation.context != context {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "recovery request does not match the complete first persisted request",
            )
            .for_request(operation.request.request_id));
        }
        self.recover_loaded(operation, &context, DEFAULT_RECOVERY_QUERY_TIMEOUT)
            .await
    }

    async fn recover_operation(
        &self,
        request_id: &str,
        context: AuthenticatedContext,
    ) -> SignResult<SigningRequestStatus> {
        let Some(operation) = self.store.load(request_id).await? else {
            return Ok(SigningRequestStatus::NotFound);
        };
        self.recover_loaded(operation, &context, DEFAULT_RECOVERY_QUERY_TIMEOUT)
            .await
    }
}
