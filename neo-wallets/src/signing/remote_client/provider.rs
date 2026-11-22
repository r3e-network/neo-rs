use super::session::{AttestedSession, ConfiguredKey, RemoteAttestationRuntime};
use super::*;
/// Strict adapter from the unified signer service to a dedicated remote signer.
pub struct RemoteSignerProvider {
    capabilities: ProviderCapabilities,
    service_identity: String,
    policy_version: String,
    keys: BTreeMap<KeyRef, ConfiguredKey>,
    pub(super) transport: Arc<dyn RemoteSignerTransport>,
    attestation: Option<RemoteAttestationRuntime>,
    pub(super) sessions: RwLock<BTreeMap<KeyRef, AttestedSession>>,
}

impl RemoteSignerProvider {
    pub fn new(
        provider: &SignerProviderSection,
        policy_version: &str,
        transport: Arc<dyn RemoteSignerTransport>,
        verifier: Option<Arc<dyn RemoteAttestationVerifier>>,
    ) -> Result<Self> {
        let remote = provider
            .remote
            .as_ref()
            .context("validated remote signer has no remote configuration")?;
        let mut keys = BTreeMap::new();
        for key in &provider.keys {
            keys.insert(
                KeyRef::new(&provider.id, &key.key_id, key.version),
                ConfiguredKey {
                    public_key: hex::decode(&key.public_key_hex)?,
                    account: key.account.clone(),
                },
            );
        }
        let attestation = match provider.kind {
            SignerProviderKind::RemoteTee => {
                let policy = remote
                    .attestation
                    .clone()
                    .context("remote TEE requires attestation policy")?;
                if policy.max_challenge_age_ms == 0
                    || policy.max_session_age_ms == 0
                    || policy.max_clock_skew_ms > MAX_ATTESTATION_CLOCK_SKEW_MS
                {
                    bail!(
                        "remote TEE freshness policy requires positive challenge/session ages and clock skew no greater than {MAX_ATTESTATION_CLOCK_SKEW_MS} ms"
                    );
                }
                Some(RemoteAttestationRuntime {
                    policy,
                    verifier: verifier.context(
                        "remote TEE verifier is unavailable; refusing to substitute parsed or simulated evidence",
                    )?,
                })
            }
            SignerProviderKind::Remote => {
                if verifier.is_some() {
                    bail!("plain remote signer must not be promoted to a hardware TEE provider");
                }
                None
            }
            _ => bail!("remote signer adapter received a local provider kind"),
        };
        Ok(Self {
            capabilities: ProviderCapabilities {
                provider: provider.id.clone(),
                purposes: provider.purposes.iter().cloned().collect(),
                canonical_encodings: provider.canonical_encodings.iter().cloned().collect(),
                algorithms: provider.algorithms.iter().cloned().collect(),
                security_level: provider
                    .security_level
                    .context("validated remote signer has no security level")?,
                outcome_provenance: ProviderOutcomeProvenance::RemoteUncertain,
            },
            service_identity: remote.service_identity.clone(),
            policy_version: policy_version.to_string(),
            keys,
            transport,
            attestation,
            sessions: RwLock::new(BTreeMap::new()),
        })
    }

    fn configured_key(&self, key_ref: &KeyRef) -> SignResult<&ConfiguredKey> {
        self.keys.get(key_ref).ok_or_else(|| {
            SignError::new(
                SignErrorCode::KeyNotFound,
                "remote signer key is not explicitly configured",
            )
        })
    }

    async fn session_for(&self, key_ref: &KeyRef) -> SignResult<Option<AttestedSession>> {
        let Some(runtime) = &self.attestation else {
            return Ok(None);
        };
        let now = now_unix_ms();
        if let Some(session) = self.sessions.read().get(key_ref).cloned()
            && session.is_valid_for(now, &self.service_identity, &self.policy_version, key_ref)
        {
            return Ok(Some(session));
        }
        self.sessions.write().remove(key_ref);

        let mut challenge_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut challenge_bytes);
        let challenge = AttestationChallenge {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            challenge: challenge_bytes,
            issued_at_unix_ms: now,
            key_ref: key_ref.clone(),
            service_identity: self.service_identity.clone(),
            policy_version: self.policy_version.clone(),
        };
        let evidence = self.transport.attest(&challenge).await?;
        validate_remote_schema(evidence.schema_version)?;
        let measurement = hex::decode(&runtime.policy.measurement_hex).map_err(|error| {
            provider_error(format!(
                "invalid configured attestation measurement: {error}"
            ))
        })?;
        let signer_measurement = runtime
            .policy
            .signer_measurement_hex
            .as_deref()
            .map(hex::decode)
            .transpose()
            .map_err(|error| {
                provider_error(format!("invalid configured signer measurement: {error}"))
            })?;
        let expected = AttestationExpectations {
            challenge: challenge_bytes,
            challenge_issued_at_unix_ms: now,
            max_challenge_age_ms: runtime.policy.max_challenge_age_ms,
            max_clock_skew_ms: runtime.policy.max_clock_skew_ms,
            max_session_age_ms: runtime.policy.max_session_age_ms,
            service_identity: self.service_identity.clone(),
            policy_version: self.policy_version.clone(),
            key_ref: key_ref.clone(),
            measurement,
            signer_measurement,
            product_id: runtime.policy.product_id,
            minimum_security_version: runtime.policy.minimum_security_version,
        };
        let response_received_at = now_unix_ms();
        Self::validate_evidence_fields(&evidence, &expected, response_received_at)?;
        let verified = runtime
            .verifier
            .verify(&evidence, &expected, response_received_at)?;
        Self::validate_verified_claims(&evidence, &expected, &verified)?;
        let session = AttestedSession {
            session_id: evidence.session_id,
            session_public_key: evidence.session_public_key,
            service_identity: evidence.service_identity,
            policy_version: evidence.policy_version,
            key_ref: evidence.key_ref,
            expires_at_unix_ms: evidence.expires_at_unix_ms,
        };
        self.sessions
            .write()
            .insert(key_ref.clone(), session.clone());
        Ok(Some(session))
    }

    pub(super) fn validate_evidence_fields(
        evidence: &AttestationEvidence,
        expected: &AttestationExpectations,
        now: i64,
    ) -> SignResult<()> {
        let max_challenge_age = i64::try_from(expected.max_challenge_age_ms).unwrap_or(i64::MAX);
        let max_clock_skew = i64::try_from(expected.max_clock_skew_ms).unwrap_or(i64::MAX);
        let max_session_age = i64::try_from(expected.max_session_age_ms).unwrap_or(i64::MAX);
        let max_evidence_age = max_challenge_age.saturating_add(max_clock_skew);
        let earliest_evidence_time = expected
            .challenge_issued_at_unix_ms
            .saturating_sub(max_clock_skew);
        let latest_issued_at = now.saturating_add(max_clock_skew);
        if evidence.challenge != expected.challenge
            || evidence.service_identity != expected.service_identity
            || evidence.policy_version != expected.policy_version
            || evidence.key_ref != expected.key_ref
            || evidence.session_id.trim().is_empty()
            || evidence.session_public_key.len() != 33
            || evidence.evidence.is_empty()
            || expected.challenge_issued_at_unix_ms > latest_issued_at
            || evidence.issued_at_unix_ms < earliest_evidence_time
            || evidence.issued_at_unix_ms > latest_issued_at
            || now.saturating_sub(expected.challenge_issued_at_unix_ms) > max_challenge_age
            || now.saturating_sub(evidence.issued_at_unix_ms) > max_evidence_age
            || now.saturating_sub(evidence.issued_at_unix_ms) > max_session_age
            || evidence.expires_at_unix_ms <= evidence.issued_at_unix_ms
            || evidence.expires_at_unix_ms <= now
            || evidence.expires_at_unix_ms
                > evidence.issued_at_unix_ms.saturating_add(max_session_age)
        {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "remote attestation challenge, session, identity, policy, key, or freshness binding failed",
            ));
        }
        Ok(())
    }

    fn validate_verified_claims(
        evidence: &AttestationEvidence,
        expected: &AttestationExpectations,
        verified: &VerifiedAttestation,
    ) -> SignResult<()> {
        let binding = evidence.binding_digest()?;
        if verified.report_data != binding
            || verified.measurement != expected.measurement
            || expected
                .signer_measurement
                .as_ref()
                .is_some_and(|value| value != &verified.signer_measurement)
            || verified.product_id != expected.product_id
            || verified.security_version < expected.minimum_security_version
            || verified.debug
            || verified.tcb_status != TcbStatus::UpToDate
            || verified.collateral_status != CollateralStatus::Valid
            || verified.revoked
        {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "remote hardware evidence failed measurement, product, SVN, debug, TCB, collateral, revocation, or report-data checks",
            ));
        }
        Ok(())
    }

    fn validate_key_info(&self, response: RemoteKeyInfoResponse) -> SignResult<KeyInfo> {
        validate_remote_schema(response.schema_version)?;
        if response.service_identity != self.service_identity {
            return Err(SignError::new(
                SignErrorCode::Unauthenticated,
                "remote signer service identity changed",
            ));
        }
        let configured = self.configured_key(&response.key_info.key_ref)?;
        if response.key_info.schema_version != REMOTE_SIGNER_PROTOCOL_VERSION
            || response.key_info.public_key != configured.public_key
            || response.key_info.algorithm != ALGORITHM_ECDSA_P256_SHA256
            || response.key_info.security_level != self.capabilities.security_level
            || !response.key_info.enabled
        {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "remote key metadata does not match the configured immutable key",
            ));
        }
        Ok(response.key_info)
    }

    pub(super) fn validate_response(
        &self,
        request: &RemoteSignRequest,
        response: &RemoteSignResponse,
        session: Option<&AttestedSession>,
    ) -> SignResult<()> {
        validate_remote_schema(response.schema_version)?;
        if response.request_id != request.request_id
            || response.key_ref != request.key_ref
            || response.request_digest != request.request_digest
            || response.policy_version != self.policy_version
            || response.service_identity != self.service_identity
        {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "remote signing response identity or request binding changed",
            ));
        }
        match session {
            Some(session) => {
                if !session.is_valid_for(
                    now_unix_ms(),
                    &self.service_identity,
                    &self.policy_version,
                    &request.key_ref,
                ) || response.attestation_session_id.as_deref() != Some(&session.session_id)
                {
                    return Err(SignError::new(
                        SignErrorCode::VerificationFailed,
                        "attested signing session expired or changed identity",
                    ));
                }
                let signature = response.session_signature.as_deref().ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::Unauthenticated,
                        "attested response is missing session authentication",
                    )
                })?;
                let signature: [u8; 64] = signature.try_into().map_err(|_| {
                    SignError::new(
                        SignErrorCode::Unauthenticated,
                        "attested response session signature has invalid length",
                    )
                })?;
                if !Secp256r1Crypto::verify_prehash(
                    &response.auth_digest()?,
                    &signature,
                    &session.session_public_key,
                )
                .unwrap_or(false)
                {
                    return Err(SignError::new(
                        SignErrorCode::Unauthenticated,
                        "attested response session authentication failed",
                    ));
                }
            }
            None if response.attestation_session_id.is_some()
                || response.session_signature.is_some() =>
            {
                return Err(SignError::new(
                    SignErrorCode::VerificationFailed,
                    "plain remote signer returned an unconfigured TEE session",
                ));
            }
            None => {}
        }
        Ok(())
    }

    fn validate_rejection(
        &self,
        request: &RemoteSignRequest,
        rejection: &RemoteSignRejection,
        session: Option<&AttestedSession>,
    ) -> SignResult<()> {
        validate_remote_schema(rejection.schema_version).map_err(|error| {
            SignError::new(
                SignErrorCode::ResultUnknown,
                format!("remote rejection schema could not be authenticated: {error}"),
            )
            .retryable(true)
        })?;
        if !rejection.permanent
            || !is_permanent_rejection_code(&rejection.code)
            || rejection.request_id != request.request_id
            || rejection.key_ref != request.key_ref
            || rejection.request_digest != request.request_digest
            || rejection.policy_version != self.policy_version
            || rejection.service_identity != self.service_identity
        {
            return Err(SignError::new(
                SignErrorCode::ResultUnknown,
                "remote rejection was not permanent or did not bind the original request",
            )
            .retryable(true));
        }
        match session {
            Some(session) => {
                if !session.is_valid_for(
                    now_unix_ms(),
                    &self.service_identity,
                    &self.policy_version,
                    &request.key_ref,
                ) || rejection.attestation_session_id.as_deref() != Some(&session.session_id)
                {
                    return Err(SignError::new(
                        SignErrorCode::ResultUnknown,
                        "remote rejection was not bound to the active attested session",
                    )
                    .retryable(true));
                }
                let signature = rejection.session_signature.as_deref().ok_or_else(|| {
                    SignError::new(
                        SignErrorCode::ResultUnknown,
                        "remote rejection lacks attested session authentication",
                    )
                    .retryable(true)
                })?;
                let signature: [u8; 64] = signature.try_into().map_err(|_| {
                    SignError::new(
                        SignErrorCode::ResultUnknown,
                        "remote rejection has an invalid session signature",
                    )
                    .retryable(true)
                })?;
                let auth_digest = rejection.auth_digest().map_err(|error| {
                    SignError::new(
                        SignErrorCode::ResultUnknown,
                        format!("remote rejection binding could not be authenticated: {error}"),
                    )
                    .retryable(true)
                })?;
                if !Secp256r1Crypto::verify_prehash(
                    &auth_digest,
                    &signature,
                    &session.session_public_key,
                )
                .unwrap_or(false)
                {
                    return Err(SignError::new(
                        SignErrorCode::ResultUnknown,
                        "remote rejection session authentication failed",
                    )
                    .retryable(true));
                }
            }
            None if rejection.attestation_session_id.is_some()
                || rejection.session_signature.is_some() =>
            {
                return Err(SignError::new(
                    SignErrorCode::ResultUnknown,
                    "plain remote signer returned an unexpected rejection session",
                )
                .retryable(true));
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) async fn query_unknown(
        &self,
        request_id: &str,
        request_digest: [u8; 32],
        key_ref: &KeyRef,
    ) -> SignResult<RemoteOperationStatus> {
        self.configured_key(key_ref)?;
        let session = self.session_for(key_ref).await?;
        let query = RemoteOperationQuery {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            request_digest,
            key_ref: key_ref.clone(),
            attestation_session_id: session.as_ref().map(|value| value.session_id.clone()),
        };
        let result = self.transport.query(&query).await?;
        let binding = RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            key_ref: key_ref.clone(),
            network: 0,
            purpose: crate::signing::SignPurpose::Custom("query".to_string()),
            payload: crate::signing::SignPayload::Digest {
                algorithm: crate::signing::DigestAlgorithm::Sha256,
                digest: [0; 32],
            },
            request_digest,
            policy_version: self.policy_version.clone(),
            attestation_session_id: session.as_ref().map(|value| value.session_id.clone()),
        };
        match &result {
            RemoteOperationStatus::Succeeded { response } => {
                self.validate_response(&binding, response, session.as_ref())?;
            }
            RemoteOperationStatus::Failed { rejection } => {
                self.validate_rejection(&binding, rejection, session.as_ref())?;
            }
            RemoteOperationStatus::Pending | RemoteOperationStatus::NotFound => {}
        }
        Ok(result)
    }
}

#[async_trait]
impl SignerProvider for RemoteSignerProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo> {
        self.configured_key(key_ref)?;
        self.session_for(key_ref).await?;
        let response = self.transport.key_info(key_ref).await?;
        if response.key_info.key_ref != *key_ref {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "remote key lookup returned a different key reference",
            ));
        }
        let mut info = self.validate_key_info(response)?;
        let account = &self.configured_key(key_ref)?.account;
        info.attributes
            .insert("account".to_string(), account.clone());
        Ok(info)
    }

    async fn sign_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature> {
        self.configured_key(request.key_ref())?;
        let session = self.session_for(request.key_ref()).await?;
        let remote_request = RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request.request_id().to_string(),
            key_ref: request.key_ref().clone(),
            network: request.network(),
            purpose: request.purpose().clone(),
            payload: request.payload().clone(),
            request_digest: *request.request_digest(),
            policy_version: request.policy_version().to_string(),
            attestation_session_id: session.as_ref().map(|value| value.session_id.clone()),
        };
        match self.transport.sign(&remote_request).await? {
            RemoteSignDispatchResult::Succeeded(response) => {
                self.validate_response(&remote_request, &response, session.as_ref())
                    .map_err(|error| {
                        SignError::new(
                            SignErrorCode::ResultUnknown,
                            format!("remote signer response could not be authenticated: {error}"),
                        )
                        .retryable(true)
                    })?;
                Ok(response.provider_signature)
            }
            RemoteSignDispatchResult::Rejected(rejection) => {
                self.validate_rejection(&remote_request, &rejection, session.as_ref())?;
                Err(SignError::new(
                    SignErrorCode::ProviderFailure,
                    format!(
                        "remote signer permanently rejected request ({}): {}",
                        rejection.code, rejection.message
                    ),
                ))
            }
        }
    }

    async fn query_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderOperationStatus> {
        match self
            .query_unknown(
                request.request_id(),
                *request.request_digest(),
                request.key_ref(),
            )
            .await?
        {
            RemoteOperationStatus::Pending => Ok(ProviderOperationStatus::Pending),
            RemoteOperationStatus::Succeeded { response } => Ok(
                ProviderOperationStatus::Succeeded(response.provider_signature),
            ),
            RemoteOperationStatus::Failed { rejection } => Ok(ProviderOperationStatus::Failed(
                SignError::new(
                    SignErrorCode::ProviderFailure,
                    format!(
                        "remote signer permanently rejected request ({}): {}",
                        rejection.code, rejection.message
                    ),
                )
                .for_request(request.request_id().to_string()),
            )),
            RemoteOperationStatus::NotFound => Ok(ProviderOperationStatus::NotFound),
        }
    }
}
