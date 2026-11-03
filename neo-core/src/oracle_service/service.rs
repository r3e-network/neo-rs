//! Oracle service implementation (parity with Neo.Plugins.OracleService).

mod handlers;
mod lifecycle;
mod processing;
mod transactions;
mod utils;

#[cfg(test)]
mod tests;

use crate::cryptography::ECPoint;
use crate::neo_system::NeoSystem;
use crate::network::p2p::payloads::Transaction;
use crate::signing::{
    FrozenSignIntent, ProtocolContext, SIGNING_SCHEMA_VERSION, SignPayload, SignPurpose,
    TrustedSignerClient,
};
use crate::wallets::Wallet;
use parking_lot::{Mutex, RwLock};
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hash;
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::task::JoinHandle;

pub(super) use super::OracleServiceSettings;
#[cfg(feature = "oracle")]
pub(super) use super::https::OracleHttpsProtocol;
#[cfg(feature = "oracle")]
pub(super) use super::neofs::OracleNeoFsProtocol;

const REFRESH_INTERVAL: Duration = Duration::from_secs(3 * 60);
const FINISHED_CACHE_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const FILTER_MAX_NEST: usize = 64;
#[cfg(feature = "oracle")]
const SIGNATURE_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// TTL for request deduplication cache (5 minutes).
const DEDUP_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Lifecycle state of the oracle service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    /// Created but never started.
    Unstarted,
    /// Currently processing oracle requests.
    Running,
    /// Explicitly stopped after running.
    Stopped,
}

impl OracleStatus {
    fn as_u8(self) -> u8 {
        match self {
            OracleStatus::Unstarted => 0,
            OracleStatus::Running => 1,
            OracleStatus::Stopped => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => OracleStatus::Running,
            2 => OracleStatus::Stopped,
            _ => OracleStatus::Unstarted,
        }
    }
}

/// Errors raised while processing oracle requests and responses.
#[derive(Debug, Error)]
pub enum OracleServiceError {
    /// The service is disabled by configuration.
    #[error("oracle service disabled")]
    Disabled,
    /// The request was already responded to and retired.
    #[error("oracle request already finished")]
    RequestFinished,
    /// No active oracle request matches the given id.
    #[error("oracle request not found")]
    RequestNotFound,
    /// This node is not among the designated responders for the request.
    #[error("oracle not designated: {0}")]
    NotDesignated(String),
    /// A submitted signature failed verification.
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    /// The supplied oracle public key is malformed.
    #[error("invalid oracle public key")]
    InvalidOraclePublicKey,
    /// The request transaction is absent from the ledger.
    #[error("oracle request transaction not found")]
    RequestTransactionNotFound,
    /// Building the response transaction failed.
    #[error("oracle response build failed: {0}")]
    BuildFailed(String),
    /// An internal processing step failed.
    #[error("oracle processing error: {0}")]
    Processing(String),
    /// The same request was submitted twice within the dedup window.
    #[error("duplicate request")]
    DuplicateRequest,
    /// The request URL was rejected by the SSRF/whitelist policy.
    #[error("URL blocked by security policy")]
    UrlBlocked,
}

struct OracleTask {
    tx: Option<Transaction>,
    backup_tx: Option<Transaction>,
    signs: BTreeMap<ECPoint, Vec<u8>>,
    backup_signs: BTreeMap<ECPoint, Vec<u8>>,
    timestamp: SystemTime,
}

struct ExpiringSet<T> {
    entries: HashMap<T, SystemTime>,
    ttl: Duration,
}

impl<T> ExpiringSet<T>
where
    T: Eq + Hash,
{
    fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    fn insert_at(&mut self, key: T, timestamp: SystemTime) {
        self.entries.insert(key, timestamp);
    }

    fn contains<Q>(&self, key: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.contains_key(key)
    }

    fn contains_fresh<Q>(&self, key: &Q, now: SystemTime) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.get(key).is_some_and(|timestamp| {
            now.duration_since(*timestamp)
                .is_ok_and(|elapsed| elapsed < self.ttl)
        })
    }

    fn prune_expired(&mut self, now: SystemTime, boundary: ExpiryBoundary) {
        let ttl = self.ttl;
        self.entries.retain(|_, timestamp| {
            now.duration_since(*timestamp)
                .map_or(true, |elapsed| boundary.retains(elapsed, ttl))
        });
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone, Copy)]
enum ExpiryBoundary {
    Exclusive,
    Inclusive,
}

impl ExpiryBoundary {
    fn retains(self, elapsed: Duration, ttl: Duration) -> bool {
        match self {
            Self::Exclusive => elapsed < ttl,
            Self::Inclusive => elapsed <= ttl,
        }
    }
}

struct OracleDedupState {
    completed: ExpiringSet<String>,
    in_flight: HashSet<String>,
}

impl Default for OracleDedupState {
    fn default() -> Self {
        Self {
            completed: ExpiringSet::new(DEDUP_CACHE_TTL),
            in_flight: HashSet::new(),
        }
    }
}

impl OracleDedupState {
    fn prune_expired_completed(&mut self, now: SystemTime) {
        self.completed.prune_expired(now, ExpiryBoundary::Exclusive);
    }

    fn is_recent_completed(&self, url: &str, now: SystemTime) -> bool {
        self.completed.contains_fresh(url, now)
    }

    fn start(&mut self, url: &str) {
        self.in_flight.insert(url.to_string());
    }

    fn complete(&mut self, url: &str, timestamp: SystemTime) {
        self.in_flight.remove(url);
        self.completed.insert_at(url.to_string(), timestamp);
    }

    fn cleanup_in_flight(&mut self, url: &str) {
        self.in_flight.remove(url);
    }
}

/// Oracle service runtime.
pub struct OracleService {
    settings: OracleServiceSettings,
    system: Arc<NeoSystem>,
    status: AtomicU8,
    self_ref: RwLock<Weak<OracleService>>,
    wallet: RwLock<Option<Arc<dyn Wallet>>>,
    signer: RwLock<Option<TrustedSignerClient>>,
    pending_queue: Mutex<HashMap<u64, OracleTask>>,
    finished_cache: Mutex<ExpiringSet<u64>>,
    /// Deduplication state for completed and in-flight request URLs.
    dedup: Mutex<OracleDedupState>,
    cancel: AtomicBool,
    request_task: Mutex<Option<JoinHandle<()>>>,
    timer_task: Mutex<Option<JoinHandle<()>>>,
    #[cfg(feature = "oracle")]
    counter: AtomicU64,
    #[cfg(feature = "oracle")]
    https: OracleHttpsProtocol,
    #[cfg(feature = "oracle")]
    neofs: OracleNeoFsProtocol,
}

impl OracleService {
    /// Installs the immutable, startup-authenticated signer client.
    pub fn set_signer_client(&self, signer: TrustedSignerClient) {
        *self.signer.write() = Some(signer);
    }

    fn build_sign_intent(
        &self,
        signer: &TrustedSignerClient,
        account: &crate::UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<FrozenSignIntent, OracleServiceError> {
        let account_selector = account.to_string();
        let key_ref = signer
            .resolve_key(&account_selector, &SignPurpose::OracleResponse)
            .map_err(|error| OracleServiceError::Processing(error.to_string()))?;
        Ok(FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: self.settings.network,
            purpose: SignPurpose::OracleResponse,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: payload,
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: protocol.to_string(),
                protocol_version: "1".to_string(),
                fields,
            },
            policy_version: signer.policy_version().to_string(),
            fee_units: 0,
            quota_units: 1,
            account: Some(account_selector),
            targets: BTreeSet::new(),
        })
    }

    async fn sign_neo_payload(
        &self,
        operation_id: String,
        account: crate::UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<Vec<u8>, OracleServiceError> {
        let signer = self.signer.read().clone().ok_or_else(|| {
            OracleServiceError::Processing(
                "oracle signing is disabled until a trusted signer service is installed"
                    .to_string(),
            )
        })?;
        let intent = self.build_sign_intent(&signer, &account, payload, protocol, fields)?;
        signer
            .resume_or_start(&operation_id, intent)
            .await
            .map(|response| response.signature)
            .map_err(|error| OracleServiceError::Processing(error.to_string()))
    }

    /// Checks if a request is a duplicate and should be skipped.
    /// Returns true if the request is a duplicate.
    pub fn is_duplicate_request(&self, request_id: u64, url: &str) -> bool {
        if !self.settings.enable_deduplication {
            return false;
        }

        let now = SystemTime::now();
        let mut dedup = self.dedup.lock();

        // Clean up expired entries
        dedup.prune_expired_completed(now);

        // Check if URL is currently being processed
        if dedup.in_flight.contains(url) {
            tracing::debug!(
                target: "neo::oracle",
                request_id,
                url = %url,
                "Request already in-flight"
            );
            return true;
        }

        // Check if we've seen this URL recently
        if dedup.is_recent_completed(url, now) {
            tracing::debug!(
                target: "neo::oracle",
                request_id,
                url = %url,
                "Duplicate request detected (recent)"
            );
            return true;
        }

        // Mark URL as in-flight
        dedup.start(url);

        false
    }

    /// Marks a request as completed and removes it from in-flight.
    pub fn mark_request_completed(&self, request_id: u64, url: &str) {
        self.dedup.lock().complete(url, SystemTime::now());

        tracing::debug!(
            target: "neo::oracle",
            request_id,
            url = %url,
            "Request marked as completed"
        );
    }

    /// Cleans up in-flight requests (call on error/timeout).
    pub fn cleanup_in_flight(&self, url: &str) {
        self.dedup.lock().cleanup_in_flight(url);
    }

    /// Validates a URL against security policies.
    pub fn validate_url(&self, url: &str) -> Result<(), OracleServiceError> {
        // Check whitelist/blacklist
        if !self.settings.is_url_allowed(url) {
            return Err(OracleServiceError::UrlBlocked);
        }

        // Additional SSRF validation (sync version for pre-check)
        #[cfg(feature = "oracle")]
        {
            if let Err(reason) = super::https::security::validate_url_for_ssrf(url) {
                tracing::warn!(
                    target: "neo::oracle",
                    url = %url,
                    reason = %reason,
                    "URL failed SSRF validation"
                );
                return Err(OracleServiceError::UrlBlocked);
            }
        }

        Ok(())
    }

    /// Gets the current deduplication cache size (for monitoring).
    pub fn dedup_cache_size(&self) -> usize {
        self.dedup.lock().completed.len()
    }

    /// Gets the current in-flight request count (for monitoring).
    pub fn in_flight_count(&self) -> usize {
        self.dedup.lock().in_flight.len()
    }
}

#[cfg(test)]
mod task34_signing_tests {
    use super::{OracleService, OracleServiceError, OracleServiceSettings};
    use crate::protocol_settings::ProtocolSettings;
    use crate::signing::{
        AuthenticatedContext, FrozenSignIntent, IdempotencyInfo, KeyRef, ProtocolContext,
        SIGNING_SCHEMA_VERSION, SignError, SignErrorCode, SignPayload, SignPurpose, SignRequest,
        SignResponse, SignResult, SignerService, SigningKeyBinding, SigningKeyResolver,
        TrustedSignerClient,
    };
    use crate::wallets::KeyPair;
    use async_trait::async_trait;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const TEST_DEADLINE_UNIX_MS: i64 = 1_800_000_005_000;
    const TEST_PROTOCOL: &str = "neo-n3-oracle-response-submission";
    const TEST_POLICY_VERSION: &str = "oracle-policy-v5";

    struct RecordingOracleSigner {
        registered_key_ref: KeyRef,
        requests: Mutex<Vec<(SignRequest, AuthenticatedContext)>>,
    }

    #[async_trait]
    impl SignerService for RecordingOracleSigner {
        async fn sign(
            &self,
            request: SignRequest,
            context: AuthenticatedContext,
        ) -> SignResult<SignResponse> {
            let request_digest = request.binding_digest(&context)?;
            self.requests
                .lock()
                .expect("task 34 Oracle recording lock")
                .push((request.clone(), context));
            if request.key_ref != self.registered_key_ref {
                return Err(SignError::new(
                    SignErrorCode::KeyNotFound,
                    "task 34 Oracle signer key is not registered",
                ));
            }
            Ok(SignResponse {
                schema_version: SIGNING_SCHEMA_VERSION,
                request_id: request.request_id.clone(),
                key_ref: request.key_ref.clone(),
                algorithm: crate::signing::ALGORITHM_ECDSA_P256_SHA256.to_string(),
                signature: vec![0x34; 64],
                signed_digest: request.payload.signing_digest(),
                request_digest,
                policy_version: request.policy_version.clone(),
                completed_at_unix_ms: TEST_DEADLINE_UNIX_MS,
                idempotent_replay: false,
            })
        }

        async fn resume_or_start(
            &self,
            operation_id: &str,
            frozen_intent: FrozenSignIntent,
            context: AuthenticatedContext,
            _request_timeout: Duration,
        ) -> SignResult<SignResponse> {
            self.sign(
                frozen_intent.into_request(operation_id, TEST_DEADLINE_UNIX_MS)?,
                context,
            )
            .await
        }
    }

    struct OracleSigningHarness {
        service: OracleService,
        recording: Arc<RecordingOracleSigner>,
        account: crate::UInt160,
        network: u32,
        context: AuthenticatedContext,
    }

    fn oracle_context() -> AuthenticatedContext {
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: "task-34-oracle".to_string(),
            tenant: "task-34-node".to_string(),
            authentication_method: "local-test".to_string(),
            scopes: BTreeSet::from([SignPurpose::OracleResponse.required_scope()]),
            claims: BTreeMap::new(),
        }
    }

    fn signing_harness(
        resolved_key_ref: KeyRef,
        registered_key_ref: KeyRef,
        private_key_byte: u8,
    ) -> OracleSigningHarness {
        let protocol = ProtocolSettings::testnet();
        let network = protocol.network;
        let system = crate::neo_system::NeoSystem::new(protocol, None, None).expect("Neo system");
        let service = OracleService::new(
            OracleServiceSettings {
                network,
                ..Default::default()
            },
            system,
        )
        .expect("Oracle service");
        let key_pair =
            KeyPair::from_private_key(&[private_key_byte; 32]).expect("task 34 Oracle key");
        let account = key_pair.get_script_hash();
        let resolver = Arc::new(
            SigningKeyResolver::new([SigningKeyBinding {
                account: account.to_string(),
                purpose: SignPurpose::OracleResponse,
                key_ref: resolved_key_ref,
                public_key: key_pair.public_key(),
            }])
            .expect("task 34 Oracle key resolver"),
        );
        let recording = Arc::new(RecordingOracleSigner {
            registered_key_ref,
            requests: Mutex::new(Vec::new()),
        });
        let context = oracle_context();
        let signer = TrustedSignerClient::new(
            recording.clone(),
            "software-default",
            TEST_POLICY_VERSION,
            Duration::from_secs(5),
            context.clone(),
            resolver,
        )
        .expect("task 34 trusted Oracle signer");
        service.set_signer_client(signer);
        OracleSigningHarness {
            service,
            recording,
            account,
            network,
            context,
        }
    }

    fn expected_request(
        network: u32,
        operation_id: &str,
        account: &crate::UInt160,
        key_ref: KeyRef,
        payload: Vec<u8>,
        fields: BTreeMap<String, String>,
    ) -> SignRequest {
        SignRequest {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: operation_id.to_string(),
            network,
            purpose: SignPurpose::OracleResponse,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: payload,
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: TEST_PROTOCOL.to_string(),
                protocol_version: "1".to_string(),
                fields,
            },
            policy_version: TEST_POLICY_VERSION.to_string(),
            deadline_unix_ms: TEST_DEADLINE_UNIX_MS,
            idempotency: IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: operation_id.to_string(),
                replay_nonce: operation_id.to_string(),
            },
            fee_units: 0,
            quota_units: 1,
            account: Some(account.to_string()),
            targets: BTreeSet::new(),
        }
    }

    fn recorded_request(recording: &RecordingOracleSigner) -> (SignRequest, AuthenticatedContext) {
        let requests = recording
            .requests
            .lock()
            .expect("task 34 Oracle recording lock");
        assert_eq!(requests.len(), 1, "signing must not retry with a fallback");
        requests[0].clone()
    }

    #[tokio::test]
    async fn task34_oracle_signing_builds_exact_non_default_request() {
        let configured = KeyRef::new("oracle-hsm-34", "oracle-key-34-v6", 6);
        let harness = signing_harness(configured.clone(), configured.clone(), 0x51);
        let operation_id = "task34-oracle-operation";
        let payload = vec![1, 3, 5, 7];
        let fields = BTreeMap::from([("request_id".to_string(), "34".to_string())]);

        let signature = harness
            .service
            .sign_neo_payload(
                operation_id.to_string(),
                harness.account.clone(),
                payload.clone(),
                TEST_PROTOCOL,
                fields.clone(),
            )
            .await
            .expect("task 34 Oracle signature");

        assert_eq!(signature, vec![0x34; 64]);
        let (request, context) = recorded_request(&harness.recording);
        assert_eq!(
            request,
            expected_request(
                harness.network,
                operation_id,
                &harness.account,
                configured,
                payload,
                fields,
            )
        );
        assert_eq!(request.purpose, SignPurpose::OracleResponse);
        assert_eq!(
            context.scopes,
            BTreeSet::from([SignPurpose::OracleResponse.required_scope()])
        );
        assert_eq!(context, harness.context);
    }

    #[tokio::test]
    async fn task34_oracle_signing_unknown_provider_fails_closed_without_fallback() {
        let registered = KeyRef::new("oracle-hsm-34", "oracle-key-34-v6", 6);
        let unknown = KeyRef::new("missing-oracle-provider-34", "oracle-key-34-v6", 6);
        let fallback = KeyRef::new("software-default", "default-key", 1);
        let harness = signing_harness(unknown.clone(), registered, 0x52);
        let operation_id = "task34-oracle-unknown-provider";
        let payload = vec![2, 4, 6, 8];
        let fields = BTreeMap::from([("request_id".to_string(), "35".to_string())]);

        let error = harness
            .service
            .sign_neo_payload(
                operation_id.to_string(),
                harness.account.clone(),
                payload.clone(),
                TEST_PROTOCOL,
                fields.clone(),
            )
            .await
            .expect_err("unknown Oracle provider must fail closed");

        assert!(matches!(&error, OracleServiceError::Processing(_)));
        assert!(error.to_string().contains("not registered"));
        let (request, context) = recorded_request(&harness.recording);
        assert_eq!(
            request,
            expected_request(
                harness.network,
                operation_id,
                &harness.account,
                unknown,
                payload,
                fields,
            )
        );
        assert_ne!(request.key_ref, fallback);
        assert_eq!(request.purpose, SignPurpose::OracleResponse);
        assert_eq!(context, harness.context);
    }

    #[tokio::test]
    async fn task34_oracle_signing_version_mismatch_fails_closed_without_fallback() {
        let registered = KeyRef::new("oracle-hsm-34", "oracle-key-34-v6", 6);
        let mismatched = KeyRef::new("oracle-hsm-34", "oracle-key-34-v6", 7);
        let fallback = KeyRef::new("software-default", "default-key", 1);
        let harness = signing_harness(mismatched.clone(), registered, 0x53);
        let operation_id = "task34-oracle-version-mismatch";
        let payload = vec![9, 7, 5, 3];
        let fields = BTreeMap::from([("request_id".to_string(), "36".to_string())]);

        let error = harness
            .service
            .sign_neo_payload(
                operation_id.to_string(),
                harness.account.clone(),
                payload.clone(),
                TEST_PROTOCOL,
                fields.clone(),
            )
            .await
            .expect_err("Oracle key version mismatch must fail closed");

        assert!(matches!(&error, OracleServiceError::Processing(_)));
        assert!(error.to_string().contains("not registered"));
        let (request, context) = recorded_request(&harness.recording);
        assert_eq!(
            request,
            expected_request(
                harness.network,
                operation_id,
                &harness.account,
                mismatched,
                payload,
                fields,
            )
        );
        assert_ne!(request.key_ref, fallback);
        assert_eq!(request.purpose, SignPurpose::OracleResponse);
        assert_eq!(context, harness.context);
    }
}
