//! Dedicated mTLS remote signer and remotely attested TEE adapter.

use crate::config::{RemoteAttestationPolicySection, SignerProviderKind, SignerProviderSection};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use neo_core::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AttestationChallenge, AttestationEvidence,
    AttestationExpectations, AuthorizedSignRequest, CollateralStatus, KeyInfo, KeyRef,
    ProviderCapabilities, ProviderOperationStatus, ProviderOutcomeProvenance, ProviderSignature,
    REMOTE_SIGNER_PROTOCOL_VERSION, RemoteAttestationVerifier, RemoteKeyInfoResponse,
    RemoteOperationQuery, RemoteOperationStatus, RemoteSignDispatchResult, RemoteSignRejection,
    RemoteSignRequest, RemoteSignResponse, RemoteSignerTransport, SignError, SignErrorCode,
    SignResult, SignerProvider, TcbStatus, VerifiedAttestation, validate_remote_schema,
};
use neo_crypto::Secp256r1Crypto;
use parking_lot::RwLock;
use rand::{RngCore, rngs::OsRng};
use reqwest::{Certificate, Client, Identity, StatusCode, Url, redirect::Policy};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
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
pub(crate) const MAX_ATTESTATION_CLOCK_SKEW_MS: u64 = 60_000;

fn is_permanent_rejection_code(code: &str) -> bool {
    PERMANENT_REJECTION_CODES.contains(&code)
}

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

pub(crate) fn validate_remote_provider_config(provider: &SignerProviderSection) -> Result<()> {
    let endpoint = provider
        .endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "remote signer provider '{}' requires an HTTPS endpoint",
                provider.id
            )
        })?;
    let url = Url::parse(endpoint).with_context(|| {
        format!(
            "remote signer provider '{}' has an invalid endpoint",
            provider.id
        )
    })?;
    if url.scheme() != "https"
        || url.username() != ""
        || url.password().is_some()
        || url.host_str().is_none()
    {
        bail!(
            "remote signer provider '{}' endpoint must be credential-free HTTPS",
            provider.id
        );
    }
    let remote = provider.remote.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "remote signer provider '{}' requires dedicated mTLS configuration",
            provider.id
        )
    })?;
    if remote.server_name.trim().is_empty()
        || url.host_str() != Some(remote.server_name.trim())
        || remote.ca_certificate_path.trim().is_empty()
        || remote.client_certificate_path.trim().is_empty()
        || remote.client_private_key_path.trim().is_empty()
        || remote.service_identity.trim().is_empty()
        || provider.keys.is_empty()
        || remote.identity_mapping.get(&remote.service_identity) != Some(&provider.id)
    {
        bail!(
            "remote signer provider '{}' requires matching service name, mTLS files, identity mapping, and keys",
            provider.id
        );
    }
    for path in [
        &remote.ca_certificate_path,
        &remote.client_certificate_path,
        &remote.client_private_key_path,
    ] {
        let metadata = fs::metadata(path).with_context(|| {
            format!(
                "remote signer provider '{}' cannot read TLS material '{}'",
                provider.id, path
            )
        })?;
        if !metadata.is_file() {
            bail!(
                "remote signer provider '{}' TLS material '{}' is not a file",
                provider.id,
                path
            );
        }
    }
    let mut keys = BTreeSet::new();
    for key in &provider.keys {
        let public_key = hex::decode(&key.public_key_hex).with_context(|| {
            format!(
                "remote signer provider '{}' key '{}' has invalid public_key_hex",
                provider.id, key.key_id
            )
        })?;
        if key.key_id.trim().is_empty()
            || key.version == 0
            || public_key.len() != 33
            || !keys.insert((key.key_id.as_str(), key.version))
        {
            bail!(
                "remote signer provider '{}' requires unique versioned keys and compressed P-256 public keys",
                provider.id
            );
        }
    }
    Ok(())
}

/// HTTPS transport isolated from wallet and node RPC clients.
pub(crate) struct MtlsRemoteSignerTransport {
    client: Client,
    endpoint: Url,
}

impl MtlsRemoteSignerTransport {
    pub(crate) fn from_config(provider: &SignerProviderSection) -> Result<Self> {
        let remote = provider
            .remote
            .as_ref()
            .context("validated remote signer has no mTLS configuration")?;
        let endpoint = Url::parse(
            provider
                .endpoint
                .as_deref()
                .context("validated remote signer has no endpoint")?,
        )?;
        let ca_pem = fs::read(&remote.ca_certificate_path)
            .with_context(|| format!("failed to read remote signer CA for '{}'", provider.id))?;
        let client_cert = fs::read(&remote.client_certificate_path).with_context(|| {
            format!(
                "failed to read remote signer client certificate for '{}'",
                provider.id
            )
        })?;
        let client_key = fs::read(&remote.client_private_key_path).with_context(|| {
            format!(
                "failed to read remote signer client key for '{}'",
                provider.id
            )
        })?;
        let ca = Certificate::from_pem(&ca_pem)
            .context("remote signer CA is not a valid PEM certificate")?;
        let mut identity_pem = client_cert;
        identity_pem.extend_from_slice(b"\n");
        identity_pem.extend_from_slice(&client_key);
        let identity = Identity::from_pem(&identity_pem)
            .context("remote signer client certificate/key is not valid PEM identity material")?;
        let client = Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca)
            .identity(identity)
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("failed to build dedicated remote signer mTLS client")?;
        Ok(Self { client, endpoint })
    }

    fn url(&self, path: &str) -> SignResult<Url> {
        self.endpoint
            .join(path)
            .map_err(|error| provider_error(format!("invalid remote signer route: {error}")))
    }

    async fn post<T: serde::Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> SignResult<R> {
        let response = self
            .client
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

    fn result_unknown(message: impl Into<String>) -> SignError {
        SignError::new(SignErrorCode::ResultUnknown, message).retryable(true)
    }

    fn decode_sign_response(
        status: StatusCode,
        body: &[u8],
    ) -> SignResult<RemoteSignDispatchResult> {
        if status.is_success() {
            return serde_json::from_slice(body)
                .map(RemoteSignDispatchResult::Succeeded)
                .map_err(|error| {
                    Self::result_unknown(format!(
                        "remote signer success response could not be decoded: {error}"
                    ))
                });
        }
        if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            return Err(Self::result_unknown(format!(
                "remote signer returned ambiguous HTTP {status} after dispatch"
            )));
        }
        if status.is_client_error() {
            return serde_json::from_slice::<RemoteSignRejection>(body)
                .map(RemoteSignDispatchResult::Rejected)
                .map_err(|error| {
                    Self::result_unknown(format!(
                        "remote signer returned HTTP {status} without a valid permanent rejection: {error}"
                    ))
                });
        }
        Err(Self::result_unknown(format!(
            "remote signer returned ambiguous HTTP {status} after dispatch"
        )))
    }

    async fn post_sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
        let response = self
            .client
            .post(self.url(SIGN_PATH)?)
            .json(request)
            .send()
            .await
            .map_err(|error| {
                Self::result_unknown(format!(
                    "remote signer connection failed after dispatch became possible: {error}"
                ))
            })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| {
            Self::result_unknown(format!(
                "remote signer response body was interrupted after dispatch: {error}"
            ))
        })?;
        Self::decode_sign_response(status, &body)
    }
}

#[async_trait]
impl RemoteSignerTransport for MtlsRemoteSignerTransport {
    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<RemoteKeyInfoResponse> {
        self.post(KEY_INFO_PATH, key_ref).await
    }

    async fn attest(&self, challenge: &AttestationChallenge) -> SignResult<AttestationEvidence> {
        self.post(ATTEST_PATH, challenge).await
    }

    async fn sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
        self.post_sign(request).await
    }

    async fn query(&self, query: &RemoteOperationQuery) -> SignResult<RemoteOperationStatus> {
        self.post(QUERY_PATH, query).await.map_err(|error| {
            Self::result_unknown(format!(
                "remote signer recovery query could not establish a result: {error}"
            ))
        })
    }
}

#[derive(Clone)]
struct ConfiguredKey {
    public_key: Vec<u8>,
    account: String,
}

#[derive(Clone)]
struct AttestedSession {
    session_id: String,
    session_public_key: Vec<u8>,
    service_identity: String,
    policy_version: String,
    key_ref: KeyRef,
    expires_at_unix_ms: i64,
}

impl AttestedSession {
    fn is_valid_for(
        &self,
        now: i64,
        service_identity: &str,
        policy_version: &str,
        key_ref: &KeyRef,
    ) -> bool {
        now < self.expires_at_unix_ms
            && self.service_identity == service_identity
            && self.policy_version == policy_version
            && &self.key_ref == key_ref
    }
}

/// Strict adapter from the unified signer service to a dedicated remote signer.
pub(crate) struct RemoteSignerProvider {
    capabilities: ProviderCapabilities,
    service_identity: String,
    policy_version: String,
    keys: BTreeMap<KeyRef, ConfiguredKey>,
    transport: Arc<dyn RemoteSignerTransport>,
    attestation: Option<RemoteAttestationRuntime>,
    sessions: RwLock<BTreeMap<KeyRef, AttestedSession>>,
}

struct RemoteAttestationRuntime {
    policy: RemoteAttestationPolicySection,
    verifier: Arc<dyn RemoteAttestationVerifier>,
}

impl RemoteSignerProvider {
    pub(crate) fn new(
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
                        "remote TEE freshness policy requires positive challenge/session ages and clock skew no greater than {} ms",
                        MAX_ATTESTATION_CLOCK_SKEW_MS
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

    fn validate_evidence_fields(
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

    fn validate_response(
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
            purpose: neo_core::signing::SignPurpose::Custom("query".to_string()),
            payload: neo_core::signing::SignPayload::Digest {
                algorithm: neo_core::signing::DigestAlgorithm::Sha256,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RemoteSignerSection, SignerKeySection};
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Request as HttpRequest, Response as HttpResponse, Server};
    use neo_core::signing::{
        AuthenticatedContext, DefaultSignerService, DurableSigningStore, IdempotencyInfo,
        PolicyLimits, ProtocolContext, SIGNING_SCHEMA_VERSION, SecurityLevel, SignPayload,
        SignPurpose, SignRequest, SignerService, SigningKeyBinding, SigningKeyResolver,
        SigningPolicy, SigningRequestStatus, SigningStore,
    };
    use parking_lot::Mutex;
    use std::convert::Infallible;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

    #[derive(Clone, Copy)]
    enum EvidenceFault {
        None,
        ReplayedChallenge,
        Stale,
        Future,
        Delayed,
        Expired,
        Measurement,
        EvidenceBytes,
        Debug,
        LowSvn,
        Session,
        Key,
        Policy,
        Tcb,
        Collateral,
        Revoked,
    }

    struct MockTransport {
        fault: EvidenceFault,
        key_public: Vec<u8>,
        session_public: Vec<u8>,
    }

    #[async_trait]
    impl RemoteSignerTransport for MockTransport {
        async fn key_info(&self, key_ref: &KeyRef) -> SignResult<RemoteKeyInfoResponse> {
            Ok(RemoteKeyInfoResponse {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                service_identity: "signer.prod".to_string(),
                key_info: KeyInfo {
                    schema_version: SIGNING_SCHEMA_VERSION,
                    key_ref: key_ref.clone(),
                    algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                    public_key: self.key_public.clone(),
                    security_level: SecurityLevel::RemoteHardwareTee,
                    enabled: true,
                    attributes: BTreeMap::new(),
                },
            })
        }

        async fn attest(
            &self,
            challenge: &AttestationChallenge,
        ) -> SignResult<AttestationEvidence> {
            if matches!(self.fault, EvidenceFault::Delayed) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let mut evidence = AttestationEvidence {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                challenge: challenge.challenge,
                session_id: "session-1".to_string(),
                session_public_key: self.session_public.clone(),
                service_identity: challenge.service_identity.clone(),
                policy_version: challenge.policy_version.clone(),
                key_ref: challenge.key_ref.clone(),
                issued_at_unix_ms: challenge.issued_at_unix_ms,
                expires_at_unix_ms: challenge.issued_at_unix_ms + 5_000,
                measurement: vec![0x11; 32],
                signer_measurement: vec![0x22; 32],
                product_id: 7,
                security_version: 5,
                debug: false,
                evidence: b"mock-cryptographic-evidence".to_vec(),
            };
            match self.fault {
                EvidenceFault::None
                | EvidenceFault::Delayed
                | EvidenceFault::Tcb
                | EvidenceFault::Collateral
                | EvidenceFault::Revoked => {}
                EvidenceFault::ReplayedChallenge => evidence.challenge[0] ^= 0xff,
                EvidenceFault::Stale => evidence.issued_at_unix_ms -= 60_000,
                EvidenceFault::Future => evidence.issued_at_unix_ms += 10_000,
                EvidenceFault::Expired => evidence.expires_at_unix_ms = challenge.issued_at_unix_ms,
                EvidenceFault::Measurement => evidence.measurement[0] ^= 0xff,
                EvidenceFault::EvidenceBytes => evidence.evidence[0] ^= 0xff,
                EvidenceFault::Debug => evidence.debug = true,
                EvidenceFault::LowSvn => evidence.security_version = 1,
                EvidenceFault::Session => evidence.session_id.clear(),
                EvidenceFault::Key => evidence.key_ref = KeyRef::new("remote", "other", 1),
                EvidenceFault::Policy => evidence.policy_version = "other-policy".to_string(),
            }
            Ok(evidence)
        }

        async fn sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
            Ok(RemoteSignDispatchResult::Succeeded(RemoteSignResponse {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                request_id: request.request_id.clone(),
                key_ref: request.key_ref.clone(),
                request_digest: request.request_digest,
                policy_version: request.policy_version.clone(),
                service_identity: "signer.prod".to_string(),
                provider_signature: ProviderSignature {
                    algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                    signature: vec![0; 64],
                },
                attestation_session_id: Some("different-session".to_string()),
                session_signature: Some(vec![0; 64]),
            }))
        }

        async fn query(&self, _query: &RemoteOperationQuery) -> SignResult<RemoteOperationStatus> {
            Ok(RemoteOperationStatus::Pending)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DispatchOutcome {
        Http500,
        Http429,
        InvalidJson,
        Disconnect,
        InvalidSignatureThenValid,
        EmptySignatureThenValid,
        AlgorithmMismatchThenValid,
        InvalidSignatureAlways,
        AuthenticatedRejection,
        TamperedRejection,
        UnknownRejectionCode,
    }

    struct StatefulRemoteTransport {
        outcome: DispatchOutcome,
        key_private: [u8; 32],
        key_public: Vec<u8>,
        sign_calls: AtomicUsize,
        query_calls: AtomicUsize,
        original_request: Mutex<Option<RemoteSignRequest>>,
        stored_response: Mutex<Option<RemoteSignResponse>>,
    }

    impl StatefulRemoteTransport {
        fn new(outcome: DispatchOutcome) -> Self {
            let key_private = [0x31; 32];
            let key_public = Secp256r1Crypto::derive_public_key(&key_private).expect("key public");
            Self {
                outcome,
                key_private,
                key_public,
                sign_calls: AtomicUsize::new(0),
                query_calls: AtomicUsize::new(0),
                original_request: Mutex::new(None),
                stored_response: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl RemoteSignerTransport for StatefulRemoteTransport {
        async fn key_info(&self, key_ref: &KeyRef) -> SignResult<RemoteKeyInfoResponse> {
            Ok(RemoteKeyInfoResponse {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                service_identity: "signer.prod".to_string(),
                key_info: KeyInfo {
                    schema_version: SIGNING_SCHEMA_VERSION,
                    key_ref: key_ref.clone(),
                    algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                    public_key: self.key_public.clone(),
                    security_level: SecurityLevel::Software,
                    enabled: true,
                    attributes: BTreeMap::new(),
                },
            })
        }

        async fn attest(
            &self,
            _challenge: &AttestationChallenge,
        ) -> SignResult<AttestationEvidence> {
            Err(provider_error("plain remote transport does not attest"))
        }

        async fn sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
            self.sign_calls.fetch_add(1, Ordering::SeqCst);
            *self.original_request.lock() = Some(request.clone());
            if matches!(
                self.outcome,
                DispatchOutcome::AuthenticatedRejection
                    | DispatchOutcome::TamperedRejection
                    | DispatchOutcome::UnknownRejectionCode
            ) {
                let mut rejection = RemoteSignRejection {
                    schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                    request_id: request.request_id.clone(),
                    key_ref: request.key_ref.clone(),
                    request_digest: request.request_digest,
                    policy_version: request.policy_version.clone(),
                    service_identity: "signer.prod".to_string(),
                    code: "policy_permanent_denial".to_string(),
                    message: "request is permanently denied".to_string(),
                    permanent: true,
                    attestation_session_id: None,
                    session_signature: None,
                };
                if self.outcome == DispatchOutcome::TamperedRejection {
                    rejection.request_id = "different-request".to_string();
                } else if self.outcome == DispatchOutcome::UnknownRejectionCode {
                    rejection.code = "temporary_backend_error".to_string();
                }
                let body = serde_json::to_vec(&rejection).expect("serialize rejection");
                return MtlsRemoteSignerTransport::decode_sign_response(
                    StatusCode::BAD_REQUEST,
                    &body,
                );
            }

            let signature =
                Secp256r1Crypto::sign_prehash(&request.payload.signing_digest(), &self.key_private)
                    .expect("remote signature");
            let response = RemoteSignResponse {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                request_id: request.request_id.clone(),
                key_ref: request.key_ref.clone(),
                request_digest: request.request_digest,
                policy_version: request.policy_version.clone(),
                service_identity: "signer.prod".to_string(),
                provider_signature: ProviderSignature {
                    algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                    signature: signature.to_vec(),
                },
                attestation_session_id: None,
                session_signature: None,
            };
            let mut dispatched_response = response.clone();
            match self.outcome {
                DispatchOutcome::InvalidSignatureThenValid
                | DispatchOutcome::InvalidSignatureAlways => {
                    dispatched_response.provider_signature.signature = vec![0; 64];
                }
                DispatchOutcome::EmptySignatureThenValid => {
                    dispatched_response.provider_signature.signature.clear();
                }
                DispatchOutcome::AlgorithmMismatchThenValid => {
                    dispatched_response.provider_signature.algorithm =
                        "unexpected-algorithm".to_string();
                }
                _ => {}
            }
            let query_response = if self.outcome == DispatchOutcome::InvalidSignatureAlways {
                dispatched_response.clone()
            } else {
                response.clone()
            };
            *self.stored_response.lock() = Some(query_response);
            match self.outcome {
                DispatchOutcome::Http500 => MtlsRemoteSignerTransport::decode_sign_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &serde_json::to_vec(&response).expect("serialize response"),
                ),
                DispatchOutcome::Http429 => MtlsRemoteSignerTransport::decode_sign_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    &serde_json::to_vec(&response).expect("serialize response"),
                ),
                DispatchOutcome::InvalidJson => {
                    MtlsRemoteSignerTransport::decode_sign_response(StatusCode::OK, b"{not-json")
                }
                DispatchOutcome::Disconnect => Err(MtlsRemoteSignerTransport::result_unknown(
                    "remote signer connection interrupted after dispatch",
                )),
                DispatchOutcome::InvalidSignatureThenValid
                | DispatchOutcome::EmptySignatureThenValid
                | DispatchOutcome::AlgorithmMismatchThenValid
                | DispatchOutcome::InvalidSignatureAlways => {
                    Ok(RemoteSignDispatchResult::Succeeded(dispatched_response))
                }
                DispatchOutcome::AuthenticatedRejection
                | DispatchOutcome::TamperedRejection
                | DispatchOutcome::UnknownRejectionCode => {
                    unreachable!("rejections returned above")
                }
            }
        }

        async fn query(&self, query: &RemoteOperationQuery) -> SignResult<RemoteOperationStatus> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            let original = self.original_request.lock();
            let original = original.as_ref().expect("query must follow dispatch");
            assert_eq!(query.request_id, original.request_id);
            assert_eq!(query.request_digest, original.request_digest);
            assert_eq!(query.key_ref, original.key_ref);
            assert_eq!(
                query.attestation_session_id,
                original.attestation_session_id
            );
            let response = self
                .stored_response
                .lock()
                .clone()
                .expect("ambiguous dispatch recorded a signature");
            Ok(RemoteOperationStatus::Succeeded { response })
        }
    }

    struct MockVerifier {
        fault: EvidenceFault,
    }

    impl RemoteAttestationVerifier for MockVerifier {
        fn verify(
            &self,
            evidence: &AttestationEvidence,
            _expected: &AttestationExpectations,
            _now_unix_ms: i64,
        ) -> SignResult<VerifiedAttestation> {
            if evidence.evidence != b"mock-cryptographic-evidence" {
                return Err(SignError::new(
                    SignErrorCode::VerificationFailed,
                    "mock rejected modified evidence bytes",
                ));
            }
            Ok(VerifiedAttestation {
                report_data: evidence.binding_digest()?,
                measurement: evidence.measurement.clone(),
                signer_measurement: evidence.signer_measurement.clone(),
                product_id: evidence.product_id,
                security_version: evidence.security_version,
                debug: evidence.debug,
                tcb_status: if matches!(self.fault, EvidenceFault::Tcb) {
                    TcbStatus::OutOfDate
                } else {
                    TcbStatus::UpToDate
                },
                collateral_status: if matches!(self.fault, EvidenceFault::Collateral) {
                    CollateralStatus::Expired
                } else {
                    CollateralStatus::Valid
                },
                revoked: matches!(self.fault, EvidenceFault::Revoked),
            })
        }
    }

    fn provider_config(key_public: &[u8]) -> SignerProviderSection {
        SignerProviderSection {
            id: "remote".to_string(),
            kind: SignerProviderKind::RemoteTee,
            enabled: true,
            default: true,
            security_level: Some(SecurityLevel::RemoteHardwareTee),
            purposes: vec![SignPurpose::Transaction],
            canonical_encodings: vec![crate::signing_provider::NEO_SIGN_DATA_ENCODING.to_string()],
            algorithms: vec![ALGORITHM_ECDSA_P256_SHA256.to_string()],
            keys: vec![SignerKeySection {
                key_id: "validator-1".to_string(),
                version: 2,
                public_key_hex: hex::encode(key_public),
                account: "0x0123456789abcdef0123456789abcdef01234567".to_string(),
                purposes: vec![SignPurpose::Transaction],
            }],
            endpoint: Some("https://signer.example/".to_string()),
            remote: Some(RemoteSignerSection {
                server_name: "signer.example".to_string(),
                ca_certificate_path: "unused-test-ca".to_string(),
                client_certificate_path: "unused-test-cert".to_string(),
                client_private_key_path: "unused-test-key".to_string(),
                service_identity: "signer.prod".to_string(),
                identity_mapping: BTreeMap::from([(
                    "signer.prod".to_string(),
                    "remote".to_string(),
                )]),
                attestation: Some(RemoteAttestationPolicySection {
                    verifier: "mock-only".to_string(),
                    measurement_hex: hex::encode([0x11; 32]),
                    signer_measurement_hex: Some(hex::encode([0x22; 32])),
                    product_id: 7,
                    minimum_security_version: 5,
                    max_challenge_age_ms: 1_000,
                    max_clock_skew_ms: 10,
                    max_session_age_ms: 10_000,
                }),
            }),
            pkcs11: None,
        }
    }

    fn plain_provider_config(key_public: &[u8]) -> SignerProviderSection {
        let mut config = provider_config(key_public);
        config.kind = SignerProviderKind::Remote;
        config.security_level = Some(SecurityLevel::Software);
        config
            .remote
            .as_mut()
            .expect("remote configuration")
            .attestation = None;
        config
    }

    fn signing_context() -> AuthenticatedContext {
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: "service-a".to_string(),
            tenant: "tenant-a".to_string(),
            authentication_method: "mtls".to_string(),
            scopes: BTreeSet::from(["sign:transaction".to_string()]),
            claims: BTreeMap::new(),
        }
    }

    fn signing_request(request_id: &str, key_ref: KeyRef) -> SignRequest {
        SignRequest {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: request_id.to_string(),
            network: 860_833_102,
            purpose: SignPurpose::Transaction,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: crate::signing_provider::NEO_SIGN_DATA_ENCODING.to_string(),
                bytes: vec![1, 2, 3, 4],
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-transaction".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "policy-v1".to_string(),
            deadline_unix_ms: now_unix_ms().saturating_add(60_000),
            idempotency: IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: format!("idem-{request_id}"),
                replay_nonce: format!("nonce-{request_id}"),
            },
            fee_units: 1,
            quota_units: 1,
            account: Some("account-a".to_string()),
            targets: BTreeSet::new(),
        }
    }

    fn signing_policy(key_ref: KeyRef, key_public: Vec<u8>) -> SigningPolicy {
        let mut policy = SigningPolicy {
            version: "policy-v1".to_string(),
            key_resolver: SigningKeyResolver::new([SigningKeyBinding {
                account: "account-a".to_string(),
                purpose: SignPurpose::Transaction,
                key_ref: key_ref.clone(),
                public_key: key_public,
            }])
            .expect("key resolver"),
            limits: PolicyLimits {
                max_fee_units: 10,
                quota_limit_units: 10,
                quota_window_ms: 60_000,
            },
            ..SigningPolicy::default()
        };
        policy.principals.allow.insert("service-a".to_string());
        policy.tenants.allow.insert("tenant-a".to_string());
        policy.keys.allow.insert(key_ref.selector());
        policy.networks.allow.insert(860_833_102);
        policy.purposes.allow.insert(SignPurpose::Transaction);
        policy
            .authorizations
            .allow
            .insert(SigningPolicy::authorization_selector(
                "service-a",
                "tenant-a",
                &key_ref,
                860_833_102,
                &SignPurpose::Transaction,
                "sign:transaction",
            ));
        policy.security_levels.allow.insert(SecurityLevel::Software);
        policy.accounts.allow.insert("account-a".to_string());
        policy
    }

    fn provider(fault: EvidenceFault) -> RemoteSignerProvider {
        let key_private = [0x31; 32];
        let session_private = [0x42; 32];
        let key_public = Secp256r1Crypto::derive_public_key(&key_private).expect("key public");
        let session_public =
            Secp256r1Crypto::derive_public_key(&session_private).expect("session public");
        let transport: Arc<dyn RemoteSignerTransport> = Arc::new(MockTransport {
            fault,
            key_public: key_public.clone(),
            session_public,
        });
        let mut config = provider_config(&key_public);
        if matches!(fault, EvidenceFault::Delayed) {
            config
                .remote
                .as_mut()
                .and_then(|remote| remote.attestation.as_mut())
                .expect("attestation policy")
                .max_challenge_age_ms = 5;
        }
        RemoteSignerProvider::new(
            &config,
            "policy-v1",
            transport,
            Some(Arc::new(MockVerifier { fault })),
        )
        .expect("mock provider")
    }

    #[test]
    fn sign_response_classification_requires_strict_permanent_rejection_protocol() {
        let rejection = RemoteSignRejection {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: "request-1".to_string(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            request_digest: [4; 32],
            policy_version: "policy-v1".to_string(),
            service_identity: "signer.prod".to_string(),
            code: "policy_permanent_denial".to_string(),
            message: "permanent denial".to_string(),
            permanent: true,
            attestation_session_id: None,
            session_signature: None,
        };
        let rejection_body = serde_json::to_vec(&rejection).expect("serialize rejection");
        for (status, body) in [
            (StatusCode::INTERNAL_SERVER_ERROR, rejection_body.as_slice()),
            (StatusCode::TOO_MANY_REQUESTS, rejection_body.as_slice()),
            (StatusCode::BAD_GATEWAY, b"gateway failure".as_slice()),
            (StatusCode::BAD_REQUEST, b"{invalid-json".as_slice()),
            (StatusCode::OK, b"{invalid-json".as_slice()),
        ] {
            let error = MtlsRemoteSignerTransport::decode_sign_response(status, body)
                .expect_err("ambiguous response must remain unknown");
            assert_eq!(error.code, SignErrorCode::ResultUnknown);
            assert!(error.retryable);
        }
        let decoded = MtlsRemoteSignerTransport::decode_sign_response(
            StatusCode::BAD_REQUEST,
            &rejection_body,
        )
        .expect("structured 4xx rejection remains a validation candidate");
        assert_eq!(decoded, RemoteSignDispatchResult::Rejected(rejection));
    }

    #[tokio::test]
    async fn ambiguous_remote_dispatch_stays_pending_and_recovers_without_resigning() {
        for outcome in [
            DispatchOutcome::Http500,
            DispatchOutcome::Http429,
            DispatchOutcome::InvalidJson,
            DispatchOutcome::Disconnect,
        ] {
            let transport = Arc::new(StatefulRemoteTransport::new(outcome));
            let key_ref = KeyRef::new("remote", "validator-1", 2);
            let provider: Arc<dyn SignerProvider> = Arc::new(
                RemoteSignerProvider::new(
                    &plain_provider_config(&transport.key_public),
                    "policy-v1",
                    transport.clone(),
                    None,
                )
                .expect("plain remote provider"),
            );
            let directory = tempfile::tempdir().expect("temporary signing store");
            let store = Arc::new(
                DurableSigningStore::open(directory.path().join("signing.journal"))
                    .expect("durable signing store"),
            );
            let store_trait: Arc<dyn SigningStore> = store.clone();
            let service = DefaultSignerService::new(
                provider,
                store_trait,
                Arc::new(signing_policy(
                    key_ref.clone(),
                    transport.key_public.clone(),
                )),
            );
            let request = signing_request(&format!("ambiguous-{outcome:?}"), key_ref);
            let context = signing_context();
            let error = service
                .sign(request.clone(), context.clone())
                .await
                .expect_err("ambiguous dispatch must not become terminal");
            assert_eq!(error.code, SignErrorCode::ResultUnknown);
            assert!(error.retryable);
            assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);
            assert!(matches!(
                store
                    .load(&request.request_id)
                    .await
                    .expect("load pending")
                    .unwrap()
                    .status,
                SigningRequestStatus::Pending
            ));

            let retry = service
                .sign(request.clone(), context.clone())
                .await
                .expect_err("pending operation must not be re-signed");
            assert_eq!(retry.code, SignErrorCode::RequestInProgress);
            assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);

            drop(service);
            drop(store);
            let reopened_store = Arc::new(
                DurableSigningStore::open(directory.path().join("signing.journal"))
                    .expect("reopen durable signing store"),
            );
            let reopened_store_trait: Arc<dyn SigningStore> = reopened_store.clone();
            let restarted_provider: Arc<dyn SignerProvider> = Arc::new(
                RemoteSignerProvider::new(
                    &plain_provider_config(&transport.key_public),
                    "policy-v1",
                    transport.clone(),
                    None,
                )
                .expect("restarted plain remote provider"),
            );
            let restarted = DefaultSignerService::new(
                restarted_provider,
                reopened_store_trait,
                Arc::new(signing_policy(
                    request.key_ref.clone(),
                    transport.key_public.clone(),
                )),
            );
            let recovered = restarted
                .recover_operation(&request.request_id, context.clone())
                .await
                .expect("restart performs query-only recovery");
            assert!(matches!(recovered, SigningRequestStatus::Succeeded(_)));
            assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);
            assert_eq!(transport.query_calls.load(Ordering::SeqCst), 1);
            assert!(matches!(
                reopened_store
                    .load(&request.request_id)
                    .await
                    .expect("load success")
                    .unwrap()
                    .status,
                SigningRequestStatus::Succeeded(_)
            ));

            let persisted = restarted
                .recover_operation(&request.request_id, context)
                .await
                .expect("persisted recovered result");
            assert!(matches!(persisted, SigningRequestStatus::Succeeded(_)));
            assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);
            assert_eq!(transport.query_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn remote_unverified_signature_material_stays_pending_then_recovers_by_query() {
        for outcome in [
            DispatchOutcome::InvalidSignatureThenValid,
            DispatchOutcome::EmptySignatureThenValid,
            DispatchOutcome::AlgorithmMismatchThenValid,
        ] {
            let transport = Arc::new(StatefulRemoteTransport::new(outcome));
            let key_ref = KeyRef::new("remote", "validator-1", 2);
            let provider: Arc<dyn SignerProvider> = Arc::new(
                RemoteSignerProvider::new(
                    &plain_provider_config(&transport.key_public),
                    "policy-v1",
                    transport.clone(),
                    None,
                )
                .expect("plain remote provider"),
            );
            let directory = tempfile::tempdir().expect("temporary signing store");
            let store = Arc::new(
                DurableSigningStore::open(directory.path().join("signing.journal"))
                    .expect("durable signing store"),
            );
            let store_trait: Arc<dyn SigningStore> = store.clone();
            let service = DefaultSignerService::new(
                provider,
                store_trait,
                Arc::new(signing_policy(
                    key_ref.clone(),
                    transport.key_public.clone(),
                )),
            );
            let request = signing_request(&format!("unverified-{outcome:?}"), key_ref);
            let context = signing_context();

            let error = service
                .sign(request.clone(), context.clone())
                .await
                .expect_err("unverified remote material must be outcome-unknown");
            assert_eq!(error.code, SignErrorCode::ResultUnknown);
            assert!(error.retryable);
            assert!(matches!(
                store
                    .load(&request.request_id)
                    .await
                    .expect("load pending operation")
                    .unwrap()
                    .status,
                SigningRequestStatus::Pending
            ));

            let recovered = service
                .recover_operation(&request.request_id, context)
                .await
                .expect("query returns the valid signature for the original request");
            assert!(matches!(recovered, SigningRequestStatus::Succeeded(_)));
            assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);
            assert_eq!(transport.query_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn repeated_invalid_remote_query_stays_pending_without_resigning() {
        let transport = Arc::new(StatefulRemoteTransport::new(
            DispatchOutcome::InvalidSignatureAlways,
        ));
        let key_ref = KeyRef::new("remote", "validator-1", 2);
        let provider: Arc<dyn SignerProvider> = Arc::new(
            RemoteSignerProvider::new(
                &plain_provider_config(&transport.key_public),
                "policy-v1",
                transport.clone(),
                None,
            )
            .expect("plain remote provider"),
        );
        let directory = tempfile::tempdir().expect("temporary signing store");
        let store = Arc::new(
            DurableSigningStore::open(directory.path().join("signing.journal"))
                .expect("durable signing store"),
        );
        let store_trait: Arc<dyn SigningStore> = store.clone();
        let service = DefaultSignerService::new(
            provider,
            store_trait,
            Arc::new(signing_policy(
                key_ref.clone(),
                transport.key_public.clone(),
            )),
        );
        let request = signing_request("repeated-invalid-query", key_ref);
        let context = signing_context();

        let error = service
            .sign(request.clone(), context.clone())
            .await
            .expect_err("invalid remote signature must remain unknown");
        assert_eq!(error.code, SignErrorCode::ResultUnknown);
        for _ in 0..2 {
            let recovered = service
                .recover_operation(&request.request_id, context.clone())
                .await
                .expect("invalid query material is not terminal");
            assert!(matches!(recovered, SigningRequestStatus::Pending));
        }
        let retry = service
            .sign(request.clone(), context)
            .await
            .expect_err("pending request must never be signed again");
        assert_eq!(retry.code, SignErrorCode::RequestInProgress);
        assert!(matches!(
            store
                .load(&request.request_id)
                .await
                .expect("load pending operation")
                .unwrap()
                .status,
            SigningRequestStatus::Pending
        ));
        assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);
        assert_eq!(transport.query_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn only_authenticated_request_bound_permanent_rejection_becomes_failed() {
        for (outcome, expected_code, expected_failed) in [
            (
                DispatchOutcome::AuthenticatedRejection,
                SignErrorCode::ProviderFailure,
                true,
            ),
            (
                DispatchOutcome::TamperedRejection,
                SignErrorCode::ResultUnknown,
                false,
            ),
            (
                DispatchOutcome::UnknownRejectionCode,
                SignErrorCode::ResultUnknown,
                false,
            ),
        ] {
            let transport = Arc::new(StatefulRemoteTransport::new(outcome));
            let key_ref = KeyRef::new("remote", "validator-1", 2);
            let provider: Arc<dyn SignerProvider> = Arc::new(
                RemoteSignerProvider::new(
                    &plain_provider_config(&transport.key_public),
                    "policy-v1",
                    transport.clone(),
                    None,
                )
                .expect("plain remote provider"),
            );
            let directory = tempfile::tempdir().expect("temporary signing store");
            let store = Arc::new(
                DurableSigningStore::open(directory.path().join("signing.journal"))
                    .expect("durable signing store"),
            );
            let store_trait: Arc<dyn SigningStore> = store.clone();
            let service = DefaultSignerService::new(
                provider,
                store_trait,
                Arc::new(signing_policy(
                    key_ref.clone(),
                    transport.key_public.clone(),
                )),
            );
            let request = signing_request(&format!("rejection-{outcome:?}"), key_ref);
            let context = signing_context();
            let error = service
                .sign(request.clone(), context.clone())
                .await
                .expect_err("rejection must not sign");
            assert_eq!(error.code, expected_code);
            assert_eq!(error.retryable, !expected_failed);
            let persisted = store
                .load(&request.request_id)
                .await
                .expect("load rejection")
                .expect("persisted rejection operation");
            assert_eq!(
                matches!(persisted.status, SigningRequestStatus::Failed(_)),
                expected_failed
            );
            if expected_failed {
                assert!(matches!(
                    service
                        .recover_operation(&request.request_id, context)
                        .await
                        .expect("persisted terminal rejection"),
                    SigningRequestStatus::Failed(_)
                ));
            } else {
                assert!(matches!(persisted.status, SigningRequestStatus::Pending));
            }
            assert_eq!(transport.sign_calls.load(Ordering::SeqCst), 1);
            assert_eq!(transport.query_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum HttpDispatchOutcome {
        SignedThen500,
        RateLimited,
        InvalidJson,
        Disconnect,
        PermanentRejection,
    }

    struct MockHttpSigner {
        outcome: HttpDispatchOutcome,
        key_private: [u8; 32],
        key_public: Vec<u8>,
        sign_calls: AtomicUsize,
        query_calls: AtomicUsize,
        original_request: Mutex<Option<RemoteSignRequest>>,
        stored_response: Mutex<Option<RemoteSignResponse>>,
    }

    impl MockHttpSigner {
        fn new(outcome: HttpDispatchOutcome) -> Self {
            let key_private = [0x31; 32];
            let key_public = Secp256r1Crypto::derive_public_key(&key_private).expect("key public");
            Self {
                outcome,
                key_private,
                key_public,
                sign_calls: AtomicUsize::new(0),
                query_calls: AtomicUsize::new(0),
                original_request: Mutex::new(None),
                stored_response: Mutex::new(None),
            }
        }

        fn json_response<T: serde::Serialize>(
            status: hyper::StatusCode,
            value: &T,
        ) -> HttpResponse<Body> {
            HttpResponse::builder()
                .status(status)
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(value).expect("serialize mock HTTP response"),
                ))
                .expect("mock HTTP response")
        }

        fn signed_response(&self, request: &RemoteSignRequest) -> RemoteSignResponse {
            let signature =
                Secp256r1Crypto::sign_prehash(&request.payload.signing_digest(), &self.key_private)
                    .expect("remote signature");
            RemoteSignResponse {
                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                request_id: request.request_id.clone(),
                key_ref: request.key_ref.clone(),
                request_digest: request.request_digest,
                policy_version: request.policy_version.clone(),
                service_identity: "signer.prod".to_string(),
                provider_signature: ProviderSignature {
                    algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                    signature: signature.to_vec(),
                },
                attestation_session_id: None,
                session_signature: None,
            }
        }

        async fn handle(
            self: Arc<Self>,
            request: HttpRequest<Body>,
        ) -> std::io::Result<HttpResponse<Body>> {
            let path = request.uri().path().to_string();
            let body = hyper::body::to_bytes(request.into_body())
                .await
                .map_err(std::io::Error::other)?;
            match path.as_str() {
                "/v1/signer/key-info" => {
                    let key_ref: KeyRef = serde_json::from_slice(&body)
                        .map_err(|error| std::io::Error::other(error.to_string()))?;
                    Ok(Self::json_response(
                        hyper::StatusCode::OK,
                        &RemoteKeyInfoResponse {
                            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                            service_identity: "signer.prod".to_string(),
                            key_info: KeyInfo {
                                schema_version: SIGNING_SCHEMA_VERSION,
                                key_ref,
                                algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
                                public_key: self.key_public.clone(),
                                security_level: SecurityLevel::Software,
                                enabled: true,
                                attributes: BTreeMap::new(),
                            },
                        },
                    ))
                }
                "/v1/signer/sign" => {
                    self.sign_calls.fetch_add(1, Ordering::SeqCst);
                    let request: RemoteSignRequest = serde_json::from_slice(&body)
                        .map_err(|error| std::io::Error::other(error.to_string()))?;
                    *self.original_request.lock() = Some(request.clone());
                    if self.outcome != HttpDispatchOutcome::PermanentRejection {
                        *self.stored_response.lock() = Some(self.signed_response(&request));
                    }
                    match self.outcome {
                        HttpDispatchOutcome::SignedThen500 => {
                            let response = self.signed_response(&request);
                            Ok(Self::json_response(
                                hyper::StatusCode::INTERNAL_SERVER_ERROR,
                                &response,
                            ))
                        }
                        HttpDispatchOutcome::RateLimited => Ok(HttpResponse::builder()
                            .status(hyper::StatusCode::TOO_MANY_REQUESTS)
                            .body(Body::from("rate limited"))
                            .expect("rate-limit response")),
                        HttpDispatchOutcome::InvalidJson => Ok(HttpResponse::builder()
                            .status(hyper::StatusCode::OK)
                            .body(Body::from("{invalid-json"))
                            .expect("invalid JSON response")),
                        HttpDispatchOutcome::Disconnect => Err(std::io::Error::new(
                            std::io::ErrorKind::ConnectionAborted,
                            "mock disconnected after reading the request",
                        )),
                        HttpDispatchOutcome::PermanentRejection => {
                            let rejection = RemoteSignRejection {
                                schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
                                request_id: request.request_id,
                                key_ref: request.key_ref,
                                request_digest: request.request_digest,
                                policy_version: request.policy_version,
                                service_identity: "signer.prod".to_string(),
                                code: "policy_permanent_denial".to_string(),
                                message: "request is permanently denied".to_string(),
                                permanent: true,
                                attestation_session_id: None,
                                session_signature: None,
                            };
                            Ok(Self::json_response(
                                hyper::StatusCode::BAD_REQUEST,
                                &rejection,
                            ))
                        }
                    }
                }
                "/v1/signer/query" => {
                    self.query_calls.fetch_add(1, Ordering::SeqCst);
                    let query: RemoteOperationQuery = serde_json::from_slice(&body)
                        .map_err(|error| std::io::Error::other(error.to_string()))?;
                    let original = self.original_request.lock();
                    let original = original.as_ref().expect("query follows dispatch");
                    assert_eq!(query.request_id, original.request_id);
                    assert_eq!(query.request_digest, original.request_digest);
                    assert_eq!(query.key_ref, original.key_ref);
                    let status = self
                        .stored_response
                        .lock()
                        .clone()
                        .map_or(RemoteOperationStatus::Pending, |response| {
                            RemoteOperationStatus::Succeeded { response }
                        });
                    Ok(Self::json_response(hyper::StatusCode::OK, &status))
                }
                _ => Ok(HttpResponse::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .expect("not-found response")),
            }
        }
    }

    async fn spawn_mock_http_signer(
        outcome: HttpDispatchOutcome,
    ) -> (
        Arc<MockHttpSigner>,
        MtlsRemoteSignerTransport,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind mock signer");
        listener
            .set_nonblocking(true)
            .expect("nonblocking mock listener");
        let address = listener.local_addr().expect("mock signer address");
        let state = Arc::new(MockHttpSigner::new(outcome));
        let service_state = state.clone();
        let service = make_service_fn(move |_| {
            let connection_state = service_state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |request| {
                    connection_state.clone().handle(request)
                }))
            }
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = Server::from_tcp(listener)
            .expect("mock HTTP server")
            .serve(service)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });
        let task = tokio::spawn(async move {
            server.await.expect("mock HTTP signer server");
        });
        let transport = MtlsRemoteSignerTransport {
            client: Client::builder()
                .redirect(Policy::none())
                .build()
                .expect("mock HTTP client"),
            endpoint: Url::parse(&format!("http://{address}/")).expect("mock signer URL"),
        };
        (state, transport, shutdown_tx, task)
    }

    #[tokio::test]
    async fn mock_http_ambiguous_dispatches_stay_pending_and_query_without_resigning() {
        for outcome in [
            HttpDispatchOutcome::SignedThen500,
            HttpDispatchOutcome::RateLimited,
            HttpDispatchOutcome::InvalidJson,
            HttpDispatchOutcome::Disconnect,
        ] {
            let (server, transport, shutdown, task) = spawn_mock_http_signer(outcome).await;
            let key_ref = KeyRef::new("remote", "validator-1", 2);
            let provider: Arc<dyn SignerProvider> = Arc::new(
                RemoteSignerProvider::new(
                    &plain_provider_config(&server.key_public),
                    "policy-v1",
                    Arc::new(transport),
                    None,
                )
                .expect("HTTP remote provider"),
            );
            let directory = tempfile::tempdir().expect("temporary signing store");
            let store = Arc::new(
                DurableSigningStore::open(directory.path().join("signing.journal"))
                    .expect("durable signing store"),
            );
            let store_trait: Arc<dyn SigningStore> = store.clone();
            let service = DefaultSignerService::new(
                provider,
                store_trait,
                Arc::new(signing_policy(key_ref.clone(), server.key_public.clone())),
            );
            let request = signing_request(&format!("mock-http-{outcome:?}"), key_ref);
            let context = signing_context();
            let error = service
                .sign(request.clone(), context.clone())
                .await
                .expect_err("ambiguous HTTP dispatch must not become terminal");
            assert_eq!(error.code, SignErrorCode::ResultUnknown);
            assert!(error.retryable);
            assert_eq!(server.sign_calls.load(Ordering::SeqCst), 1);
            assert!(matches!(
                store
                    .load(&request.request_id)
                    .await
                    .expect("load pending")
                    .unwrap()
                    .status,
                SigningRequestStatus::Pending
            ));

            let recovered = service
                .recover_operation(&request.request_id, context)
                .await
                .expect("query-only recovery");
            assert!(matches!(recovered, SigningRequestStatus::Succeeded(_)));
            assert_eq!(server.sign_calls.load(Ordering::SeqCst), 1);
            assert_eq!(server.query_calls.load(Ordering::SeqCst), 1);
            let _ = shutdown.send(());
            task.await.expect("mock signer task");
        }
    }

    #[tokio::test]
    async fn mock_http_authenticated_permanent_rejection_is_terminal() {
        let (server, transport, shutdown, task) =
            spawn_mock_http_signer(HttpDispatchOutcome::PermanentRejection).await;
        let key_ref = KeyRef::new("remote", "validator-1", 2);
        let provider: Arc<dyn SignerProvider> = Arc::new(
            RemoteSignerProvider::new(
                &plain_provider_config(&server.key_public),
                "policy-v1",
                Arc::new(transport),
                None,
            )
            .expect("HTTP remote provider"),
        );
        let directory = tempfile::tempdir().expect("temporary signing store");
        let store = Arc::new(
            DurableSigningStore::open(directory.path().join("signing.journal"))
                .expect("durable signing store"),
        );
        let store_trait: Arc<dyn SigningStore> = store.clone();
        let service = DefaultSignerService::new(
            provider,
            store_trait,
            Arc::new(signing_policy(key_ref.clone(), server.key_public.clone())),
        );
        let request = signing_request("mock-http-permanent-rejection", key_ref);
        let error = service
            .sign(request.clone(), signing_context())
            .await
            .expect_err("authenticated permanent rejection must fail");
        assert_eq!(error.code, SignErrorCode::ProviderFailure);
        assert!(!error.retryable);
        assert!(matches!(
            store
                .load(&request.request_id)
                .await
                .expect("load failed operation")
                .unwrap()
                .status,
            SigningRequestStatus::Failed(_)
        ));
        assert_eq!(server.sign_calls.load(Ordering::SeqCst), 1);
        assert_eq!(server.query_calls.load(Ordering::SeqCst), 0);
        let _ = shutdown.send(());
        task.await.expect("mock signer task");
    }

    #[test]
    fn transport_config_rejects_missing_mtls_and_plaintext() {
        let key_public = Secp256r1Crypto::derive_public_key(&[0x31; 32]).expect("key public");
        let mut missing_mtls = provider_config(&key_public);
        missing_mtls.remote = None;
        let error = validate_remote_provider_config(&missing_mtls)
            .expect_err("missing mTLS configuration must fail");
        assert!(error.to_string().contains("mTLS configuration"));

        let mut plaintext = provider_config(&key_public);
        plaintext.endpoint = Some("http://signer.example".to_string());
        let error =
            validate_remote_provider_config(&plaintext).expect_err("plaintext endpoint must fail");
        assert!(error.to_string().contains("credential-free HTTPS"));
    }

    #[tokio::test]
    async fn attestation_rejects_replayed_stale_challenge_and_tampered_quote_fields() {
        for fault in [
            EvidenceFault::ReplayedChallenge,
            EvidenceFault::Stale,
            EvidenceFault::Measurement,
            EvidenceFault::EvidenceBytes,
        ] {
            let error = provider(fault)
                .key_info(&KeyRef::new("remote", "validator-1", 2))
                .await
                .expect_err("invalid evidence must fail");
            assert_eq!(error.code, SignErrorCode::VerificationFailed);
        }
    }

    #[tokio::test]
    async fn attestation_rejects_real_response_delay_beyond_challenge_age() {
        let error = provider(EvidenceFault::Delayed)
            .key_info(&KeyRef::new("remote", "validator-1", 2))
            .await
            .expect_err("delayed evidence must exceed the challenge age");
        assert_eq!(error.code, SignErrorCode::VerificationFailed);
    }

    #[tokio::test]
    async fn attestation_rejects_evidence_issued_beyond_clock_skew() {
        let error = provider(EvidenceFault::Future)
            .key_info(&KeyRef::new("remote", "validator-1", 2))
            .await
            .expect_err("future evidence beyond configured clock skew must fail");
        assert_eq!(error.code, SignErrorCode::VerificationFailed);
    }

    #[test]
    fn attestation_rejects_challenge_issued_beyond_clock_skew() {
        let key_ref = KeyRef::new("remote", "validator-1", 2);
        let expected = AttestationExpectations {
            challenge: [7; 32],
            challenge_issued_at_unix_ms: 1_011,
            max_challenge_age_ms: 1_000,
            max_clock_skew_ms: 10,
            max_session_age_ms: 10_000,
            service_identity: "signer.prod".to_string(),
            policy_version: "policy-v1".to_string(),
            key_ref: key_ref.clone(),
            measurement: vec![0x11; 32],
            signer_measurement: Some(vec![0x22; 32]),
            product_id: 7,
            minimum_security_version: 5,
        };
        let evidence = AttestationEvidence {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            challenge: expected.challenge,
            session_id: "session-1".to_string(),
            session_public_key: vec![2; 33],
            service_identity: expected.service_identity.clone(),
            policy_version: expected.policy_version.clone(),
            key_ref,
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 2_000,
            measurement: expected.measurement.clone(),
            signer_measurement: expected.signer_measurement.clone().unwrap(),
            product_id: expected.product_id,
            security_version: expected.minimum_security_version,
            debug: false,
            evidence: vec![1],
        };
        let error = RemoteSignerProvider::validate_evidence_fields(&evidence, &expected, 1_000)
            .expect_err("future challenge beyond configured clock skew must fail");
        assert_eq!(error.code, SignErrorCode::VerificationFailed);
    }

    #[tokio::test]
    async fn attestation_rejects_debug_low_svn_expiry_and_binding_mismatches() {
        for fault in [
            EvidenceFault::Debug,
            EvidenceFault::LowSvn,
            EvidenceFault::Expired,
            EvidenceFault::Session,
            EvidenceFault::Key,
            EvidenceFault::Policy,
        ] {
            let error = provider(fault)
                .key_info(&KeyRef::new("remote", "validator-1", 2))
                .await
                .expect_err("invalid evidence must fail");
            assert_eq!(error.code, SignErrorCode::VerificationFailed);
        }
    }

    #[tokio::test]
    async fn attestation_rejects_tcb_collateral_and_revocation_failures() {
        for fault in [
            EvidenceFault::Tcb,
            EvidenceFault::Collateral,
            EvidenceFault::Revoked,
        ] {
            let error = provider(fault)
                .key_info(&KeyRef::new("remote", "validator-1", 2))
                .await
                .expect_err("unacceptable verified platform claim must fail");
            assert_eq!(error.code, SignErrorCode::VerificationFailed);
        }
    }

    #[tokio::test]
    async fn attested_response_rejects_session_mismatch_and_query_is_available() {
        let provider = provider(EvidenceFault::None);
        let key_ref = KeyRef::new("remote", "validator-1", 2);
        provider.key_info(&key_ref).await.expect("attested key");
        let request = RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: "request-1".to_string(),
            key_ref: key_ref.clone(),
            network: 1,
            purpose: SignPurpose::Transaction,
            payload: neo_core::signing::SignPayload::Digest {
                algorithm: neo_core::signing::DigestAlgorithm::Sha256,
                digest: [3; 32],
            },
            request_digest: [4; 32],
            policy_version: "policy-v1".to_string(),
            attestation_session_id: Some("session-1".to_string()),
        };
        let response = provider.transport.sign(&request).await.expect("response");
        let RemoteSignDispatchResult::Succeeded(response) = response else {
            panic!("mock must return a signing response");
        };
        let session = provider.sessions.read().get(&key_ref).cloned().unwrap();
        let error = provider
            .validate_response(&request, &response, Some(&session))
            .expect_err("session mismatch must fail");
        assert_eq!(error.code, SignErrorCode::VerificationFailed);

        let mut wrong_session_key = response;
        wrong_session_key.attestation_session_id = Some(session.session_id.clone());
        let error = provider
            .validate_response(&request, &wrong_session_key, Some(&session))
            .expect_err("response not authenticated by the attested session key must fail");
        assert_eq!(error.code, SignErrorCode::Unauthenticated);

        assert_eq!(
            provider
                .query_unknown("request-1", [4; 32], &key_ref)
                .await
                .expect("query"),
            RemoteOperationStatus::Pending
        );
    }

    #[test]
    fn remote_tee_rejects_clock_skew_above_strict_limit() {
        let key_public = Secp256r1Crypto::derive_public_key(&[0x31; 32]).expect("key public");
        let transport: Arc<dyn RemoteSignerTransport> = Arc::new(MockTransport {
            fault: EvidenceFault::None,
            key_public: key_public.clone(),
            session_public: key_public.clone(),
        });
        let mut config = provider_config(&key_public);
        config
            .remote
            .as_mut()
            .and_then(|remote| remote.attestation.as_mut())
            .expect("attestation policy")
            .max_clock_skew_ms = MAX_ATTESTATION_CLOCK_SKEW_MS + 1;
        let error = RemoteSignerProvider::new(
            &config,
            "policy-v1",
            transport,
            Some(Arc::new(MockVerifier {
                fault: EvidenceFault::None,
            })),
        )
        .err()
        .expect("unbounded clock skew must refuse startup");
        assert!(error.to_string().contains("clock skew no greater"));
    }

    #[test]
    fn remote_tee_never_falls_back_without_a_real_verifier() {
        let key_public = Secp256r1Crypto::derive_public_key(&[0x31; 32]).expect("key public");
        let transport: Arc<dyn RemoteSignerTransport> = Arc::new(MockTransport {
            fault: EvidenceFault::None,
            key_public: key_public.clone(),
            session_public: key_public.clone(),
        });
        let error =
            RemoteSignerProvider::new(&provider_config(&key_public), "policy-v1", transport, None)
                .err()
                .expect("missing verifier must refuse startup");
        assert!(error.to_string().contains("refusing to substitute"));
    }
}
