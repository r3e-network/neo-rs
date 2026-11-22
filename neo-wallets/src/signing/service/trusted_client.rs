use super::*;
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
