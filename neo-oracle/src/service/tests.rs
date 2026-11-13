//! Unit tests for the Oracle service runtime.
//!
//! The reference implementation's `service/tests/{dedup,response_tx,mod}.rs`
//! are mirrored here as far as the port abstraction allows:
//!
//! - `dedup.rs` is migrated verbatim (behaviour is identical).
//! - `filter_json.rs` lives with its subject in [`crate::filter`].
//! - `mod.rs` exercised `neo_signing::TrustedSignerClient` key-binding
//!   resolution; that crate is deliberately *not* a dependency here, so the
//!   equivalent guarantees (fail closed without a signer, delegate exactly
//!   once with the canonical intent, never fall back) are asserted against the
//!   [`OracleSigner`](crate::ports::OracleSigner) port instead.
//! - `response_tx.rs` covered the C# fee math inside `create_response_tx`,
//!   which is owned by the host's
//!   [`ResponseTxBuilder`](crate::ports::ResponseTxBuilder) implementation.

use super::{OracleService, OracleServiceSettings};
use crate::dedup::{DEDUP_CACHE_TTL, FINISHED_CACHE_TTL};
use crate::error::OracleServiceError;
use crate::ports::{
    OracleSigner, OracleSnapshot, OracleWallet, ResponseTxBuilder, SignatureVerifier,
    SnapshotFactory, TxRelay,
};
use crate::request::OracleRequest;

use async_trait::async_trait;
use neo_crypto::{Crypto, ECCurve, ECPoint, Secp256r1Crypto};
use neo_primitives::{OracleResponseCode, UInt160, UInt256};
use neo_protocol::payloads::Transaction;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const TEST_NETWORK: u32 = 0x74_6e_41;

/// Deterministic secp256r1 point derived from a single-byte private key.
fn sample_point(byte: u8) -> ECPoint {
    let mut private_key = [0u8; 32];
    private_key[31] = byte.max(1);
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive test key");
    ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
        .expect("static test key")
}

fn sample_request(id: u64, url: &str) -> OracleRequest {
    OracleRequest::new(
        UInt256::from([id as u8; 32]),
        100_000_000,
        url.to_string(),
        None,
        UInt160::default(),
        "callback".to_string(),
        Vec::new(),
    )
}

/// Configurable in-memory snapshot standing in for the ledger.
#[derive(Clone, Default)]
struct TestSnapshot {
    height: u32,
    oracles: Vec<ECPoint>,
    requests: BTreeMap<u64, OracleRequest>,
    tx_block_index: Option<u32>,
}

impl OracleSnapshot for TestSnapshot {
    fn ledger_height(&self) -> u32 {
        self.height
    }

    fn designated_oracles(&self, _height: u32) -> Result<Vec<ECPoint>, OracleServiceError> {
        Ok(self.oracles.clone())
    }

    fn get_request(&self, request_id: u64) -> Result<Option<OracleRequest>, OracleServiceError> {
        Ok(self.requests.get(&request_id).cloned())
    }

    fn get_requests(&self) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError> {
        Ok(self.requests.iter().map(|(k, v)| (*k, v.clone())).collect())
    }

    fn get_requests_by_url(
        &self,
        url: &str,
    ) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError> {
        Ok(self
            .requests
            .iter()
            .filter(|(_, request)| request.url == url)
            .map(|(k, v)| (*k, v.clone()))
            .collect())
    }

    fn transaction_block_index(
        &self,
        _tx_hash: &UInt256,
    ) -> Result<Option<u32>, OracleServiceError> {
        Ok(self.tx_block_index)
    }

    fn max_valid_until_block_increment(&self) -> u32 {
        24
    }

    fn exec_fee_factor(&self) -> u32 {
        30
    }

    fn fee_per_byte(&self) -> i64 {
        1000
    }
}

#[derive(Clone, Default)]
struct TestSnapshots {
    snapshot: TestSnapshot,
}

impl SnapshotFactory for TestSnapshots {
    type Snapshot = TestSnapshot;

    fn snapshot(&self) -> TestSnapshot {
        self.snapshot.clone()
    }
}

#[derive(Default)]
struct TestWallet {
    accounts: Vec<(UInt160, ECPoint)>,
}

impl OracleWallet for TestWallet {
    fn oracle_accounts(&self, designated_oracles: &[ECPoint]) -> Vec<(UInt160, ECPoint)> {
        self.accounts
            .iter()
            .filter(|&(_, key)| designated_oracles.contains(key))
            .cloned()
            .collect()
    }

    fn oracle_key(&self, _account: &UInt160) -> Option<neo_wallets::KeyPair> {
        None
    }
}

struct TestTxBuilder;

impl ResponseTxBuilder for TestTxBuilder {
    fn build_response_tx(
        &self,
        _snapshot: &dyn OracleSnapshot,
        _request: &OracleRequest,
        _response_id: u64,
        response_code: OracleResponseCode,
        _response_result: Vec<u8>,
        _oracle_nodes: &[ECPoint],
        _network: u32,
        _use_current_height: bool,
    ) -> Result<(Transaction, OracleResponseCode), OracleServiceError> {
        Ok((Transaction::new(), response_code))
    }
}

#[derive(Default)]
struct TestRelay {
    relayed: Mutex<Vec<Transaction>>,
}

impl TxRelay for TestRelay {
    fn relay_transaction(&self, tx: Transaction) -> Result<(), OracleServiceError> {
        self.relayed.lock().expect("relay lock").push(tx);
        Ok(())
    }
}

struct TestVerifier;

impl SignatureVerifier for TestVerifier {
    fn verify(&self, public_key: &ECPoint, message: &[u8], signature: &[u8]) -> bool {
        public_key
            .verify_signature(message, signature)
            .unwrap_or(false)
    }
}

/// Records every signing intent and replays a fixed signature.
struct RecordingSigner {
    calls: Mutex<Vec<RecordedCall>>,
    signature: Vec<u8>,
    call_count: AtomicUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedCall {
    operation_id: String,
    account: UInt160,
    payload: Vec<u8>,
    protocol: String,
    fields: BTreeMap<String, String>,
}

impl RecordingSigner {
    fn new(signature: Vec<u8>) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            signature,
            call_count: AtomicUsize::new(0),
        })
    }

    fn recorded(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("signer lock").clone()
    }
}

#[async_trait]
impl OracleSigner for RecordingSigner {
    async fn sign_payload(
        &self,
        operation_id: String,
        account: UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<Vec<u8>, OracleServiceError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().expect("signer lock").push(RecordedCall {
            operation_id,
            account,
            payload,
            protocol: protocol.to_string(),
            fields,
        });
        Ok(self.signature.clone())
    }
}

type TestService = OracleService<
    TestSnapshots,
    TestWallet,
    RecordingSigner,
    TestTxBuilder,
    TestRelay,
    TestVerifier,
>;

fn service(
    settings: OracleServiceSettings,
    snapshot: TestSnapshot,
    relay: Arc<TestRelay>,
) -> TestService {
    OracleService::new(
        settings,
        Arc::new(TestSnapshots { snapshot }),
        Arc::new(TestTxBuilder),
        relay,
        Arc::new(TestVerifier),
    )
    .expect("oracle service")
}

fn test_settings(enable_deduplication: bool) -> OracleServiceSettings {
    OracleServiceSettings {
        network: TEST_NETWORK,
        nodes: Vec::new(),
        enable_deduplication,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Deduplication (migrated from the reference `service/tests/dedup.rs`)
// ---------------------------------------------------------------------------

#[test]
fn dedup_cache_tracks_in_flight_and_recent_urls() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let url = "https://oracle.example/request";

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.in_flight_count(), 1);
    assert_eq!(service.dedup_cache_size(), 0);

    assert!(service.is_duplicate_request(2, url));

    service.mark_request_completed(1, url);
    assert_eq!(service.in_flight_count(), 0);
    assert_eq!(service.dedup_cache_size(), 1);
    assert!(service.is_duplicate_request(3, url));
}

#[test]
fn dedup_cache_prunes_expired_urls_before_checking_recent_duplicates() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let url = "https://oracle.example/expired";
    let expired = SystemTime::now() - DEDUP_CACHE_TTL - Duration::from_secs(1);
    service
        .dedup
        .lock()
        .completed
        .insert_at(url.to_string(), expired);

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.dedup_cache_size(), 0);
    assert_eq!(service.in_flight_count(), 1);
}

#[test]
fn dedup_cache_retains_future_urls_without_marking_them_recent() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let url = "https://oracle.example/future";
    let future = SystemTime::now() + Duration::from_secs(60);
    service
        .dedup
        .lock()
        .completed
        .insert_at(url.to_string(), future);

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.dedup_cache_size(), 1);
    assert_eq!(service.in_flight_count(), 1);
}

#[test]
fn disabled_deduplication_does_not_track_urls() {
    let service = service(
        test_settings(false),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let url = "https://oracle.example/request";

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.in_flight_count(), 0);
    assert_eq!(service.dedup_cache_size(), 0);
}

#[test]
fn finished_cache_entries_clear_on_timer_sweep() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let request_id = 42;

    service
        .finished_cache
        .lock()
        .insert_at(request_id, SystemTime::UNIX_EPOCH);
    assert!(service.is_request_finished(request_id));

    service.cleanup_finished_cache(SystemTime::UNIX_EPOCH + FINISHED_CACHE_TTL);
    assert!(service.is_request_finished(request_id));

    service.cleanup_finished_cache(
        SystemTime::UNIX_EPOCH + FINISHED_CACHE_TTL + Duration::from_secs(1),
    );
    assert!(!service.is_request_finished(request_id));
}

// ---------------------------------------------------------------------------
// Signing through the OracleSigner port
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signing_fails_closed_without_an_installed_signer() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );

    let error = service
        .sign_neo_payload(
            "oracle-operation".to_string(),
            UInt160::default(),
            vec![1, 3, 5, 7],
            "neo-n3-oracle-response-submission",
            BTreeMap::from([("request_id".to_string(), "7".to_string())]),
        )
        .await
        .expect_err("signing must fail closed without a signer");

    assert!(matches!(&error, OracleServiceError::Processing(_)));
    assert!(error.to_string().contains("trusted signer service"));
}

#[tokio::test]
async fn signing_delegates_the_canonical_intent_exactly_once() {
    let signer = RecordingSigner::new(vec![0x4F; 64]);
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    service.set_signer(Some(signer.clone()));
    let account = UInt160::from([0x11; 20]);

    let signature = service
        .sign_neo_payload(
            "oracle-operation".to_string(),
            account,
            vec![1, 3, 5, 7],
            "neo-n3-oracle-response-submission",
            BTreeMap::from([("request_id".to_string(), "7".to_string())]),
        )
        .await
        .expect("oracle signature");

    assert_eq!(signature, vec![0x4F; 64]);
    // No retry, no fallback key: exactly one intent reaches the signer.
    assert_eq!(signer.call_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        signer.recorded(),
        vec![RecordedCall {
            operation_id: "oracle-operation".to_string(),
            account,
            payload: vec![1, 3, 5, 7],
            protocol: "neo-n3-oracle-response-submission".to_string(),
            fields: BTreeMap::from([("request_id".to_string(), "7".to_string())]),
        }]
    );
}

#[tokio::test]
async fn send_response_signature_signs_the_canonical_submission_message() {
    let signer = RecordingSigner::new(vec![0x2A; 64]);
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    service.set_signer(Some(signer.clone()));

    let request_id: u64 = 0x0102_0304_0506_0708;
    let account = UInt160::from([0x33; 20]);
    let public_key = sample_point(0x07);
    let tx_sign = vec![0xAA; 64];

    let handle = service
        .send_response_signature(request_id, tx_sign.clone(), account, public_key.clone())
        .await
        .expect("submission task");
    handle.await.expect("submission broadcast");

    let mut message = Vec::new();
    message.extend_from_slice(public_key.as_bytes());
    message.extend_from_slice(&request_id.to_le_bytes());
    message.extend_from_slice(&tx_sign);
    let expected_operation_id = format!(
        "oracle-submission:{}:{}:{}:{}",
        TEST_NETWORK,
        request_id,
        account,
        hex::encode(Crypto::sha256(&message)),
    );

    assert_eq!(
        signer.recorded(),
        vec![RecordedCall {
            operation_id: expected_operation_id,
            account,
            payload: message,
            protocol: "neo-n3-oracle-response-submission".to_string(),
            fields: BTreeMap::from([
                ("request_id".to_string(), request_id.to_string()),
                (
                    "oracle_public_key".to_string(),
                    hex::encode(public_key.as_bytes())
                ),
            ]),
        }]
    );
}

// ---------------------------------------------------------------------------
// Signature verification and pending-queue guards
// ---------------------------------------------------------------------------

#[test]
fn verify_oracle_signature_rejects_wrong_length_signatures() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let public_key = sample_point(0x05);

    assert!(!service.verify_oracle_signature(&public_key, b"message", &[0u8; 63]));
    assert!(!service.verify_oracle_signature(&public_key, b"message", &[0u8; 65]));
}

fn queue_service(request: Option<OracleRequest>, tx_block_index: Option<u32>) -> TestService {
    let mut requests = BTreeMap::new();
    if let Some(request) = request.clone() {
        requests.insert(7, request);
    }
    service(
        test_settings(true),
        TestSnapshot {
            height: 10,
            oracles: vec![sample_point(0x01)],
            requests,
            tx_block_index,
        },
        Arc::new(TestRelay::default()),
    )
}

#[test]
fn add_response_tx_sign_rejects_unknown_request() {
    let service = queue_service(None, Some(1));

    let error = service
        .add_response_tx_sign(
            &service.snapshots.snapshot(),
            7,
            sample_point(0x01),
            vec![0u8; 64],
            None,
            None,
            None,
        )
        .expect_err("unknown request must be rejected");

    assert!(matches!(error, OracleServiceError::RequestNotFound));
}

#[test]
fn add_response_tx_sign_rejects_missing_request_transaction() {
    let service = queue_service(Some(sample_request(7, "https://oracle.example/ok")), None);

    let error = service
        .add_response_tx_sign(
            &service.snapshots.snapshot(),
            7,
            sample_point(0x01),
            vec![0u8; 64],
            None,
            None,
            None,
        )
        .expect_err("missing transaction state must be rejected");

    assert!(matches!(
        error,
        OracleServiceError::RequestTransactionNotFound
    ));
}

#[test]
fn add_response_tx_sign_rejects_blocked_url() {
    let mut settings = test_settings(true);
    settings.url_blacklist = vec!["oracle.example".to_string()];
    let mut requests = BTreeMap::new();
    requests.insert(7, sample_request(7, "https://oracle.example/blocked"));
    let service = service(
        settings,
        TestSnapshot {
            height: 10,
            oracles: vec![sample_point(0x01)],
            requests,
            tx_block_index: Some(1),
        },
        Arc::new(TestRelay::default()),
    );

    let error = service
        .add_response_tx_sign(
            &service.snapshots.snapshot(),
            7,
            sample_point(0x01),
            vec![0u8; 64],
            None,
            None,
            None,
        )
        .expect_err("blacklisted URL must be rejected");

    assert!(matches!(error, OracleServiceError::UrlBlocked));
}

#[test]
fn add_response_tx_sign_rejects_when_pending_queue_is_full() {
    let service = queue_service(
        Some(sample_request(7, "https://oracle.example/ok")),
        Some(1),
    );
    // Id outside the pre-filled `0..MAX_PENDING_QUEUE_SIZE` range.
    let overflow_id = super::MAX_PENDING_QUEUE_SIZE as u64;
    {
        let mut queue = service.pending_queue.lock();
        for id in 0..super::MAX_PENDING_QUEUE_SIZE {
            queue.insert(
                id as u64,
                super::OracleTask {
                    tx: None,
                    backup_tx: None,
                    signs: BTreeMap::new(),
                    backup_signs: BTreeMap::new(),
                    timestamp: SystemTime::now(),
                },
            );
        }
    }

    let error = service
        .add_response_tx_sign(
            &service.snapshots.snapshot(),
            overflow_id,
            sample_point(0x01),
            vec![0u8; 64],
            None,
            None,
            None,
        )
        .expect_err("full queue must be rejected");

    assert!(matches!(error, OracleServiceError::Processing(_)));
    assert!(error.to_string().contains("Pending queue is full"));

    // An already-queued request keeps being aggregated even at capacity.
    service
        .add_response_tx_sign(
            &service.snapshots.snapshot(),
            7,
            sample_point(0x01),
            vec![0u8; 64],
            None,
            None,
            None,
        )
        .expect("queued request must still be accepted at capacity");
}

#[test]
fn submit_oracle_response_requires_a_running_service() {
    let service = service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );

    let error = service
        .submit_oracle_response(sample_point(0x01), 7, vec![0u8; 64], vec![0u8; 64])
        .expect_err("a stopped service must reject submissions");

    assert!(matches!(error, OracleServiceError::Disabled));
}

#[tokio::test]
async fn process_url_rejects_unsupported_schemes_and_dispatches_neofs() {
    let service = self::service(
        test_settings(true),
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );

    let (code, body) = service.process_url("ftp://example.com/data").await;
    assert_eq!(code, OracleResponseCode::ProtocolNotSupported);
    assert_eq!(body, "Invalid Protocol:<ftp://example.com/data>");

    // Plain HTTP is not an oracle protocol (HTTPS only).
    let (code, body) = service.process_url("http://example.com/data").await;
    assert_eq!(code, OracleResponseCode::ProtocolNotSupported);
    assert_eq!(body, "Invalid Protocol:<http://example.com/data>");

    // NeoFS is dispatched to its protocol parser rather than rejected as an
    // unsupported scheme. This malformed object identifier fails closed.
    let (code, body) = service.process_url("neofs://container/object").await;
    assert_eq!(code, OracleResponseCode::Error);
    assert!(body.is_empty());

    let mut signing_settings = test_settings(true);
    signing_settings.neofs_auto_sign_bearer = true;
    signing_settings.neofs_bearer_token = Some("dG9rZW4=".to_string());
    let signing_service = self::service(
        signing_settings,
        TestSnapshot::default(),
        Arc::new(TestRelay::default()),
    );
    let (code, body) = signing_service
        .process_url("neofs://container/object")
        .await;
    assert_eq!(code, OracleResponseCode::Error);
    assert!(body.is_empty());

    let (code, body) = service.process_url("").await;
    assert_eq!(code, OracleResponseCode::Error);
    assert_eq!(body, "Empty URL");

    let long = format!("https://example.com/{}", "a".repeat(2048));
    let (code, _) = service.process_url(&long).await;
    assert_eq!(code, OracleResponseCode::Error);
}
