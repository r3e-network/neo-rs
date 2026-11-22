use super::*;
use crate::signing::config::{RemoteSignerSection, SignerKeySection};
use crate::signing::{
    AuthenticatedContext, DefaultSignerService, DurableSigningStore, IdempotencyInfo, PolicyLimits,
    ProtocolContext, SIGNING_SCHEMA_VERSION, SecurityLevel, SignPayload, SignPurpose, SignRequest,
    SignerService, SigningKeyBinding, SigningKeyResolver, SigningPolicy, SigningRequestStatus,
    SigningStore,
};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request as HttpRequest, Response as HttpResponse, Server};
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

    async fn attest(&self, challenge: &AttestationChallenge) -> SignResult<AttestationEvidence> {
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

    async fn attest(&self, _challenge: &AttestationChallenge) -> SignResult<AttestationEvidence> {
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
            return MtlsRemoteSignerTransport::decode_sign_response(StatusCode::BAD_REQUEST, &body);
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
        canonical_encodings: vec!["neo-n3-sign-data-v1".to_string()],
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
            identity_mapping: BTreeMap::from([("signer.prod".to_string(), "remote".to_string())]),
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
            encoding: "neo-n3-sign-data-v1".to_string(),
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
    let decoded =
        MtlsRemoteSignerTransport::decode_sign_response(StatusCode::BAD_REQUEST, &rejection_body)
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
        payload: crate::signing::SignPayload::Digest {
            algorithm: crate::signing::DigestAlgorithm::Sha256,
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
