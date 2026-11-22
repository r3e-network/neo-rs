use super::*;
impl SignerService {
    /// Builds the service: seals or unseals the signing key, initializes the
    /// neo-tee attestation service in simulation mode, replays the durable
    /// operation journal, and loads the client-authorization policy.
    pub fn new(config: SignerServerConfig) -> anyhow::Result<Self> {
        let authorization = ClientAuthorizationPolicy::load(&config)
            .context("failed to load the client-authorization policy")?;
        Self::with_client_authorization(config, authorization)
    }

    /// Builds the service with an explicit identity-scoping policy instead of
    /// the one discovered from the environment or the sealed-data directory.
    pub fn with_client_authorization(
        config: SignerServerConfig,
        authorization: ClientAuthorizationPolicy,
    ) -> anyhow::Result<Self> {
        let label = format!("{}:{}", config.key_ref.provider, config.key_ref.key_id);
        let key = SealedSigningKey::load_or_generate(
            &config.sealed_data_dir,
            &config.sealed_key_path,
            &label,
        )?;
        let enclave: std::sync::Arc<TeeEnclave> = key.enclave().clone();
        // Simulation evidence: allowed here because this is the reference
        // implementation. A production deployment generates hardware quotes.
        let attestation_config = AttestationConfig {
            allow_simulated: true,
            ..AttestationConfig::testing()
        };
        let attestation = AttestationService::with_config(enclave, attestation_config)?;
        let journal = Journal::open(&config.sealed_data_dir)
            .context("failed to open the durable operation journal")?;
        Ok(Self {
            config,
            key,
            attestation,
            authorization,
            sessions: Mutex::new(BTreeMap::new()),
            used_challenges: Mutex::new(BTreeMap::new()),
            journal: Mutex::new(journal),
            signatures_issued: AtomicUsize::new(0),
            attestation_sessions_issued: AtomicUsize::new(0),
        })
    }

    /// Configuration served by this instance.
    #[must_use]
    pub fn config(&self) -> &SignerServerConfig {
        &self.config
    }

    /// Effective client-identity authorization policy.
    #[must_use]
    pub fn client_authorization(&self) -> &ClientAuthorizationPolicy {
        &self.authorization
    }

    /// Number of operations currently journaled (recovered on boot included).
    #[must_use]
    pub fn journaled_operations(&self) -> usize {
        self.journal.lock().len()
    }

    /// Path of the durable operation journal.
    #[must_use]
    pub fn journal_path(&self) -> std::path::PathBuf {
        self.journal.lock().path().to_path_buf()
    }

    /// Authorizes an authenticated client identity for one key and purpose.
    ///
    /// Called by the transport before every endpoint so unauthorized requests
    /// are refused with HTTP 403, and again inside the endpoint handlers so a
    /// new endpoint cannot forget the check.
    pub fn authorize(
        &self,
        identity: Option<&ClientIdentity>,
        key_ref: &KeyRef,
        purpose: Option<&SignPurpose>,
    ) -> SignResult<()> {
        let Some(identity) = identity else {
            return Err(SignError::new(
                SignErrorCode::PolicyDenied,
                "the request carries no authenticated client identity",
            ));
        };
        if self
            .authorization
            .is_authorized(identity.common_name(), key_ref, purpose)
        {
            return Ok(());
        }
        let scope = purpose.map_or_else(String::new, |purpose| {
            format!(" for purpose '{}'", purpose.required_scope())
        });
        Err(SignError::new(
            SignErrorCode::PolicyDenied,
            format!(
                "client identity '{}' is not authorized for key '{}'{scope}",
                identity.common_name(),
                key_selector(key_ref)
            ),
        ))
    }

    /// Number of times the sealed key actually produced a signature. Used by
    /// integration tests to assert no double-signing.
    #[must_use]
    pub fn signatures_issued(&self) -> usize {
        self.signatures_issued.load(Ordering::SeqCst)
    }

    /// Number of attestation sessions created.
    #[must_use]
    pub fn attestation_sessions_issued(&self) -> usize {
        self.attestation_sessions_issued.load(Ordering::SeqCst)
    }

    /// The compressed public key of the sealed signing key.
    #[must_use]
    pub fn public_key(&self) -> Vec<u8> {
        self.key.public_key().to_vec()
    }

    /// `POST /v1/signer/key-info`.
    pub fn key_info(
        &self,
        identity: &ClientIdentity,
        key_ref: &KeyRef,
    ) -> SignResult<RemoteKeyInfoResponse> {
        self.authorize(Some(identity), key_ref, None)?;
        validate_remote_schema(key_ref.schema_version)?;
        if *key_ref != self.config.key_ref {
            return Err(SignError::new(
                SignErrorCode::KeyNotFound,
                "requested key is not served by this signer",
            ));
        }
        Ok(RemoteKeyInfoResponse {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            service_identity: self.config.service_identity.clone(),
            key_info: KeyInfo {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                key_ref: key_ref.clone(),
                algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                public_key: self.public_key(),
                security_level: SecurityLevel::RemoteHardwareTee,
                enabled: true,
                attributes: BTreeMap::new(),
            },
        })
    }

    /// `POST /v1/signer/attest`: consumes a single-use challenge and returns
    /// evidence binding the challenge to a fresh session key.
    ///
    /// The evidence claims (measurement, signer measurement, product id,
    /// security version, debug) are taken from the neo-tee attestation report
    /// generated over [`AttestationEvidence::binding_digest`], so a verifier
    /// that checks the report's report data obtains exactly the binding the
    /// node client requires.
    pub async fn attest(
        &self,
        identity: &ClientIdentity,
        challenge: &AttestationChallenge,
    ) -> SignResult<AttestationEvidence> {
        // A session may only be established for a key this identity may use.
        self.authorize(Some(identity), &challenge.key_ref, None)?;
        validate_remote_schema(challenge.schema_version)?;
        if challenge.service_identity != self.config.service_identity {
            return Err(SignError::new(
                SignErrorCode::Unauthenticated,
                "challenge service identity does not match this signer",
            ));
        }
        if challenge.policy_version != self.config.policy_version {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "challenge policy version does not match this signer",
            ));
        }
        if challenge.key_ref != self.config.key_ref {
            return Err(SignError::new(
                SignErrorCode::KeyNotFound,
                "challenge key is not served by this signer",
            ));
        }

        // Server-side freshness bound, mirroring the node's
        // MAX_ATTESTATION_CLOCK_SKEW_MS acceptance window.
        let now = now_unix_ms();
        if (now - challenge.issued_at_unix_ms).abs() > self.config.max_challenge_age_ms {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "attestation challenge is outside the freshness window",
            ));
        }

        // Challenge nonces are single-use: reject replays.
        {
            let mut used = self.used_challenges.lock();
            used.retain(|_, expires_at| *expires_at > now);
            if used.contains_key(&challenge.challenge) {
                return Err(SignError::new(
                    SignErrorCode::VerificationFailed,
                    "attestation challenge nonce was already used (replay)",
                ));
            }
        }

        if self.config.faults.attest_delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(
                self.config.faults.attest_delay_ms,
            ))
            .await;
        }

        let session_private_key = Zeroizing::new(Secp256r1Crypto::generate_private_key());
        let session_public_key =
            Secp256r1Crypto::derive_public_key(&session_private_key).map_err(|error| {
                SignError::new(
                    SignErrorCode::ProviderFailure,
                    format!("failed to derive the session public key: {error}"),
                )
            })?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let issued_at_unix_ms = now.saturating_add(self.config.faults.attest_clock_skew_ms);
        let expires_at_unix_ms = self.config.session_expires_at_unix_ms(issued_at_unix_ms);

        let mut evidence = AttestationEvidence {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            challenge: challenge.challenge,
            session_id: session_id.clone(),
            session_public_key: session_public_key.clone(),
            service_identity: challenge.service_identity.clone(),
            policy_version: challenge.policy_version.clone(),
            key_ref: challenge.key_ref.clone(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            measurement: Vec::new(),
            signer_measurement: Vec::new(),
            product_id: 0,
            security_version: 0,
            debug: false,
            evidence: Vec::new(),
        };

        // Generate the neo-tee attestation report over the exact binding the
        // node client verifies. The report data is 64 bytes; the binding
        // digest occupies the first 32.
        let binding = evidence.binding_digest()?;
        let report = self
            .attestation
            .generate_report(&binding)
            .map_err(|error| {
                SignError::new(
                    SignErrorCode::ProviderFailure,
                    format!("failed to generate attestation evidence: {error}"),
                )
            })?;
        evidence.measurement = report.mrenclave.to_vec();
        evidence.signer_measurement = report.mrsigner.to_vec();
        evidence.product_id = report.isv_prod_id;
        evidence.security_version = report.isv_svn;
        evidence.debug = report.attributes.debug;
        evidence.evidence = serde_json::to_vec(&report).map_err(|error| {
            SignError::new(
                SignErrorCode::InvalidRequest,
                format!("failed to encode attestation report: {error}"),
            )
        })?;

        self.sessions.lock().insert(
            session_id.clone(),
            SessionRecord {
                session_id,
                session_private_key,
                key_ref: challenge.key_ref.clone(),
                client_common_name: identity.common_name().to_string(),
                expires_at_unix_ms,
            },
        );
        self.used_challenges
            .lock()
            .insert(challenge.challenge, expires_at_unix_ms);
        self.attestation_sessions_issued
            .fetch_add(1, Ordering::SeqCst);
        Ok(evidence)
    }

    /// `POST /v1/signer/sign`: idempotent, journal-first dispatch.
    ///
    /// The dispatch is answered from the durable journal when the request id
    /// was already seen (including before a restart), refuses any consensus
    /// payload that conflicts with an already reserved slot, and only then
    /// reaches the sealed key.
    pub fn sign(&self, identity: &ClientIdentity, request: &RemoteSignRequest) -> SignOutcome {
        if let Err(error) = validate_remote_schema(request.schema_version) {
            return SignOutcome::Ambiguous(format!(
                "unsupported remote signer protocol version: {error}"
            ));
        }

        // A CA-accepted certificate says who the client is, not what it may
        // sign: the identity must be scoped to this key and purpose.
        if let Err(error) = self.authorize(Some(identity), &request.key_ref, Some(&request.purpose))
        {
            return SignOutcome::Unauthorized(error.to_string());
        }

        let slot = ConsensusSlot::from_request(request);
        let mut journal = self.journal.lock();

        // Idempotency: a completed request is replayed verbatim and is never
        // re-signed, even across restarts.
        if let Some(entry) = journal.get(&request.request_id) {
            if entry.request_digest != request.request_digest {
                return SignOutcome::Ambiguous(
                    "request id is already bound to a different request digest".to_string(),
                );
            }
            match &entry.state {
                JournalState::Succeeded(response) => {
                    return SignOutcome::Succeeded(response.clone());
                }
                JournalState::Failed(rejection) => {
                    return SignOutcome::Rejected(rejection.clone());
                }
                JournalState::InFlight { .. } => {
                    return SignOutcome::Ambiguous(
                        "request is still being processed; recover through the query endpoint"
                            .to_string(),
                    );
                }
            }
        }

        if request.key_ref != self.config.key_ref {
            // Without a valid attested session bound to the configured key we
            // cannot produce an authenticated terminal rejection; the
            // ambiguity forces the client onto its recovery path.
            return SignOutcome::Ambiguous("key is not served by this signer".to_string());
        }

        // Every terminal answer must be authenticated by a live attested
        // session: the node rejects unauthenticated or session-mismatched
        // responses, so without a session the only safe answer is ambiguity.
        let Some(session) = self.valid_session(request, identity) else {
            return SignOutcome::Ambiguous(
                "no valid attested session for this request; re-attest and retry".to_string(),
            );
        };

        if let Some((code, message)) = self.permanent_denial(request, &journal) {
            match self.authenticated_rejection(request, &session, code, message) {
                Ok(rejection) => {
                    // Journal the rejection durably too: a repeated dispatch
                    // replays it verbatim instead of re-evaluating the slot.
                    if let Err(error) = journal.record_failed(request, &rejection, None) {
                        tracing::warn!(
                            "failed to journal the permanent rejection for {}: {error}",
                            request.request_id
                        );
                    }
                    return SignOutcome::Rejected(rejection);
                }
                Err(error) => {
                    return SignOutcome::Ambiguous(format!(
                        "failed to authenticate the rejection: {error}"
                    ));
                }
            }
        }

        // Test fault: record the dispatch, do not sign, answer ambiguously.
        // The first query performs the deferred signing and reports Pending.
        if self.config.faults.hold_dispatch {
            if let Err(error) = journal.record_in_flight(request, slot.as_ref()) {
                return SignOutcome::Ambiguous(format!(
                    "failed to record the dispatch durably: {error}"
                ));
            }
            return SignOutcome::Ambiguous(
                "dispatch recorded; processing deferred (fault injection)".to_string(),
            );
        }

        match self.complete_request(&mut journal, request, &session, slot.as_ref()) {
            Ok(response) => SignOutcome::Succeeded(response),
            Err(error) => SignOutcome::Ambiguous(format!("failed to complete signing: {error}")),
        }
    }

    /// `POST /v1/signer/query`: idempotent recovery for an unknown dispatch
    /// outcome. The first query for an in-flight request completes it
    /// (modeling the server's deferred processing) while still reporting
    /// `Pending` for that query; later queries serve the durable result.
    pub fn query(
        &self,
        identity: &ClientIdentity,
        query: &RemoteOperationQuery,
    ) -> SignResult<RemoteOperationStatus> {
        validate_remote_schema(query.schema_version)?;
        self.authorize(Some(identity), &query.key_ref, None)?;
        let mut journal = self.journal.lock();
        let Some(entry) = journal.get_mut(&query.request_id) else {
            return Ok(RemoteOperationStatus::NotFound);
        };
        if entry.request_digest != query.request_digest {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "query digest does not match the recorded request",
            ));
        }
        match &entry.state {
            JournalState::Succeeded(response) => Ok(RemoteOperationStatus::Succeeded {
                response: response.clone(),
            }),
            JournalState::Failed(rejection) => Ok(RemoteOperationStatus::Failed {
                rejection: rejection.clone(),
            }),
            JournalState::InFlight { request } => {
                let request = request.clone();
                let session = request
                    .attestation_session_id
                    .as_deref()
                    .and_then(|session_id| self.live_session(session_id));
                if let Some(session) = session {
                    // Deferred completion: perform the signing now. This
                    // query still reports Pending because the operation had
                    // not completed when it arrived.
                    let slot = ConsensusSlot::from_request(&request);
                    let _ = self.complete_request(&mut journal, &request, &session, slot.as_ref());
                }
                Ok(RemoteOperationStatus::Pending)
            }
        }
    }

    /// Looks up the session named by the request, requiring that it is live,
    /// bound to the requested key and owned by the calling identity.
    fn valid_session(
        &self,
        request: &RemoteSignRequest,
        identity: &ClientIdentity,
    ) -> Option<SessionRecord> {
        let session_id = request.attestation_session_id.as_deref()?;
        let session = self.sessions.lock().get(session_id).cloned()?;
        if !session.is_valid_at(now_unix_ms(), &request.key_ref, identity) {
            return None;
        }
        Some(session)
    }

    /// Looks up a live session by id (used for deferred completion, where the
    /// session key must still authenticate the response).
    fn live_session(&self, session_id: &str) -> Option<SessionRecord> {
        let session = self.sessions.lock().get(session_id).cloned()?;
        if now_unix_ms() >= session.expires_at_unix_ms {
            return None;
        }
        Some(session)
    }

    /// Permanent policy denials for an otherwise well-bound request. Returns
    /// a (code, message) pair using only codes the node client treats as
    /// terminal.
    fn permanent_denial(
        &self,
        request: &RemoteSignRequest,
        journal: &Journal,
    ) -> Option<(&'static str, String)> {
        if request.policy_version != self.config.policy_version {
            return Some((
                "policy_permanent_denial",
                format!(
                    "request policy version '{}' does not match the signer policy version '{}'",
                    request.policy_version, self.config.policy_version
                ),
            ));
        }
        if !self.config.is_purpose_authorized(&request.purpose) {
            return Some((
                "authorization_denied",
                format!(
                    "purpose '{}' is not authorized for this key",
                    request.purpose.required_scope()
                ),
            ));
        }
        // Anti-equivocation: a consensus slot that is already reserved by a
        // different request must never receive a second signature - not now,
        // and not after a restart (the reservation is journaled on disk).
        if let Some(slot) = ConsensusSlot::from_request(request)
            && let Some(owner) = journal.slot_owner(&slot)
            && owner != request.request_id
        {
            return Some((
                "policy_permanent_denial",
                format!(
                    "consensus slot ({}) is already reserved by request '{owner}'; \
                     signing a different payload for the same slot would equivocate",
                    slot.describe()
                ),
            ));
        }
        None
    }

    /// Signs the request payload with the sealed key and authenticates the
    /// response with the attested session key.
    ///
    /// The caller holds the journal lock. The in-flight record is durable
    /// *before* the sealed key is unsealed, and the completed record is durable
    /// *before* the response is handed out, so a crash can never leave the
    /// signer with a signature it does not know it produced.
    fn complete_request(
        &self,
        journal: &mut Journal,
        request: &RemoteSignRequest,
        session: &SessionRecord,
        slot: Option<&ConsensusSlot>,
    ) -> SignResult<RemoteSignResponse> {
        journal.record_in_flight(request, slot).map_err(|error| {
            SignError::new(
                SignErrorCode::StorageFailure,
                format!("failed to journal the dispatch durably: {error}"),
            )
        })?;

        let payload_digest = request.payload.signing_digest();
        let signature = self.key.sign_prehash(&payload_digest).map_err(|error| {
            SignError::new(
                SignErrorCode::ProviderFailure,
                format!("sealed signing key failed: {error}"),
            )
        })?;

        let mut response = RemoteSignResponse {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            request_digest: request.request_digest,
            policy_version: request.policy_version.clone(),
            service_identity: self.config.service_identity.clone(),
            provider_signature: ProviderSignature {
                algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                signature: signature.to_vec(),
            },
            attestation_session_id: Some(session.session_id.clone()),
            session_signature: None,
        };

        let auth_digest = response.auth_digest()?;
        let session_signature = Secp256r1Crypto::sign_prehash(
            &auth_digest,
            &session.session_private_key,
        )
        .map_err(|error| {
            SignError::new(
                SignErrorCode::ProviderFailure,
                format!("failed to authenticate the response with the session key: {error}"),
            )
        })?;
        response.session_signature = Some(session_signature.to_vec());

        journal
            .record_succeeded(request, &response, slot)
            .map_err(|error| {
                SignError::new(
                    SignErrorCode::StorageFailure,
                    format!("failed to journal the completed operation durably: {error}"),
                )
            })?;
        self.signatures_issued.fetch_add(1, Ordering::SeqCst);
        Ok(response)
    }

    /// Builds a terminal rejection authenticated by the session key over the
    /// rejection binding digest. `code` must be one of
    /// [`PERMANENT_REJECTION_CODES`]; `permanent` is always true because the
    /// protocol forbids terminalizing temporary errors.
    fn authenticated_rejection(
        &self,
        request: &RemoteSignRequest,
        session: &SessionRecord,
        code: &str,
        message: String,
    ) -> SignResult<RemoteSignRejection> {
        let mut rejection = RemoteSignRejection {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            request_digest: request.request_digest,
            policy_version: request.policy_version.clone(),
            service_identity: self.config.service_identity.clone(),
            code: code.to_string(),
            message,
            permanent: true,
            attestation_session_id: Some(session.session_id.clone()),
            session_signature: None,
        };
        let auth_digest = rejection.auth_digest()?;
        let session_signature = Secp256r1Crypto::sign_prehash(
            &auth_digest,
            &session.session_private_key,
        )
        .map_err(|error| {
            SignError::new(
                SignErrorCode::ProviderFailure,
                format!("failed to authenticate the rejection with the session key: {error}"),
            )
        })?;
        rejection.session_signature = Some(session_signature.to_vec());
        Ok(rejection)
    }
}
