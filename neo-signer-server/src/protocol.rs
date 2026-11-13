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
//! * every dispatch is journaled by request id — completed requests are
//!   replayed, never re-signed, and recoverable through the query endpoint
//!   (`Pending`/`Succeeded`/`Failed`/`NotFound`).

use crate::config::SignerServerConfig;
use crate::keys::SealedSigningKey;
use neo_crypto::Secp256r1Crypto;
use neo_tee::attestation::AttestationConfig;
use neo_tee::{AttestationService, TeeEnclave};
use neo_wallets::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AttestationChallenge, AttestationEvidence, KeyInfo, KeyRef,
    ProviderSignature, REMOTE_SIGNER_PROTOCOL_VERSION, RemoteKeyInfoResponse, RemoteOperationQuery,
    RemoteOperationStatus, RemoteSignRejection, RemoteSignRequest, RemoteSignResponse,
    SecurityLevel, SignError, SignErrorCode, SignResult, validate_remote_schema,
};
use parking_lot::{Mutex, MutexGuard};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
    session_private_key: [u8; 32],
    key_ref: KeyRef,
    expires_at_unix_ms: i64,
}

impl SessionRecord {
    fn is_valid_at(&self, now: i64, key_ref: &KeyRef) -> bool {
        now < self.expires_at_unix_ms && &self.key_ref == key_ref
    }
}

/// Durable state of one dispatched signing request.
enum JournalState {
    /// Recorded but not yet signed (only under the `hold_dispatch` fault).
    InFlight { request: RemoteSignRequest },
    /// Durably completed; replayed verbatim on re-dispatch and query.
    Succeeded(RemoteSignResponse),
    /// Durably and permanently rejected; replayed verbatim.
    Failed(RemoteSignRejection),
}

struct JournalEntry {
    request_digest: [u8; 32],
    state: JournalState,
}

/// Outcome of one signing dispatch, mapped onto HTTP statuses by the HTTP
/// layer:
///
/// * [`SignOutcome::Succeeded`] → `200` with a [`RemoteSignResponse`],
/// * [`SignOutcome::Rejected`] → 4xx with an authenticated
///   [`RemoteSignRejection`] (terminal for the node client),
/// * [`SignOutcome::Ambiguous`] → 5xx, so the node classifies the dispatch as
///   `ResultUnknown` and recovers through the query endpoint.
pub enum SignOutcome {
    Succeeded(RemoteSignResponse),
    Rejected(RemoteSignRejection),
    Ambiguous(String),
}

/// The signer service. All endpoint logic lives here; the HTTP layer is a
/// thin translation shim.
pub struct SignerService {
    config: SignerServerConfig,
    key: SealedSigningKey,
    attestation: AttestationService,
    sessions: Mutex<BTreeMap<String, SessionRecord>>,
    used_challenges: Mutex<BTreeMap<[u8; 32], i64>>,
    journal: Mutex<BTreeMap<String, JournalEntry>>,
    signatures_issued: AtomicUsize,
    attestation_sessions_issued: AtomicUsize,
}

impl SignerService {
    /// Builds the service: seals or unseals the signing key and initializes
    /// the neo-tee attestation service in simulation mode.
    pub fn new(config: SignerServerConfig) -> anyhow::Result<Self> {
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
        Ok(Self {
            config,
            key,
            attestation,
            sessions: Mutex::new(BTreeMap::new()),
            used_challenges: Mutex::new(BTreeMap::new()),
            journal: Mutex::new(BTreeMap::new()),
            signatures_issued: AtomicUsize::new(0),
            attestation_sessions_issued: AtomicUsize::new(0),
        })
    }

    /// Configuration served by this instance.
    #[must_use]
    pub fn config(&self) -> &SignerServerConfig {
        &self.config
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
    pub fn key_info(&self, key_ref: &KeyRef) -> SignResult<RemoteKeyInfoResponse> {
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
        challenge: &AttestationChallenge,
    ) -> SignResult<AttestationEvidence> {
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

        let session_private_key = Secp256r1Crypto::generate_private_key();
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
    pub fn sign(&self, request: &RemoteSignRequest) -> SignOutcome {
        if let Err(error) = validate_remote_schema(request.schema_version) {
            return SignOutcome::Ambiguous(format!(
                "unsupported remote signer protocol version: {error}"
            ));
        }

        let mut journal = self.journal.lock();

        // Idempotency: a completed request is replayed verbatim and is never
        // re-signed.
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
        let Some(session) = self.valid_session(request) else {
            return SignOutcome::Ambiguous(
                "no valid attested session for this request; re-attest and retry".to_string(),
            );
        };

        if let Some((code, message)) = self.permanent_denial(request) {
            match self.authenticated_rejection(request, &session, code, message) {
                Ok(rejection) => {
                    journal.insert(
                        request.request_id.clone(),
                        JournalEntry {
                            request_digest: request.request_digest,
                            state: JournalState::Failed(rejection.clone()),
                        },
                    );
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
            journal.insert(
                request.request_id.clone(),
                JournalEntry {
                    request_digest: request.request_digest,
                    state: JournalState::InFlight {
                        request: request.clone(),
                    },
                },
            );
            return SignOutcome::Ambiguous(
                "dispatch recorded; processing deferred (fault injection)".to_string(),
            );
        }

        match self.complete_request(&mut journal, request, &session) {
            Ok(response) => SignOutcome::Succeeded(response),
            Err(error) => SignOutcome::Ambiguous(format!("failed to complete signing: {error}")),
        }
    }

    /// `POST /v1/signer/query`: idempotent recovery for an unknown dispatch
    /// outcome. The first query for an in-flight request completes it
    /// (modeling the server's deferred processing) while still reporting
    /// `Pending` for that query; later queries serve the durable result.
    pub fn query(&self, query: &RemoteOperationQuery) -> SignResult<RemoteOperationStatus> {
        validate_remote_schema(query.schema_version)?;
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
                if let Some(session) = self.valid_session(&request) {
                    // Deferred completion: perform the signing now. This
                    // query still reports Pending because the operation had
                    // not completed when it arrived.
                    let _ = self.complete_request(&mut journal, &request, &session);
                }
                Ok(RemoteOperationStatus::Pending)
            }
        }
    }

    fn valid_session(&self, request: &RemoteSignRequest) -> Option<SessionRecord> {
        let session_id = request.attestation_session_id.as_deref()?;
        let session = self.sessions.lock().get(session_id).cloned()?;
        if !session.is_valid_at(now_unix_ms(), &request.key_ref) {
            return None;
        }
        Some(session)
    }

    /// Permanent policy denials for an otherwise well-bound request. Returns
    /// a (code, message) pair using only codes the node client treats as
    /// terminal.
    fn permanent_denial(&self, request: &RemoteSignRequest) -> Option<(&'static str, String)> {
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
        None
    }

    /// Signs the request payload with the sealed key and authenticates the
    /// response with the attested session key. The caller holds the journal
    /// lock; the completed entry is journaled before the response is handed
    /// out, so the result is already recoverable when the client sees it.
    fn complete_request(
        &self,
        journal: &mut MutexGuard<'_, BTreeMap<String, JournalEntry>>,
        request: &RemoteSignRequest,
        session: &SessionRecord,
    ) -> SignResult<RemoteSignResponse> {
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

        journal.insert(
            request.request_id.clone(),
            JournalEntry {
                request_digest: request.request_digest,
                state: JournalState::Succeeded(response.clone()),
            },
        );
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
    use crate::config::{FaultPolicy, TlsPaths};
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

    #[tokio::test]
    async fn sign_without_session_is_never_terminal() {
        let (_dir, config) = service_dir("no-session");
        let service = SignerService::new(config).expect("service");
        // No session established: the only safe answer is ambiguity, because
        // the node client cannot authenticate a terminal answer without the
        // attested session key.
        match service.sign(&request("req-1", SignPurpose::Consensus)) {
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
                .key_info(&KeyRef::new("remote", "other-key", 1))
                .is_err()
        );
        let info = service
            .key_info(&KeyRef::new("remote", "validator-1", 2))
            .expect("key info");
        assert_eq!(info.key_info.public_key.len(), 33);
        assert!(info.key_info.enabled);
        assert_eq!(info.key_info.algorithm, ALGORITHM_ECDSA_P256_SHA256);
    }

    #[tokio::test]
    async fn attest_rejects_replayed_challenges() {
        let (_dir, config) = service_dir("replay");
        let service = SignerService::new(config).expect("service");
        let challenge = AttestationChallenge {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            challenge: [0x55; 32],
            issued_at_unix_ms: now_unix_ms(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            service_identity: "signer.prod".to_string(),
            policy_version: "policy-v1".to_string(),
        };
        assert!(service.attest(&challenge).await.is_ok());
        let replayed = service.attest(&challenge).await.expect_err("replay");
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
}
