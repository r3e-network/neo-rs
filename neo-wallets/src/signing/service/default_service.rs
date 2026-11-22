use super::*;
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
