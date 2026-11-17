//! Core signing protocol: endpoint semantics of the reference signer service.
//!
//! This module owns the behavior the node-side `RemoteSignerProvider`
//! validates:
//!
//! * attestation challenges are single-use and freshness-bounded;
//! * evidence binds the challenge nonce, session P-256 key, service identity,
//!   policy version and [`KeyRef`] into the report data of a neo-tee
//!   (simulation) attestation report;
//! * every response and permanent rejection is authenticated by a P-256
//!   signature of the attested session key over
//!   [`RemoteSignResponse::auth_digest`] / [`RemoteSignRejection::auth_digest`];
//! * every dispatch is journaled by request id in an **append-only file inside
//!   the sealed-data directory** — completed requests are replayed, never
//!   re-signed, survive a restart, and are recoverable through the query
//!   endpoint (`Pending`/`Succeeded`/`Failed`/`NotFound`);
//! * consensus requests reserve their `(network, height, view, phase)` slot
//!   durably, and a conflicting request for an already reserved slot is
//!   permanently rejected (anti-equivocation);
//! * every endpoint is scoped to the authenticated mutual-TLS client identity:
//!   the [`crate::config::ClientAuthorizationPolicy`] maps certificate common
//!   names to the key IDs and purposes they may use.

use crate::config::{
    ClientAuthorizationPolicy, SignerServerConfig, key_selector,
};
use crate::identity::ClientIdentity;
use crate::journal::{ConsensusSlot, Journal, JournalState};
use crate::keys::SealedSigningKey;
use anyhow::Context;
use neo_crypto::Secp256r1Crypto;
use neo_tee::attestation::AttestationConfig;
use neo_tee::{AttestationService, TeeEnclave};
use neo_wallets::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AttestationChallenge, AttestationEvidence, KeyInfo, KeyRef,
    ProviderSignature, REMOTE_SIGNER_PROTOCOL_VERSION, RemoteKeyInfoResponse, RemoteOperationQuery,
    RemoteOperationStatus, RemoteSignRejection, RemoteSignRequest, RemoteSignResponse,
    SecurityLevel, SignError, SignErrorCode, SignPurpose, SignResult, validate_remote_schema,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// Permanent rejection codes accepted by the node client's
/// `is_permanent_rejection_code` check (mirrored here for documentation).
pub const PERMANENT_REJECTION_CODES: &[&str] = &[
    "authorization_denied",
    "key_disabled",
    "key_not_found",
    "policy_permanent_denial",
];

pub(crate) fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

/// One live attested session. The private key authenticates every response
/// and rejection bound to this session.
#[derive(Clone)]
struct SessionRecord {
    session_id: String,
    /// Session key. Held in [`Zeroizing`] so the private scalar is wiped when
    /// the session expires, is replaced, or the service is dropped.
    session_private_key: Zeroizing<[u8; 32]>,
    key_ref: KeyRef,
    /// Common name of the mutual-TLS client that established the session. A
    /// session is never usable by another identity.
    client_common_name: String,
    expires_at_unix_ms: i64,
}

impl SessionRecord {
    fn is_valid_at(&self, now: i64, key_ref: &KeyRef, client: &ClientIdentity) -> bool {
        now < self.expires_at_unix_ms
            && &self.key_ref == key_ref
            && self.client_common_name == client.common_name()
    }
}

/// Outcome of one signing dispatch, mapped onto HTTP statuses by the HTTP
/// layer:
///
/// * [`SignOutcome::Succeeded`] → `200` with a [`RemoteSignResponse`],
/// * [`SignOutcome::Rejected`] → 4xx with an authenticated
///   [`RemoteSignRejection`] (terminal for the node client),
/// * [`SignOutcome::Unauthorized`] → `403` when the authenticated client
///   identity is not scoped to the requested key or purpose,
/// * [`SignOutcome::Ambiguous`] → 5xx, so the node classifies the dispatch as
///   `ResultUnknown` and recovers through the query endpoint.
pub enum SignOutcome {
    Succeeded(RemoteSignResponse),
    Rejected(RemoteSignRejection),
    Unauthorized(String),
    Ambiguous(String),
}

/// The signer service. All endpoint logic lives here; the HTTP layer is a
/// thin translation shim.
pub struct SignerService {
    config: SignerServerConfig,
    key: SealedSigningKey,
    attestation: AttestationService,
    authorization: ClientAuthorizationPolicy,
    sessions: Mutex<BTreeMap<String, SessionRecord>>,
    used_challenges: Mutex<BTreeMap<[u8; 32], i64>>,
    /// Durable, append-only operation journal. Restores on startup, so a
    /// restart can neither forget a completed operation nor forget a reserved
    /// consensus slot.
    journal: Mutex<Journal>,
    signatures_issued: AtomicUsize,
    attestation_sessions_issued: AtomicUsize,
}

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
        if let Err(error) =
            self.authorize(Some(identity), &request.key_ref, Some(&request.purpose))
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

        journal.record_succeeded(request, &response, slot).map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ClientAuthorizationPolicy, ClientGrant, FaultPolicy, TlsPaths, key_selector,
    };
    use neo_wallets::signing::{DigestAlgorithm, SignPayload, SignPurpose};

    fn service_dir(tag: &str) -> (tempfile::TempDir, SignerServerConfig) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = SignerServerConfig {
            bind_addr: "127.0.0.1:0".parse().expect("bind address"),
            tls: TlsPaths {
                server_certificate_path: dir.path().join("server.pem"),
                server_private_key_path: dir.path().join("server.key"),
                ca_certificate_path: dir.path().join("ca.pem"),
            },
            sealed_data_dir: dir.path().join("sealed-data"),
            sealed_key_path: dir.path().join("sealed-key.json"),
            service_identity: "signer.prod".to_string(),
            policy_version: "policy-v1".to_string(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            allowed_purposes: vec![SignPurpose::Consensus],
            session_lifetime_ms: 300_000,
            max_challenge_age_ms: 60_000,
            faults: FaultPolicy::default(),
        };
        let _ = tag;
        (dir, config)
    }

    fn request(request_id: &str, purpose: SignPurpose) -> RemoteSignRequest {
        RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            network: 860_833_102,
            purpose,
            payload: SignPayload::Digest {
                algorithm: DigestAlgorithm::Sha256,
                digest: [7; 32],
            },
            request_digest: [9; 32],
            policy_version: "policy-v1".to_string(),
            attestation_session_id: None,
        }
    }

    fn identity() -> ClientIdentity {
        ClientIdentity::from_common_name("neo-node-test-client")
    }

    fn challenge(nonce: u8) -> AttestationChallenge {
        AttestationChallenge {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            challenge: [nonce; 32],
            issued_at_unix_ms: now_unix_ms(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            service_identity: "signer.prod".to_string(),
            policy_version: "policy-v1".to_string(),
        }
    }

    async fn attest_session(
        service: &SignerService,
        identity: &ClientIdentity,
        nonce: u8,
    ) -> String {
        service
            .attest(identity, &challenge(nonce))
            .await
            .expect("attested session")
            .session_id
    }

    fn with_session(mut request: RemoteSignRequest, session_id: &str) -> RemoteSignRequest {
        request.attestation_session_id = Some(session_id.to_string());
        request
    }

    fn query_for(request: &RemoteSignRequest) -> RemoteOperationQuery {
        RemoteOperationQuery {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            request_digest: request.request_digest,
            key_ref: request.key_ref.clone(),
            attestation_session_id: request.attestation_session_id.clone(),
        }
    }

    #[tokio::test]
    async fn sign_without_session_is_never_terminal() {
        let (_dir, config) = service_dir("no-session");
        let service = SignerService::new(config).expect("service");
        // No session established: the only safe answer is ambiguity, because
        // the node client cannot authenticate a terminal answer without the
        // attested session key.
        match service.sign(&identity(), &request("req-1", SignPurpose::Consensus)) {
            SignOutcome::Ambiguous(reason) => {
                assert!(reason.contains("session"), "unexpected reason: {reason}");
            }
            _ => panic!("dispatch without a session must stay ambiguous"),
        }
    }

    #[tokio::test]
    async fn key_info_rejects_foreign_keys() {
        let (_dir, config) = service_dir("key-info");
        let service = SignerService::new(config).expect("service");
        assert!(
            service
                .key_info(&identity(), &KeyRef::new("remote", "other-key", 1))
                .is_err()
        );
        let info = service
            .key_info(&identity(), &KeyRef::new("remote", "validator-1", 2))
            .expect("key info");
        assert_eq!(info.key_info.public_key.len(), 33);
        assert!(info.key_info.enabled);
        assert_eq!(info.key_info.algorithm, ALGORITHM_ECDSA_P256_SHA256);
    }

    #[tokio::test]
    async fn attest_rejects_replayed_challenges() {
        let (_dir, config) = service_dir("replay");
        let service = SignerService::new(config).expect("service");
        let challenge = challenge(0x55);
        assert!(service.attest(&identity(), &challenge).await.is_ok());
        let replayed = service
            .attest(&identity(), &challenge)
            .await
            .expect_err("replay");
        assert!(replayed.to_string().contains("replay"));
    }

    #[tokio::test]
    async fn sealed_key_is_reused_across_service_instances() {
        let (dir, config) = service_dir("restart");
        let first = SignerService::new(config.clone()).expect("first boot");
        let second = SignerService::new(config).expect("second boot");
        assert_eq!(first.public_key(), second.public_key());
        let _ = dir; // keeps the tempdir alive
    }

    #[tokio::test]
    async fn completed_operations_survive_a_restart_and_are_never_resigned() {
        let (_dir, config) = service_dir("journal-restart");
        let identity = identity();
        let first = SignerService::new(config.clone()).expect("first boot");
        let session = attest_session(&first, &identity, 1).await;
        let request = with_session(request("req-journal", SignPurpose::Consensus), &session);
        let SignOutcome::Succeeded(response) = first.sign(&identity, &request) else {
            panic!("the first dispatch must sign")
        };
        assert_eq!(first.signatures_issued(), 1);
        assert!(first.journal_path().exists(), "the journal must live on disk");
        drop(first);

        // Restart on the same sealed-data directory: the journal is replayed
        // from disk, so the operation is recoverable and is never re-signed.
        let second = SignerService::new(config).expect("second boot");
        assert_eq!(
            second.journaled_operations(),
            1,
            "the operation journal must survive a restart"
        );
        match second.sign(&identity, &request) {
            SignOutcome::Succeeded(replayed) => assert_eq!(
                replayed.provider_signature.signature,
                response.provider_signature.signature
            ),
            _ => panic!("a journaled operation must be replayed verbatim"),
        }
        match second
            .query(&identity, &query_for(&request))
            .expect("query")
        {
            RemoteOperationStatus::Succeeded { response: replayed } => assert_eq!(
                replayed.provider_signature.signature,
                response.provider_signature.signature
            ),
            other => panic!("expected the durable result, got {other:?}"),
        }
        assert_eq!(
            second.signatures_issued(),
            0,
            "a restart must never produce a second signature"
        );
    }

    #[tokio::test]
    async fn a_conflicting_consensus_slot_is_refused_durably() {
        let (_dir, config) = service_dir("slot-conflict");
        let identity = identity();
        let slot_payload = |digest: u8| SignPayload::Canonical {
            encoding: "neo-consensus/v1/commit".to_string(),
            bytes: vec![0, 0, 0, 42, 1, digest],
        };

        let first = SignerService::new(config.clone()).expect("first boot");
        let session = attest_session(&first, &identity, 2).await;

        let mut original = with_session(request("req-slot-a", SignPurpose::Consensus), &session);
        original.payload = slot_payload(0x11);
        original.request_digest = [0x11; 32];
        assert!(matches!(
            first.sign(&identity, &original),
            SignOutcome::Succeeded(_)
        ));

        // Same (network, height, view, phase) with a different payload is an
        // equivocation and must never reach the sealed key.
        let mut conflicting = with_session(request("req-slot-b", SignPurpose::Consensus), &session);
        conflicting.payload = slot_payload(0x22);
        conflicting.request_digest = [0x22; 32];
        match first.sign(&identity, &conflicting) {
            SignOutcome::Rejected(rejection) => {
                assert_eq!(rejection.code, "policy_permanent_denial");
                assert!(rejection.permanent);
                assert!(
                    rejection.message.contains("consensus slot"),
                    "unexpected message: {}",
                    rejection.message
                );
            }
            _ => panic!("a conflicting consensus payload must be rejected"),
        }
        assert_eq!(first.signatures_issued(), 1);
        drop(first);

        // The slot reservation is durable: a restarted signer still refuses a
        // new payload for the same slot.
        let second = SignerService::new(config).expect("second boot");
        let session = attest_session(&second, &identity, 3).await;
        let mut late = with_session(request("req-slot-c", SignPurpose::Consensus), &session);
        late.payload = slot_payload(0x33);
        late.request_digest = [0x33; 32];
        match second.sign(&identity, &late) {
            SignOutcome::Rejected(rejection) => {
                assert_eq!(rejection.code, "policy_permanent_denial");
                assert!(rejection.message.contains("consensus slot"));
            }
            _ => panic!("the consensus slot reservation must survive a restart"),
        }
        assert_eq!(second.signatures_issued(), 0);
    }

    #[tokio::test]
    async fn an_unlisted_client_identity_cannot_reach_the_signing_path() {
        let (_dir, config) = service_dir("identity");
        let key = config.key_ref.clone();
        let policy = ClientAuthorizationPolicy {
            grants: vec![ClientGrant::new(
                "neo-node-1",
                vec![key_selector(&key)],
                vec![SignPurpose::Consensus],
            )],
        };
        let service = SignerService::with_client_authorization(config, policy).expect("service");
        let allowed = ClientIdentity::from_common_name("neo-node-1");
        let stranger = ClientIdentity::from_common_name("neo-node-2");

        // Attestation, key metadata and signing are all scoped by identity.
        assert!(service.attest(&stranger, &challenge(4)).await.is_err());
        assert!(service.key_info(&stranger, &key).is_err());
        assert_eq!(service.attestation_sessions_issued(), 0);

        let session = attest_session(&service, &allowed, 5).await;
        let allowed_request = with_session(request("req-id", SignPurpose::Consensus), &session);
        assert!(matches!(
            service.sign(&allowed, &allowed_request),
            SignOutcome::Succeeded(_)
        ));

        // The stranger is refused before the journal and the sealed key.
        assert!(matches!(
            service.sign(&stranger, &allowed_request),
            SignOutcome::Unauthorized(_)
        ));
        assert!(service.query(&stranger, &query_for(&allowed_request)).is_err());
        assert_eq!(service.signatures_issued(), 1);
    }

    #[tokio::test]
    async fn sessions_are_bound_to_the_identity_that_attested() {
        let (_dir, config) = service_dir("identity-binding");
        let key = config.key_ref.clone();
        let policy = ClientAuthorizationPolicy {
            grants: vec![
                ClientGrant::new("neo-node-1", vec![key_selector(&key)], vec![SignPurpose::Consensus]),
                ClientGrant::new("neo-node-2", vec![key_selector(&key)], vec![SignPurpose::Consensus]),
            ],
        };
        let service = SignerService::with_client_authorization(config, policy).expect("service");
        let node1 = ClientIdentity::from_common_name("neo-node-1");
        let node2 = ClientIdentity::from_common_name("neo-node-2");
        let session = attest_session(&service, &node1, 6).await;
        let stolen = with_session(request("req-steal", SignPurpose::Consensus), &session);
        match service.sign(&node2, &stolen) {
            SignOutcome::Ambiguous(reason) => {
                assert!(reason.contains("session"), "unexpected reason: {reason}");
            }
            _ => panic!("another identity must not be able to use the session"),
        }
        assert_eq!(service.signatures_issued(), 0);
    }
}
