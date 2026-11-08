use super::DurableSigningStore;
use super::error::{SignErrorCode, SignResult};
use super::memory_store::InMemorySigningStore;
use super::policy::{PolicyLimits, SigningPolicy};
use super::service::{
    AuthorizedSignRequest, DefaultSignerService, ProviderOperationStatus, SignerProvider,
    SignerProviderRegistry, SignerService, SigningDeadlineRegistry, TrustedSignerClient,
};
use super::store::{AuditOutcome, SigningRequestStatus, SigningStore};
use super::types::{
    ALGORITHM_ECDSA_P256_SHA256, AuthenticatedContext, FrozenSignIntent, IdempotencyInfo, KeyInfo,
    KeyRef, ProtocolContext, ProviderCapabilities, ProviderOutcomeProvenance, ProviderSignature,
    SIGNING_SCHEMA_VERSION, SecurityLevel, SignPayload, SignPurpose, SignRequest,
    SigningKeyBinding, SigningKeyResolver,
};
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use neo_crypto::{Crypto, CryptoResult, Secp256r1Crypto};
use neo_primitives::time_provider::TimeSource;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Barrier;

// Test-only key material; canonical payloads are hashed exactly once before real P-256 signing.
#[derive(Debug)]
struct TestSigningKey {
    private_key: [u8; 32],
    public_key: Vec<u8>,
}

impl TestSigningKey {
    fn from_private_key(private_key: &[u8; 32]) -> CryptoResult<Self> {
        Ok(Self {
            private_key: *private_key,
            public_key: Secp256r1Crypto::derive_public_key(private_key)?,
        })
    }

    fn public_key(&self) -> Vec<u8> {
        self.public_key.clone()
    }

    fn sign(&self, bytes: &[u8]) -> CryptoResult<Vec<u8>> {
        Secp256r1Crypto::sign_prehash(&Crypto::sha256(bytes), &self.private_key)
            .map(|signature| signature.to_vec())
    }
}

#[derive(Debug)]
struct FixedClock(i64);

impl TimeSource for FixedClock {
    fn utc_now(&self) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(self.0)
            .single()
            .expect("valid time")
    }
}

#[derive(Debug)]
struct MockProvider {
    key_info: KeyInfo,
    key_pair: TestSigningKey,
    recovered_signature: ProviderSignature,
    calls: AtomicUsize,
    query_calls: AtomicUsize,
    barrier: Option<Arc<Barrier>>,
}

#[async_trait]
impl SignerProvider for MockProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: "test-provider".to_string(),
            purposes: BTreeSet::from([SignPurpose::Transaction]),
            canonical_encodings: BTreeSet::from(["neo-n3-unsigned-transaction-v1".to_string()]),
            algorithms: BTreeSet::from([ALGORITHM_ECDSA_P256_SHA256.to_string()]),
            security_level: SecurityLevel::Hsm,
            outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
        }
    }

    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo> {
        if key_ref == &self.key_info.key_ref {
            Ok(self.key_info.clone())
        } else {
            Err(super::SignError::new(
                SignErrorCode::KeyNotFound,
                "test key not found",
            ))
        }
    }

    async fn sign_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        let bytes = request.payload().canonical_bytes().ok_or_else(|| {
            super::SignError::new(
                SignErrorCode::InvalidRequest,
                "test provider requires canonical bytes",
            )
        })?;
        Ok(ProviderSignature {
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature: self.key_pair.sign(bytes).map_err(|error| {
                super::SignError::new(SignErrorCode::ProviderFailure, error.to_string())
            })?,
        })
    }

    async fn query_authorized(
        &self,
        _request: &AuthorizedSignRequest,
    ) -> SignResult<super::service::ProviderOperationStatus> {
        self.query_calls.fetch_add(1, Ordering::SeqCst);
        Ok(super::service::ProviderOperationStatus::Succeeded(
            self.recovered_signature.clone(),
        ))
    }
}

struct InvalidSignatureProvider {
    key_info: KeyInfo,
}

struct ResultUnknownProvider {
    key_info: KeyInfo,
    calls: AtomicUsize,
}

struct RecoveryQueryProvider {
    key_info: KeyInfo,
    recovered_signature: ProviderSignature,
    query_delay: Duration,
    sign_calls: AtomicUsize,
    query_calls: AtomicUsize,
    queries: Mutex<Vec<(String, KeyRef, [u8; 32])>>,
}

#[async_trait]
impl SignerProvider for RecoveryQueryProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: self.key_info.key_ref.provider.clone(),
            purposes: BTreeSet::from([SignPurpose::Transaction]),
            canonical_encodings: BTreeSet::from(["neo-n3-unsigned-transaction-v1".to_string()]),
            algorithms: BTreeSet::from([ALGORITHM_ECDSA_P256_SHA256.to_string()]),
            security_level: self.key_info.security_level,
            outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
        }
    }

    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo> {
        if key_ref == &self.key_info.key_ref {
            Ok(self.key_info.clone())
        } else {
            Err(super::SignError::new(
                SignErrorCode::KeyNotFound,
                "historical test key not found",
            ))
        }
    }

    async fn sign_authorized(
        &self,
        _request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature> {
        self.sign_calls.fetch_add(1, Ordering::SeqCst);
        Err(super::SignError::new(
            SignErrorCode::ProviderFailure,
            "recovery provider must never re-dispatch signing",
        ))
    }

    async fn query_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderOperationStatus> {
        self.query_calls.fetch_add(1, Ordering::SeqCst);
        self.queries.lock().expect("recovery query lock").push((
            request.request_id().to_string(),
            request.key_ref().clone(),
            *request.request_digest(),
        ));
        tokio::time::sleep(self.query_delay).await;
        Ok(ProviderOperationStatus::Succeeded(
            self.recovered_signature.clone(),
        ))
    }
}

#[async_trait]
impl SignerProvider for ResultUnknownProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: "test-provider".to_string(),
            purposes: BTreeSet::from([SignPurpose::Transaction]),
            canonical_encodings: BTreeSet::from(["neo-n3-unsigned-transaction-v1".to_string()]),
            algorithms: BTreeSet::from([ALGORITHM_ECDSA_P256_SHA256.to_string()]),
            security_level: SecurityLevel::Hsm,
            outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
        }
    }

    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo> {
        if key_ref == &self.key_info.key_ref {
            Ok(self.key_info.clone())
        } else {
            Err(super::SignError::new(
                SignErrorCode::KeyNotFound,
                "test key not found",
            ))
        }
    }

    async fn sign_authorized(
        &self,
        _request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(
            super::SignError::new(SignErrorCode::ResultUnknown, "response lost after dispatch")
                .retryable(true),
        )
    }
}

#[async_trait]
impl SignerProvider for InvalidSignatureProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: "test-provider".to_string(),
            purposes: BTreeSet::from([SignPurpose::Transaction]),
            canonical_encodings: BTreeSet::from(["neo-n3-unsigned-transaction-v1".to_string()]),
            algorithms: BTreeSet::from([ALGORITHM_ECDSA_P256_SHA256.to_string()]),
            security_level: SecurityLevel::Hsm,
            outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
        }
    }

    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo> {
        if key_ref == &self.key_info.key_ref {
            Ok(self.key_info.clone())
        } else {
            Err(super::SignError::new(
                SignErrorCode::KeyNotFound,
                "test key not found",
            ))
        }
    }

    async fn sign_authorized(
        &self,
        _request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature> {
        Ok(ProviderSignature {
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature: vec![0; 64],
        })
    }
}

fn key_ref() -> KeyRef {
    KeyRef::new("test-provider", "validator-1", 7)
}

fn test_key_pair() -> TestSigningKey {
    TestSigningKey::from_private_key(&[1u8; 32]).expect("valid test key")
}

fn key_info(key_pair: &TestSigningKey) -> KeyInfo {
    KeyInfo {
        schema_version: SIGNING_SCHEMA_VERSION,
        key_ref: key_ref(),
        algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
        public_key: key_pair.public_key(),
        security_level: SecurityLevel::Hsm,
        enabled: true,
        attributes: BTreeMap::new(),
    }
}

fn context() -> AuthenticatedContext {
    AuthenticatedContext {
        schema_version: SIGNING_SCHEMA_VERSION,
        principal: "service-a".to_string(),
        tenant: "tenant-a".to_string(),
        authentication_method: "mtls".to_string(),
        scopes: BTreeSet::from(["sign:transaction".to_string()]),
        claims: BTreeMap::new(),
    }
}

fn request(id: &str) -> SignRequest {
    SignRequest {
        schema_version: SIGNING_SCHEMA_VERSION,
        request_id: id.to_string(),
        network: 860_833_102,
        purpose: SignPurpose::Transaction,
        key_ref: key_ref(),
        payload: SignPayload::Canonical {
            encoding: "neo-n3-unsigned-transaction-v1".to_string(),
            bytes: vec![1, 2, 3, 4],
        },
        protocol_context: ProtocolContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            protocol: "neo-n3-transaction".to_string(),
            protocol_version: "1".to_string(),
            fields: BTreeMap::from([("chain_id".to_string(), "mainnet".to_string())]),
        },
        policy_version: "policy-v1".to_string(),
        deadline_unix_ms: 2_000,
        idempotency: IdempotencyInfo {
            schema_version: SIGNING_SCHEMA_VERSION,
            key: format!("idem-{id}"),
            replay_nonce: format!("nonce-{id}"),
        },
        fee_units: 10,
        quota_units: 1,
        account: Some("account-a".to_string()),
        targets: BTreeSet::from(["contract-a".to_string()]),
    }
}

fn frozen_intent() -> FrozenSignIntent {
    let request = request("intent-template");
    FrozenSignIntent {
        schema_version: request.schema_version,
        network: request.network,
        purpose: request.purpose,
        key_ref: request.key_ref,
        payload: request.payload,
        protocol_context: request.protocol_context,
        policy_version: request.policy_version,
        fee_units: request.fee_units,
        quota_units: request.quota_units,
        account: request.account,
        targets: request.targets,
    }
}

fn test_key_resolver() -> SigningKeyResolver {
    SigningKeyResolver::new([SigningKeyBinding {
        account: "account-a".to_string(),
        purpose: SignPurpose::Transaction,
        key_ref: key_ref(),
        public_key: test_key_pair().public_key(),
    }])
    .expect("valid test key binding")
}

fn allow_policy(quota_limit: u64) -> SigningPolicy {
    let mut policy = SigningPolicy {
        version: "policy-v1".to_string(),
        key_resolver: test_key_resolver(),
        limits: PolicyLimits {
            max_fee_units: 100,
            quota_limit_units: quota_limit,
            quota_window_ms: 10_000,
        },
        ..SigningPolicy::default()
    };
    policy.principals.allow.insert("service-a".to_string());
    policy.tenants.allow.insert("tenant-a".to_string());
    policy.keys.allow.insert(key_ref().selector());
    policy.networks.allow.insert(860_833_102);
    policy.purposes.allow.insert(SignPurpose::Transaction);
    policy
        .authorizations
        .allow
        .insert(SigningPolicy::authorization_selector(
            "service-a",
            "tenant-a",
            &key_ref(),
            860_833_102,
            &SignPurpose::Transaction,
            "sign:transaction",
        ));
    policy.security_levels.allow.insert(SecurityLevel::Hsm);
    policy.accounts.allow.insert("account-a".to_string());
    policy.targets.allow.insert("contract-a".to_string());
    policy
}

fn service(
    policy: SigningPolicy,
    barrier: Option<Arc<Barrier>>,
) -> (
    Arc<DefaultSignerService>,
    Arc<MockProvider>,
    Arc<InMemorySigningStore>,
) {
    let key_pair = test_key_pair();
    let recovered_signature = ProviderSignature {
        algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
        signature: key_pair
            .sign(&[1, 2, 3, 4])
            .expect("precomputed provider result"),
    };
    let provider = Arc::new(MockProvider {
        key_info: key_info(&key_pair),
        key_pair,
        recovered_signature,
        calls: AtomicUsize::new(0),
        query_calls: AtomicUsize::new(0),
        barrier,
    });
    let store = Arc::new(InMemorySigningStore::default());
    let provider_trait: Arc<dyn SignerProvider> = provider.clone();
    let store_trait: Arc<dyn SigningStore> = store.clone();
    let service = Arc::new(DefaultSignerService::with_clock(
        provider_trait,
        store_trait,
        Arc::new(policy),
        Arc::new(FixedClock(1_000)),
    ));
    (service, provider, store)
}

#[test]
fn registry_rejects_duplicate_names_and_never_falls_back() {
    let key_pair = test_key_pair();
    let recovered_signature = ProviderSignature {
        algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
        signature: key_pair
            .sign(&[1, 2, 3, 4])
            .expect("precomputed provider result"),
    };
    let provider = Arc::new(MockProvider {
        key_info: key_info(&key_pair),
        key_pair,
        recovered_signature,
        calls: AtomicUsize::new(0),
        query_calls: AtomicUsize::new(0),
        barrier: None,
    });
    let mut registry = SignerProviderRegistry::new();
    registry
        .register(provider.clone())
        .expect("first registration");
    let duplicate = registry
        .register(provider)
        .expect_err("duplicate provider must fail");
    assert_eq!(duplicate.code, SignErrorCode::InvalidRequest);

    let missing = registry
        .provider_for(&KeyRef::new("other-provider", "validator-1", 7))
        .err()
        .expect("unknown provider must not fall back");
    assert_eq!(missing.code, SignErrorCode::CapabilityUnavailable);
}

#[test]
fn authorization_defaults_to_deny_and_explicit_policy_allows() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (denied_service, denied_provider, denied_store) =
            service(SigningPolicy::default(), None);
        let error = denied_service
            .sign(request("denied"), context())
            .await
            .expect_err("default policy must deny");
        assert_eq!(error.code, SignErrorCode::PolicyDenied);
        assert_eq!(denied_provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            denied_store.audits().last().unwrap().outcome,
            AuditOutcome::Denied
        );

        let (allowed_service, allowed_provider, _) = service(allow_policy(10), None);
        let response = allowed_service
            .sign(request("allowed"), context())
            .await
            .expect("explicit policy authorizes");
        assert!(!response.signature.is_empty());
        assert_eq!(allowed_provider.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn purpose_scope_must_be_exact_and_joint_authorization_must_match() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (wrong_scope_service, wrong_scope_provider, _) = service(allow_policy(10), None);
        let mut wrong_scope = context();
        wrong_scope.scopes = BTreeSet::from(["sign:consensus".to_string()]);
        let error = wrong_scope_service
            .sign(request("wrong-scope"), wrong_scope)
            .await
            .expect_err("a different purpose scope must be rejected");
        assert_eq!(error.code, SignErrorCode::PolicyDenied);
        assert_eq!(wrong_scope_provider.calls.load(Ordering::SeqCst), 0);

        let (extra_scope_service, extra_scope_provider, _) = service(allow_policy(10), None);
        let mut extra_scope = context();
        extra_scope.scopes.insert("sign:consensus".to_string());
        let error = extra_scope_service
            .sign(request("extra-scope"), extra_scope)
            .await
            .expect_err("additional scopes must not broaden signing authority");
        assert_eq!(error.code, SignErrorCode::PolicyDenied);
        assert_eq!(extra_scope_provider.calls.load(Ordering::SeqCst), 0);

        let mut joint_policy = allow_policy(10);
        joint_policy
            .principals
            .allow
            .insert("service-b".to_string());
        let (joint_service, joint_provider, _) = service(joint_policy, None);
        let mut joint_context = context();
        joint_context.principal = "service-b".to_string();
        let error = joint_service
            .sign(request("joint-mismatch"), joint_context)
            .await
            .expect_err("individually allowed dimensions must not bypass the joint tuple");
        assert_eq!(error.code, SignErrorCode::PolicyDenied);
        assert_eq!(joint_provider.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn deny_list_takes_priority_over_allow_list() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let mut policy = allow_policy(10);
        policy.principals.deny.insert("service-a".to_string());
        let (service, provider, _) = service(policy, None);
        let error = service
            .sign(request("deny-priority"), context())
            .await
            .expect_err("deny must win");
        assert_eq!(error.code, SignErrorCode::PolicyDenied);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn same_request_id_and_digest_returns_cached_response() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (service, provider, _) = service(allow_policy(10), None);
        let first = service
            .sign(request("same"), context())
            .await
            .expect("first signing");
        let second = service
            .sign(request("same"), context())
            .await
            .expect("idempotent replay");
        assert_eq!(first.signature, second.signature);
        assert!(!first.idempotent_replay);
        assert!(second.idempotent_replay);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn same_operation_with_changed_deadline_is_rejected() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (service, provider, _) = service(allow_policy(10), None);
        service
            .sign(request("deadline-conflict"), context())
            .await
            .expect("first signing");
        let mut retry = request("deadline-conflict");
        retry.deadline_unix_ms = 9_000;
        let error = service
            .sign(retry, context())
            .await
            .expect_err("changed deadline must conflict with the first authorization");
        assert_eq!(error.code, SignErrorCode::IdempotencyConflict);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn expired_operation_cannot_be_extended_by_rebuilding_the_request() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (service, provider, store) = service(allow_policy(10), None);
        service
            .sign(request("expired-rebuild"), context())
            .await
            .expect("first signing");

        let provider_trait: Arc<dyn SignerProvider> = provider.clone();
        let store_trait: Arc<dyn SigningStore> = store;
        let expired_service = DefaultSignerService::with_clock(
            provider_trait,
            store_trait,
            Arc::new(allow_policy(10)),
            Arc::new(FixedClock(3_000)),
        );
        let expired = expired_service
            .sign(request("expired-rebuild"), context())
            .await
            .expect_err("the original stable deadline must expire");
        assert_eq!(expired.code, SignErrorCode::DeadlineExceeded);

        let mut refreshed = request("expired-rebuild");
        refreshed.deadline_unix_ms = 4_000;
        let conflict = expired_service
            .sign(refreshed, context())
            .await
            .expect_err("a rebuilt request must not extend the old authorization");
        assert_eq!(conflict.code, SignErrorCode::IdempotencyConflict);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn deadline_registry_reuses_the_first_deadline_across_clones() {
    let registry = SigningDeadlineRegistry::default();
    let first = registry
        .resolve("stable-operation", 1_000, Duration::from_millis(500))
        .expect("first deadline");
    let retry = registry
        .clone()
        .resolve("stable-operation", 9_000, Duration::from_millis(500))
        .expect("stable retry deadline");
    assert_eq!(first, 1_500);
    assert_eq!(retry, first);
}

#[test]
fn trusted_client_clones_reuse_the_first_operation_deadline() {
    let (service, _, _) = service(allow_policy(10), None);
    let signer_service: Arc<dyn SignerService> = service;
    let client = TrustedSignerClient::new(
        signer_service,
        "test-provider",
        "policy-v1",
        Duration::from_millis(500),
        context(),
        Arc::new(test_key_resolver()),
    )
    .expect("trusted client");
    let first = client
        .deadline_for("service-operation", 1_000)
        .expect("first deadline");
    let retry = client
        .clone()
        .deadline_for("service-operation", 9_000)
        .expect("retry deadline");
    assert_eq!(first, 1_500);
    assert_eq!(retry, first);
}

#[test]
fn same_request_id_with_different_digest_is_rejected() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (service, provider, _) = service(allow_policy(10), None);
        service
            .sign(request("conflict"), context())
            .await
            .expect("first signing");
        let mut changed = request("conflict");
        changed.payload = SignPayload::Canonical {
            encoding: "neo-n3-unsigned-transaction-v1".to_string(),
            bytes: vec![9, 9, 9],
        };
        let error = service
            .sign(changed, context())
            .await
            .expect_err("different digest must conflict");
        assert_eq!(error.code, SignErrorCode::IdempotencyConflict);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn expired_request_is_rejected_before_provider_access() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let (service, provider, _) = service(allow_policy(10), None);
        let mut expired = request("expired");
        expired.deadline_unix_ms = 1_000;
        let error = service
            .sign(expired, context())
            .await
            .expect_err("expired request");
        assert_eq!(error.code, SignErrorCode::DeadlineExceeded);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn invalid_provider_signature_is_independently_rejected() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let key_pair = test_key_pair();
        let provider: Arc<dyn SignerProvider> = Arc::new(InvalidSignatureProvider {
            key_info: key_info(&key_pair),
        });
        let store = Arc::new(InMemorySigningStore::default());
        let store_trait: Arc<dyn SigningStore> = store.clone();
        let service = DefaultSignerService::with_clock(
            provider,
            store_trait,
            Arc::new(allow_policy(10)),
            Arc::new(FixedClock(1_000)),
        );
        let error = service
            .sign(request("invalid-signature"), context())
            .await
            .expect_err("invalid provider signature must fail verification");
        assert_eq!(error.code, SignErrorCode::VerificationFailed);
        assert!(matches!(
            store
                .load("invalid-signature")
                .await
                .expect("load deterministic failure")
                .unwrap()
                .status,
            SigningRequestStatus::Failed(_)
        ));
    });
}

#[test]
fn provider_reported_result_unknown_remains_pending_without_redispatch() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let key_pair = test_key_pair();
        let provider = Arc::new(ResultUnknownProvider {
            key_info: key_info(&key_pair),
            calls: AtomicUsize::new(0),
        });
        let store = Arc::new(InMemorySigningStore::default());
        let provider_trait: Arc<dyn SignerProvider> = provider.clone();
        let store_trait: Arc<dyn SigningStore> = store.clone();
        let service = DefaultSignerService::with_clock(
            provider_trait,
            store_trait,
            Arc::new(allow_policy(10)),
            Arc::new(FixedClock(1_000)),
        );
        let original = request("provider-unknown");
        let error = service
            .sign(original.clone(), context())
            .await
            .expect_err("post-dispatch uncertainty must be surfaced");
        assert_eq!(error.code, SignErrorCode::ResultUnknown);
        assert!(error.retryable);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        let digest = original.binding_digest(&context()).expect("request digest");
        assert_eq!(
            store
                .status("provider-unknown", "test-provider", digest)
                .await
                .expect("pending status"),
            SigningRequestStatus::Pending
        );
        let retry = service
            .sign(original, context())
            .await
            .expect_err("unknown result must not be dispatched again");
        assert_eq!(retry.code, SignErrorCode::RequestInProgress);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn timeout_after_provider_dispatch_remains_result_unknown() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let barrier = Arc::new(Barrier::new(2));
        let (service, provider, store) = service(allow_policy(10), Some(barrier));
        let mut timed = request("provider-timeout");
        timed.deadline_unix_ms = 1_001;
        let error = service
            .sign(timed.clone(), context())
            .await
            .expect_err("provider timeout must be unknown");
        assert_eq!(error.code, SignErrorCode::ResultUnknown);
        assert!(error.retryable);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        let retry = service
            .sign(timed.clone(), context())
            .await
            .expect_err("unknown result must remain pending");
        assert_eq!(retry.code, SignErrorCode::RequestInProgress);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        let provider_trait: Arc<dyn SignerProvider> = provider.clone();
        let store_trait: Arc<dyn SigningStore> = store;
        let restarted = DefaultSignerService::with_clock(
            provider_trait,
            store_trait,
            Arc::new(allow_policy(10)),
            Arc::new(FixedClock(3_000)),
        );
        let recovered = restarted
            .recover_operation(&timed.request_id, context())
            .await
            .expect("restart recovery queries the original expired request");
        assert!(matches!(recovered, SigningRequestStatus::Succeeded(_)));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.query_calls.load(Ordering::SeqCst), 1);

        let persisted = restarted
            .recover_operation(&timed.request_id, context())
            .await
            .expect("persisted recovery result");
        assert!(matches!(persisted, SigningRequestStatus::Succeeded(_)));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.query_calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn durable_resume_uses_historical_binding_after_expiry_and_query_timeout() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let directory = tempfile::tempdir().expect("temporary signing directory");
        let path = directory.path().join("resume-signing.jsonl");
        let key_pair = test_key_pair();
        let initial_provider = Arc::new(ResultUnknownProvider {
            key_info: key_info(&key_pair),
            calls: AtomicUsize::new(0),
        });
        let intent = frozen_intent();
        let operation_id = "durable-expired-operation";
        let original_digest;

        {
            let store = Arc::new(DurableSigningStore::open(&path).expect("open durable store"));
            let service = DefaultSignerService::with_clock(
                initial_provider.clone(),
                store.clone(),
                Arc::new(allow_policy(10)),
                Arc::new(FixedClock(1_000)),
            );
            let error = service
                .resume_or_start(
                    operation_id,
                    intent.clone(),
                    context(),
                    Duration::from_millis(1),
                )
                .await
                .expect_err("lost provider response must remain pending");
            assert_eq!(error.code, SignErrorCode::ResultUnknown);
            assert_eq!(initial_provider.calls.load(Ordering::SeqCst), 1);
            let persisted = store
                .load(operation_id)
                .await
                .expect("load original operation")
                .expect("durable pending operation");
            assert_eq!(persisted.request.deadline_unix_ms, 1_001);
            assert_eq!(persisted.key_info.key_ref, key_ref());
            original_digest = persisted.request_digest;
        }

        let recovered_signature = ProviderSignature {
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature: key_pair
                .sign(&[1, 2, 3, 4])
                .expect("historical provider signature"),
        };
        let recovery_provider = Arc::new(RecoveryQueryProvider {
            key_info: key_info(&key_pair),
            recovered_signature,
            query_delay: Duration::from_millis(20),
            sign_calls: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            queries: Mutex::new(Vec::new()),
        });
        let store = Arc::new(DurableSigningStore::open(&path).expect("reopen durable store"));
        let mut changed_policy = allow_policy(10);
        changed_policy.key_resolver = SigningKeyResolver::new([SigningKeyBinding {
            account: "account-a".to_string(),
            purpose: SignPurpose::Transaction,
            key_ref: KeyRef::new("new-default-provider", "rotated-key", 9),
            public_key: TestSigningKey::from_private_key(&[2u8; 32])
                .expect("rotated key")
                .public_key(),
        }])
        .expect("rotated active key binding");
        let restarted = DefaultSignerService::with_clock(
            recovery_provider.clone(),
            store.clone(),
            Arc::new(changed_policy),
            Arc::new(FixedClock(3_000)),
        );

        let timeout = restarted
            .resume_or_start(
                operation_id,
                intent.clone(),
                context(),
                Duration::from_millis(1),
            )
            .await
            .expect_err("bounded provider query must time out");
        assert_eq!(timeout.code, SignErrorCode::ResultUnknown);
        assert!(timeout.retryable);
        assert!(matches!(
            store
                .load(operation_id)
                .await
                .expect("load after query timeout")
                .expect("operation remains durable")
                .status,
            SigningRequestStatus::Pending
        ));

        let recovered = restarted
            .resume_or_start(
                operation_id,
                intent.clone(),
                context(),
                Duration::from_millis(100),
            )
            .await
            .expect("expired operation must query its historical provider binding");
        assert_eq!(recovered.key_ref, key_ref());
        assert_eq!(recovered.request_digest, original_digest);
        assert_eq!(initial_provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(recovery_provider.sign_calls.load(Ordering::SeqCst), 0);
        assert_eq!(recovery_provider.query_calls.load(Ordering::SeqCst), 2);
        let queries = recovery_provider
            .queries
            .lock()
            .expect("recovery query lock");
        assert!(queries.iter().all(|(request_id, queried_key, digest)| {
            request_id == operation_id && queried_key == &key_ref() && digest == &original_digest
        }));
        drop(queries);

        let mut changed_intent = intent;
        changed_intent.key_ref = KeyRef::new("new-default-provider", "rotated-key", 9);
        let conflict = restarted
            .resume_or_start(
                operation_id,
                changed_intent,
                context(),
                Duration::from_millis(100),
            )
            .await
            .expect_err("changed intent must not reuse a recovered operation");
        assert_eq!(conflict.code, SignErrorCode::IdempotencyConflict);
        assert_eq!(initial_provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(recovery_provider.sign_calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn concurrent_quota_reservations_are_atomic() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let barrier = Arc::new(Barrier::new(2));
        let (service, provider, _) = service(allow_policy(2), Some(barrier));
        let mut tasks = Vec::new();
        for id in ["quota-1", "quota-2", "quota-3"] {
            let service = service.clone();
            let request = request(id);
            tasks.push(tokio::spawn(async move {
                service.sign(request, context()).await
            }));
        }
        let mut successes = 0;
        let mut limit_errors = 0;
        for task in tasks {
            match task.await.expect("task join") {
                Ok(_) => successes += 1,
                Err(error) if error.code == SignErrorCode::LimitExceeded => limit_errors += 1,
                Err(error) => panic!("unexpected error: {error:?}"),
            }
        }
        assert_eq!(successes, 2);
        assert_eq!(limit_errors, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    });
}
