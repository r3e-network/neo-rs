//! Oracle service shell bound to the host node (parity with
//! Neo.Plugins.OracleService).
//!
//! The generic engine — request/timer loops, URL dispatch, dedup, signature
//! aggregation, HTTPS/NeoFS transports — lives in the `neo-oracle` crate.
//! This module composes it with the neo-core adapters in [`super::adapters`]
//! and preserves the historical `OracleService::new(settings,
//! Arc<NeoSystem>)` construction API for consumers (neo-node startup,
//! neo-rpc).

#[cfg(test)]
mod tests;

pub(super) use super::OracleServiceSettings;
use super::adapters::{
    CoreOracleSigner, CoreOracleWallet, CoreResponseTxBuilder, CoreSignatureVerifier,
    CoreSnapshotFactory, CoreTxRelay,
};
use super::signing_stubs::TrustedSignerClient;
use crate::neo_system::NeoSystem;
use neo_crypto::ECPoint;
use neo_wallets::Wallet;
use neo_oracle::service::OracleService as EngineService;
pub use neo_oracle::{OracleServiceError, OracleStatus};
#[cfg(test)]
use std::collections::BTreeMap;
use std::sync::Arc;

/// Oracle service runtime bound to a [`NeoSystem`] host node.
///
/// A thin shell over the generic engine: every method forwards to
/// [`EngineService`], with the host-node adapters supplying the ledger
/// snapshot, wallet account selection, trusted signing, response transaction
/// building, relay and signature verification.
pub struct OracleService {
    inner: Arc<
        EngineService<
            CoreSnapshotFactory,
            CoreOracleWallet,
            CoreOracleSigner,
            CoreResponseTxBuilder,
            CoreTxRelay,
            CoreSignatureVerifier,
        >,
    >,
}

impl OracleService {
    /// Creates the oracle service, wiring the neo-oracle engine to the host
    /// node's ledger, native contracts and transaction router.
    pub fn new(
        settings: OracleServiceSettings,
        system: Arc<NeoSystem>,
    ) -> Result<Self, OracleServiceError> {
        let snapshots = Arc::new(CoreSnapshotFactory::new(system.clone()));
        let tx_builder = Arc::new(CoreResponseTxBuilder::new(system.clone()));
        let relay = Arc::new(CoreTxRelay::new(system));
        let verifier = Arc::new(CoreSignatureVerifier);
        let inner = Arc::new(EngineService::new(
            settings, snapshots, tx_builder, relay, verifier,
        )?);
        Ok(Self { inner })
    }

    /// Wires the service's weak self-reference (call once after `Arc`-ing).
    pub fn set_self_ref(self: &Arc<Self>) {
        self.inner.set_self_ref();
    }

    /// Installs the immutable, startup-authenticated signer client.
    pub fn set_signer_client(&self, signer: TrustedSignerClient) {
        let network = self.inner.settings().network;
        self.inner
            .set_signer(Some(Arc::new(CoreOracleSigner::new(network, signer))));
    }

    /// Returns the normalized settings in effect.
    pub fn settings(&self) -> &OracleServiceSettings {
        self.inner.settings()
    }

    /// Returns the current lifecycle status.
    pub fn status(&self) -> OracleStatus {
        self.inner.status()
    }

    /// Returns true while the service is running.
    pub fn is_running(&self) -> bool {
        self.inner.is_running()
    }

    /// Starts the service with the given oracle wallet: resolves the oracle
    /// designated set and spawns the request and timer tasks.
    pub fn start(self: &Arc<Self>, wallet: Arc<dyn Wallet>) {
        self.inner.start(Arc::new(CoreOracleWallet::new(wallet)));
    }

    /// Stops the service, cancelling pending requests and timers.
    pub fn stop(&self) {
        self.inner.stop();
    }

    /// Signs `payload` with the installed trusted signer (fails closed).
    ///
    /// Only exercised by the signing tests below: production signing flows
    /// through the engine, which calls the signer port directly.
    #[cfg(test)]
    async fn sign_neo_payload(
        &self,
        operation_id: String,
        account: neo_primitives::UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<Vec<u8>, OracleServiceError> {
        self.inner
            .sign_neo_payload(operation_id, account, payload, protocol, fields)
            .await
    }

    /// Records a validator's response signatures for `request_id` and
    /// submits the response transaction once enough signatures accumulate.
    pub fn submit_oracle_response(
        &self,
        oracle_pub: ECPoint,
        request_id: u64,
        tx_sign: Vec<u8>,
        msg_sign: Vec<u8>,
    ) -> Result<(), OracleServiceError> {
        self.inner
            .submit_oracle_response(oracle_pub, request_id, tx_sign, msg_sign)
    }

    /// Checks if a request is a duplicate and should be skipped.
    /// Returns true if the request is a duplicate.
    pub fn is_duplicate_request(&self, request_id: u64, url: &str) -> bool {
        self.inner.is_duplicate_request(request_id, url)
    }

    /// Marks a request as completed and removes it from in-flight.
    pub fn mark_request_completed(&self, request_id: u64, url: &str) {
        self.inner.mark_request_completed(request_id, url)
    }

    /// Cleans up in-flight requests (call on error/timeout).
    pub fn cleanup_in_flight(&self, url: &str) {
        self.inner.cleanup_in_flight(url)
    }

    /// Validates a URL against security policies.
    pub fn validate_url(&self, url: &str) -> Result<(), OracleServiceError> {
        self.inner.validate_url(url)
    }

    /// Gets the current deduplication cache size (for monitoring).
    pub fn dedup_cache_size(&self) -> usize {
        self.inner.dedup_cache_size()
    }

    /// Gets the current in-flight request count (for monitoring).
    pub fn in_flight_count(&self) -> usize {
        self.inner.in_flight_count()
    }

    /// Gets the current pending queue size (for monitoring).
    pub fn pending_queue_size(&self) -> usize {
        self.inner.pending_queue_size()
    }

    /// Checks if the pending queue is at capacity.
    pub fn is_queue_full(&self) -> bool {
        self.inner.is_queue_full()
    }
}

impl crate::events::handlers::CommittingHandler for OracleService {
    fn blockchain_committing_handler(
        &self,
        system: &dyn std::any::Any,
        _block: &neo_protocol::execution::Block,
        _snapshot: &neo_storage::persistence::DataCache,
        _application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) {
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        // The engine takes a fresh store snapshot; the committing snapshot of
        // this hook covers the same committed state at this point.
        self.inner.handle_committing(system.settings().network);
    }
}

impl crate::i_event_handlers::WalletChangedHandler for OracleService {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn Wallet>>,
    ) {
        self.inner
            .handle_wallet_changed(wallet.map(|wallet| Arc::new(CoreOracleWallet::new(wallet))));
    }
}

#[cfg(test)]
mod task34_signing_tests {
    use super::{OracleService, OracleServiceError, OracleServiceSettings};
    use async_trait::async_trait;
    use neo_wallets::KeyPair;
    use neo_protocol::protocol_settings::ProtocolSettings;
    use neo_signing::{
        AuthenticatedContext, FrozenSignIntent, IdempotencyInfo, KeyRef, ProtocolContext,
        SIGNING_SCHEMA_VERSION, SignError, SignErrorCode, SignPayload, SignPurpose, SignRequest,
        SignResponse, SignResult, SignerService, SigningKeyBinding, SigningKeyResolver,
        TrustedSignerClient,
    };
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
                algorithm: neo_signing::ALGORITHM_ECDSA_P256_SHA256.to_string(),
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
        account: neo_primitives::UInt160,
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
        account: &neo_primitives::UInt160,
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
