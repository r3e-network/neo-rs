//! Minimal mTLS client reproducing the node-side remote signer contract.
//!
//! `neo-node::remote_signer::MtlsRemoteSignerTransport` and
//! `RemoteSignerProvider` are `pub(crate)`, so this module re-implements the
//! client behavior against the exact canonical remote-signing wire types:
//!
//! * transport semantics: HTTP 200 for success, 5xx/429 after dispatch as
//!   `ResultUnknown` (retryable), other 4xx decoded as a permanent rejection;
//! * attestation: challenge → evidence with field-level freshness/binding
//!   validation, then a pluggable verifier and verified-claims validation;
//! * response/rejection authentication by the attested session P-256 key;
//! * `ResultUnknown` recovery through the idempotent query endpoint.
//!
//! Everything here mirrors the node implementation; divergences would be
//! caught because the server must satisfy both.

use super::Pki;
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_tee::attestation::AttestationConfig;
use neo_tee::{AttestationReport, AttestationService, EnclaveConfig, TeeEnclave};
use neo_wallets::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AttestationChallenge, AttestationEvidence,
    AttestationExpectations, CollateralStatus, DigestAlgorithm, KeyRef,
    REMOTE_SIGNER_PROTOCOL_VERSION, RemoteAttestationVerifier, RemoteKeyInfoResponse,
    RemoteOperationQuery, RemoteOperationStatus, RemoteSignDispatchResult, RemoteSignRejection,
    RemoteSignRequest, RemoteSignResponse, SecurityLevel, SignError, SignErrorCode, SignPayload,
    SignPurpose, SignResult, TcbStatus, VerifiedAttestation, validate_remote_schema,
};
use rand::{RngCore, rngs::OsRng};
use reqwest::{Certificate, Client, Identity, StatusCode, redirect::Policy};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const KEY_INFO_PATH: &str = "v1/signer/key-info";
const ATTEST_PATH: &str = "v1/signer/attest";
const SIGN_PATH: &str = "v1/signer/sign";
const QUERY_PATH: &str = "v1/signer/query";

const PERMANENT_REJECTION_CODES: &[&str] = &[
    "authorization_denied",
    "key_disabled",
    "key_not_found",
    "policy_permanent_denial",
];

/// Mirrors `neo-node`'s `MAX_ATTESTATION_CLOCK_SKEW_MS`.
pub const MAX_ATTESTATION_CLOCK_SKEW_MS: u64 = 60_000;

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn provider_error(message: impl Into<String>) -> SignError {
    SignError::new(SignErrorCode::ProviderFailure, message)
}

fn result_unknown(message: impl Into<String>) -> SignError {
    SignError::new(SignErrorCode::ResultUnknown, message).retryable(true)
}

fn verification_failed(message: impl Into<String>) -> SignError {
    SignError::new(SignErrorCode::VerificationFailed, message)
}

fn is_permanent_rejection_code(code: &str) -> bool {
    PERMANENT_REJECTION_CODES.contains(&code)
}

/// Client-side attestation/freshness expectations (mirrors the node's
/// `RemoteAttestationPolicySection` mapping).
pub struct ClientPolicy {
    pub service_identity: String,
    pub policy_version: String,
    pub key_ref: KeyRef,
    pub max_challenge_age_ms: u64,
    pub max_clock_skew_ms: u64,
    pub max_session_age_ms: u64,
    pub measurement: Vec<u8>,
    pub signer_measurement: Option<Vec<u8>>,
    pub product_id: u16,
    pub minimum_security_version: u16,
}

/// A validated attested session.
#[derive(Clone, Debug)]
pub struct AttestedSession {
    pub session_id: String,
    pub session_public_key: Vec<u8>,
    pub expires_at_unix_ms: i64,
}

impl AttestedSession {
    fn is_valid_for(&self, now: i64) -> bool {
        now < self.expires_at_unix_ms
    }
}

/// Verifier for the neo-tee simulation evidence produced by the reference
/// server. It parses the embedded `AttestationReport`, verifies it through
/// the neo-tee `AttestationService` (simulation allowed), and returns only
/// claims covered by that verification.
pub struct SimulationTeeVerifier {
    service: AttestationService,
    _dir: tempfile::TempDir,
}

impl SimulationTeeVerifier {
    #[must_use]
    pub fn new() -> Self {
        let dir = tempfile::TempDir::new().expect("verifier tempdir");
        let enclave = Arc::new(TeeEnclave::new(EnclaveConfig {
            sealed_data_path: dir.path().to_path_buf(),
            simulation: true,
            ..EnclaveConfig::default()
        }));
        enclave.initialize().expect("verifier enclave");
        let service = AttestationService::with_config(
            enclave,
            AttestationConfig {
                allow_simulated: true,
                ..AttestationConfig::testing()
            },
        )
        .expect("verifier attestation service");
        Self { service, _dir: dir }
    }
}

impl Default for SimulationTeeVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteAttestationVerifier for SimulationTeeVerifier {
    fn verify(
        &self,
        evidence: &AttestationEvidence,
        _expected: &AttestationExpectations,
        _now_unix_ms: i64,
    ) -> SignResult<VerifiedAttestation> {
        let report: AttestationReport =
            serde_json::from_slice(&evidence.evidence).map_err(|_| {
                verification_failed("attestation evidence does not contain a parsable TEE report")
            })?;
        if !self.service.verify_report(&report).unwrap_or(false) {
            return Err(verification_failed(
                "remote TEE attestation report failed verification",
            ));
        }
        let Ok(binding) = <[u8; 32]>::try_from(&report.report_data[..32]) else {
            return Err(verification_failed("report data binding is malformed"));
        };
        Ok(VerifiedAttestation {
            report_data: binding,
            measurement: report.mrenclave.to_vec(),
            signer_measurement: report.mrsigner.to_vec(),
            product_id: report.isv_prod_id,
            security_version: report.isv_svn,
            debug: report.attributes.debug,
            tcb_status: TcbStatus::UpToDate,
            collateral_status: CollateralStatus::Valid,
            revoked: false,
        })
    }
}

/// The minimal mTLS remote signer client.
pub struct MtlsSignerClient {
    http: Client,
    endpoint: reqwest::Url,
    policy: ClientPolicy,
    verifier: Arc<SimulationTeeVerifier>,
}

impl MtlsSignerClient {
    /// Client with a certificate/key identity trusted by the server.
    pub fn new(endpoint: reqwest::Url, pki: &Pki, policy: ClientPolicy) -> Self {
        Self::build(endpoint, pki, policy, true)
    }

    /// Client without any client certificate: the server must refuse every
    /// request at the TLS layer.
    pub fn anonymous(endpoint: reqwest::Url, pki: &Pki, policy: ClientPolicy) -> Self {
        Self::build(endpoint, pki, policy, false)
    }

    fn build(endpoint: reqwest::Url, pki: &Pki, policy: ClientPolicy, with_identity: bool) -> Self {
        let ca = Certificate::from_pem(&pki.ca_cert_pem).expect("CA PEM");
        let mut builder = Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca)
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10));
        if with_identity {
            let mut identity_pem = pki.client_cert_pem.clone();
            identity_pem.push(b'\n');
            identity_pem.extend_from_slice(&pki.client_key_pem);
            let identity = Identity::from_pem(&identity_pem).expect("client identity PEM");
            builder = builder.identity(identity);
        }
        let http = builder.build().expect("client");
        Self {
            http,
            endpoint,
            policy,
            verifier: Arc::new(SimulationTeeVerifier::new()),
        }
    }

    pub fn policy(&self) -> &ClientPolicy {
        &self.policy
    }

    // ===== transport layer (mirrors MtlsRemoteSignerTransport) =====

    fn url(&self, path: &str) -> SignResult<reqwest::Url> {
        self.endpoint
            .join(path)
            .map_err(|error| provider_error(format!("invalid route: {error}")))
    }

    async fn post<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> SignResult<R> {
        let response = self
            .http
            .post(self.url(path)?)
            .json(body)
            .send()
            .await
            .map_err(|error| provider_error(format!("remote signer transport failed: {error}")))?;
        if response.status() != StatusCode::OK {
            return Err(provider_error(format!(
                "remote signer returned HTTP {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|error| provider_error(format!("invalid remote signer response: {error}")))
    }

    async fn post_sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
        let response = self
            .http
            .post(self.url(SIGN_PATH)?)
            .json(request)
            .send()
            .await
            .map_err(|error| {
                result_unknown(format!(
                    "remote signer connection failed after dispatch became possible: {error}"
                ))
            })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| {
            result_unknown(format!(
                "remote signer response body was interrupted after dispatch: {error}"
            ))
        })?;
        Self::decode_sign_response(status, &body)
    }

    fn decode_sign_response(
        status: StatusCode,
        body: &[u8],
    ) -> SignResult<RemoteSignDispatchResult> {
        if status.is_success() {
            return serde_json::from_slice(body)
                .map(RemoteSignDispatchResult::Succeeded)
                .map_err(|error| {
                    result_unknown(format!(
                        "remote signer success response could not be decoded: {error}"
                    ))
                });
        }
        if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            return Err(result_unknown(format!(
                "remote signer returned ambiguous HTTP {status} after dispatch"
            )));
        }
        if status.is_client_error() {
            return serde_json::from_slice::<RemoteSignRejection>(body)
                .map(RemoteSignDispatchResult::Rejected)
                .map_err(|error| {
                    result_unknown(format!(
                        "remote signer returned HTTP {status} without a valid permanent rejection: {error}"
                    ))
                });
        }
        Err(result_unknown(format!(
            "remote signer returned ambiguous HTTP {status} after dispatch"
        )))
    }

    // ===== attestation flow (mirrors RemoteSignerProvider::session_for) =====

    /// Builds a fresh attestation challenge exactly like the node client.
    pub fn make_challenge(&self) -> AttestationChallenge {
        let mut challenge_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut challenge_bytes);
        AttestationChallenge {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            challenge: challenge_bytes,
            issued_at_unix_ms: now_unix_ms(),
            key_ref: self.policy.key_ref.clone(),
            service_identity: self.policy.service_identity.clone(),
            policy_version: self.policy.policy_version.clone(),
        }
    }

    /// Raw attest call (no validation).
    pub async fn attest_raw(
        &self,
        challenge: &AttestationChallenge,
    ) -> SignResult<AttestationEvidence> {
        self.post(ATTEST_PATH, challenge).await
    }

    /// Challenge → evidence → full client-side validation → attested session.
    pub async fn ensure_session(&self) -> SignResult<AttestedSession> {
        let challenge = self.make_challenge();
        let evidence = self.attest_raw(&challenge).await?;
        self.validate_evidence(&challenge, &evidence)
    }

    fn validate_evidence(
        &self,
        challenge: &AttestationChallenge,
        evidence: &AttestationEvidence,
    ) -> SignResult<AttestedSession> {
        validate_remote_schema(evidence.schema_version)?;
        let response_received_at = now_unix_ms();
        let expected = AttestationExpectations {
            challenge: challenge.challenge,
            challenge_issued_at_unix_ms: challenge.issued_at_unix_ms,
            max_challenge_age_ms: self.policy.max_challenge_age_ms,
            max_clock_skew_ms: self.policy.max_clock_skew_ms,
            max_session_age_ms: self.policy.max_session_age_ms,
            service_identity: self.policy.service_identity.clone(),
            policy_version: self.policy.policy_version.clone(),
            key_ref: self.policy.key_ref.clone(),
            measurement: self.policy.measurement.clone(),
            signer_measurement: self.policy.signer_measurement.clone(),
            product_id: self.policy.product_id,
            minimum_security_version: self.policy.minimum_security_version,
        };
        validate_evidence_fields(evidence, &expected, response_received_at)?;
        let verified = self
            .verifier
            .verify(evidence, &expected, response_received_at)?;
        validate_verified_claims(evidence, &expected, &verified)?;
        Ok(AttestedSession {
            session_id: evidence.session_id.clone(),
            session_public_key: evidence.session_public_key.clone(),
            expires_at_unix_ms: evidence.expires_at_unix_ms,
        })
    }

    // ===== key-info (mirrors RemoteSignerProvider::key_info) =====

    pub async fn key_info(&self) -> SignResult<RemoteKeyInfoResponse> {
        let response: RemoteKeyInfoResponse =
            self.post(KEY_INFO_PATH, &self.policy.key_ref).await?;
        validate_remote_schema(response.schema_version)?;
        if response.service_identity != self.policy.service_identity {
            return Err(SignError::new(
                SignErrorCode::Unauthenticated,
                "remote signer service identity changed",
            ));
        }
        if response.key_info.key_ref != self.policy.key_ref
            || response.key_info.schema_version != REMOTE_SIGNER_PROTOCOL_VERSION
            || response.key_info.algorithm != ALGORITHM_ECDSA_P256_SHA256
            || response.key_info.public_key.len() != 33
            || response.key_info.security_level != SecurityLevel::RemoteHardwareTee
            || !response.key_info.enabled
        {
            return Err(verification_failed(
                "remote key metadata does not match the configured immutable key",
            ));
        }
        Ok(response)
    }

    // ===== signing flow (mirrors RemoteSignerProvider::sign_authorized) =====

    /// Builds a sign request with a stable local authorization binding.
    pub fn build_sign_request(
        &self,
        session: &AttestedSession,
        request_id: &str,
        purpose: SignPurpose,
        digest: [u8; 32],
        network: u32,
    ) -> RemoteSignRequest {
        let key_ref = self.policy.key_ref.clone();
        let request_digest = Crypto::sha256(
            &serde_json::to_vec(&(request_id, &key_ref, digest)).expect("binding encoding"),
        );
        RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            key_ref,
            network,
            purpose,
            payload: SignPayload::Digest {
                algorithm: DigestAlgorithm::Sha256,
                digest,
            },
            request_digest,
            policy_version: self.policy.policy_version.clone(),
            attestation_session_id: Some(session.session_id.clone()),
        }
    }

    /// Dispatches the request; validates and returns the signature on
    /// success, or a terminal error for authenticated permanent rejections.
    pub async fn dispatch_and_validate(
        &self,
        request: &RemoteSignRequest,
        session: &AttestedSession,
    ) -> SignResult<RemoteSignResponse> {
        match self.post_sign(request).await? {
            RemoteSignDispatchResult::Succeeded(response) => {
                self.validate_response(request, &response, Some(session))
                    .map_err(|error| {
                        result_unknown(format!(
                            "remote signer response could not be authenticated: {error}"
                        ))
                    })?;
                Ok(response)
            }
            RemoteSignDispatchResult::Rejected(rejection) => {
                self.validate_rejection(request, &rejection, Some(session))?;
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

    /// Raw dispatch without client-side validation (used by fault tests to
    /// observe the wire behavior directly).
    pub async fn dispatch_raw(
        &self,
        request: &RemoteSignRequest,
    ) -> SignResult<RemoteSignDispatchResult> {
        self.post_sign(request).await
    }

    /// Query recovery (mirrors RemoteSignerProvider::query_unknown).
    pub async fn query_unknown(
        &self,
        session: Option<&AttestedSession>,
        request_id: &str,
        request_digest: [u8; 32],
    ) -> SignResult<RemoteOperationStatus> {
        let key_ref = self.policy.key_ref.clone();
        let query = RemoteOperationQuery {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            request_digest,
            key_ref: key_ref.clone(),
            attestation_session_id: session.map(|value| value.session_id.clone()),
        };
        let result = self
            .post(QUERY_PATH, &query)
            .await
            .map_err(|error| result_unknown(format!("{error}")))?;
        let binding = RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            key_ref: key_ref.clone(),
            network: 0,
            purpose: SignPurpose::Custom("query".to_string()),
            payload: SignPayload::Digest {
                algorithm: DigestAlgorithm::Sha256,
                digest: [0; 32],
            },
            request_digest,
            policy_version: self.policy.policy_version.clone(),
            attestation_session_id: session.map(|value| value.session_id.clone()),
        };
        match &result {
            RemoteOperationStatus::Succeeded { response } => {
                self.validate_response(&binding, response, session)?;
            }
            RemoteOperationStatus::Failed { rejection } => {
                self.validate_rejection(&binding, rejection, session)?;
            }
            RemoteOperationStatus::Pending | RemoteOperationStatus::NotFound => {}
        }
        Ok(result)
    }

    // ===== response/rejection validation (ports of the node provider) =====

    pub fn validate_response(
        &self,
        request: &RemoteSignRequest,
        response: &RemoteSignResponse,
        session: Option<&AttestedSession>,
    ) -> SignResult<()> {
        validate_remote_schema(response.schema_version)?;
        if response.request_id != request.request_id
            || response.key_ref != request.key_ref
            || response.request_digest != request.request_digest
            || response.policy_version != self.policy.policy_version
            || response.service_identity != self.policy.service_identity
        {
            return Err(verification_failed(
                "remote signing response identity or request binding changed",
            ));
        }
        match session {
            Some(session) => {
                if !session.is_valid_for(now_unix_ms())
                    || response.attestation_session_id.as_deref() != Some(&session.session_id)
                {
                    return Err(verification_failed(
                        "attested signing session expired or changed identity",
                    ));
                }
                let signature: [u8; 64] = response
                    .session_signature
                    .as_deref()
                    .ok_or_else(|| {
                        SignError::new(
                            SignErrorCode::Unauthenticated,
                            "attested response is missing session authentication",
                        )
                    })?
                    .try_into()
                    .map_err(|_| {
                        verification_failed(
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
                return Err(verification_failed(
                    "plain remote signer returned an unconfigured TEE session",
                ));
            }
            None => {}
        }
        Ok(())
    }

    pub fn validate_rejection(
        &self,
        request: &RemoteSignRequest,
        rejection: &RemoteSignRejection,
        session: Option<&AttestedSession>,
    ) -> SignResult<()> {
        validate_remote_schema(rejection.schema_version).map_err(|error| {
            result_unknown(format!(
                "remote rejection schema could not be authenticated: {error}"
            ))
        })?;
        if !rejection.permanent
            || !is_permanent_rejection_code(&rejection.code)
            || rejection.request_id != request.request_id
            || rejection.key_ref != request.key_ref
            || rejection.request_digest != request.request_digest
            || rejection.policy_version != self.policy.policy_version
            || rejection.service_identity != self.policy.service_identity
        {
            return Err(result_unknown(
                "remote rejection was not permanent or did not bind the original request",
            ));
        }
        match session {
            Some(session) => {
                if !session.is_valid_for(now_unix_ms())
                    || rejection.attestation_session_id.as_deref() != Some(&session.session_id)
                {
                    return Err(result_unknown(
                        "remote rejection was not bound to the active attested session",
                    ));
                }
                let signature: [u8; 64] = rejection
                    .session_signature
                    .as_deref()
                    .ok_or_else(|| {
                        result_unknown("remote rejection lacks attested session authentication")
                    })?
                    .try_into()
                    .map_err(|_| {
                        result_unknown("remote rejection has an invalid session signature")
                    })?;
                let auth_digest = rejection
                    .auth_digest()
                    .map_err(|error| result_unknown(format!("{error}")))?;
                if !Secp256r1Crypto::verify_prehash(
                    &auth_digest,
                    &signature,
                    &session.session_public_key,
                )
                .unwrap_or(false)
                {
                    return Err(result_unknown(
                        "remote rejection session authentication failed",
                    ));
                }
            }
            None if rejection.attestation_session_id.is_some()
                || rejection.session_signature.is_some() =>
            {
                return Err(result_unknown(
                    "plain remote signer returned an unexpected rejection session",
                ));
            }
            None => {}
        }
        Ok(())
    }
}

/// Port of the node's evidence field validation (freshness, binding, session
/// shape).
pub fn validate_evidence_fields(
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
        || evidence.expires_at_unix_ms > evidence.issued_at_unix_ms.saturating_add(max_session_age)
    {
        return Err(verification_failed(
            "remote attestation challenge, session, identity, policy, key, or freshness binding failed",
        ));
    }
    Ok(())
}

/// Port of the node's verified-claims validation (binding digest,
/// measurement, product, SVN, debug, TCB, collateral, revocation).
pub fn validate_verified_claims(
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
        return Err(verification_failed(
            "remote hardware evidence failed measurement, product, SVN, debug, TCB, collateral, revocation, or report-data checks",
        ));
    }
    Ok(())
}
