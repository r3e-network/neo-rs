use super::*;
use crate::i_event_handlers::{CommittingHandler, WalletChangedHandler};
use crate::neo_system::NeoSystemContext;
use crate::neo_system::converters::{convert_ledger_block, convert_ledger_header};
use crate::network::p2p::ChannelsConfig;
use neo_native_contracts::ledger_contract::LedgerContract;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::execution::Block as LedgerBlock;
use neo_protocol::execution::BlockHeader as LedgerBlockHeader;
use neo_protocol::witness::Witness as PayloadWitness;
use neo_protocol::witness::Witness;
use neo_smart_contract::contract::Contract;
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::store::Store;
use neo_wallets::WalletProvider;
use neo_wallets::key_pair::KeyPair;
use neo_wallets::{Version, Wallet, WalletAccount, WalletError, WalletResult};
use std::any::Any;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use tokio::time::{Duration, sleep, timeout};

fn test_chain(length: u32) -> Vec<LedgerBlock> {
    let mut previous_hash = UInt256::zero();
    (0..length)
        .map(|index| {
            let header = LedgerBlockHeader::new(
                0,
                previous_hash,
                UInt256::zero(),
                u64::from(index),
                u64::from(index),
                index,
                0,
                UInt160::zero(),
                vec![Witness::new()],
            );
            let block = LedgerBlock::new(header, Vec::new());
            previous_hash = block.hash();
            block
        })
        .collect()
}

#[derive(Clone)]
struct SharedMemoryStoreProvider {
    store: Arc<MemoryStore>,
    requested_paths: Arc<Mutex<Vec<String>>>,
}

impl StoreProvider for SharedMemoryStoreProvider {
    fn name(&self) -> &str {
        "SharedMemoryProbe"
    }

    fn get_store(&self, path: &str) -> neo_storage::error::StorageResult<Arc<dyn Store>> {
        self.requested_paths.lock().push(path.to_string());
        Ok(self.store.clone())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn neo_system_persists_genesis_through_the_supplied_provider() {
    let shared_store = Arc::new(MemoryStore::new());
    let requested_paths = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(SharedMemoryStoreProvider {
        store: Arc::clone(&shared_store),
        requested_paths: Arc::clone(&requested_paths),
    });
    let system = NeoSystem::new(
        ProtocolSettings::default(),
        Some(provider),
        Some("t11-explicit-provider-path".to_string()),
    )
    .expect("system to start with the supplied provider");

    assert_eq!(
        requested_paths.lock().as_slice(),
        ["t11-explicit-provider-path"]
    );
    let cache = StoreCache::new_from_snapshot(shared_store.snapshot());
    assert!(LedgerContract::new().contains_block(&cache, &system.genesis_block().hash()));
    assert_eq!(system.current_block_index(), 0);

    system.shutdown().await.expect("shutdown succeeds");
}

#[derive(Debug)]
struct DummyService;

#[test]
fn typed_service_registry_registers_and_fetches_by_type() {
    let registry = ServiceRegistry::new();
    let service = Arc::new(DummyService);

    registry
        .register_typed(service.clone())
        .expect("typed registration");

    let fetched = registry
        .get_typed::<DummyService>()
        .expect("lookup should succeed")
        .expect("service should be present");

    assert!(Arc::ptr_eq(&service, &fetched));

    // Typed lookup should also surface through the get_service helper.
    let fetched_via_get = registry
        .get_service::<DummyService>()
        .expect("fallback lookup")
        .expect("service should be present");
    assert!(Arc::ptr_eq(&service, &fetched_via_get));
}

#[test]
fn hydrate_ledger_from_empty_store_is_noop() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(store, true);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::new());

    NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache)
        .expect("empty store is valid");

    assert_eq!(ledger.current_height(), 0);
    assert_eq!(header_cache.count(), 0);
}

#[test]
fn hydrate_ledger_restores_height_and_headers() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let mut store_cache = StoreCache::new_from_store(store, false);
    let blocks = test_chain(2);
    let ledger_contract = LedgerContract::new();
    for block in &blocks {
        ledger_contract
            .store_block_state(store_cache.data_cache(), block, &[])
            .expect("store block through the canonical writer");
    }
    let tip = &blocks[1];
    ledger_contract
        .set_current_block_state(store_cache.data_cache(), &tip.hash(), tip.index())
        .expect("store current block pointer");
    store_cache.commit();

    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::new());
    NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache)
        .expect("hydrate complete chain");

    assert_eq!(ledger.current_height(), 1);
    assert_eq!(header_cache.count(), 2);
    assert_eq!(ledger.block_hash_at(1), Some(tip.hash()));
    assert_eq!(ledger.block_hash_at(0), Some(blocks[0].hash()));
}

fn sample_u256(byte: u8) -> UInt256 {
    UInt256::from_bytes(&[byte; 32]).expect("uint256 from bytes")
}

fn sample_u160(byte: u8) -> UInt160 {
    UInt160::from_bytes(&[byte; 20]).expect("uint160 from bytes")
}

fn sample_ledger_header() -> LedgerBlockHeader {
    let witness = neo_protocol::witness::Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
    LedgerBlockHeader::new(
        1,
        sample_u256(1),
        sample_u256(2),
        42,
        7,
        10,
        3,
        sample_u160(5),
        vec![witness],
    )
}

#[test]
fn convert_ledger_header_preserves_fields() {
    let header = sample_ledger_header();
    let converted = convert_ledger_header(header.clone());

    assert_eq!(converted.version(), 1);
    assert_eq!(converted.prev_hash(), &sample_u256(1));
    assert_eq!(converted.merkle_root(), &sample_u256(2));
    assert_eq!(converted.timestamp(), 42);
    assert_eq!(converted.nonce(), 7);
    assert_eq!(converted.index(), 10);
    assert_eq!(converted.primary_index(), 3);
    assert_eq!(converted.next_consensus(), &sample_u160(5));

    let expected_witness = PayloadWitness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
    assert_eq!(
        converted.witness.invocation_script,
        expected_witness.invocation_script
    );
    assert_eq!(
        converted.witness.verification_script,
        expected_witness.verification_script
    );

    assert_eq!(header.witnesses.len(), 1);
}

#[test]
fn convert_ledger_block_transfers_transactions() {
    let header = sample_ledger_header();
    let txs = Vec::new();
    let ledger_block = LedgerBlock {
        header,
        transactions: txs.clone(),
    };
    let block = convert_ledger_block(ledger_block);
    assert_eq!(block.transactions.len(), txs.len());
    assert_eq!(block.header.index(), 10);
}

#[derive(Default)]
struct EventProbe {
    wallet_changes: AtomicUsize,
}

impl EventProbe {
    fn wallet_changes(&self) -> usize {
        self.wallet_changes.load(Ordering::Relaxed)
    }
}

impl WalletChangedHandler for EventProbe {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn Any,
        _wallet: Option<Arc<dyn Wallet>>,
    ) {
        let _ = _wallet;
        self.wallet_changes.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct DummyWallet {
    version: Version,
}

impl Wallet for DummyWallet {
    fn name(&self) -> &str {
        "dummy"
    }

    fn path(&self) -> Option<&str> {
        None
    }

    fn version(&self) -> &Version {
        &self.version
    }

    fn change_password(&self, _old_password: &str, _new_password: &str) -> WalletResult<bool> {
        Ok(false)
    }

    fn contains(&self, _script_hash: &UInt160) -> bool {
        false
    }

    fn create_account(&self, _private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn create_account_with_contract(
        &self,
        _contract: Contract,
        _key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
        Ok(false)
    }

    fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn get_account(&self, _script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        None
    }

    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        Vec::new()
    }

    fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn import_nep2(
        &self,
        _nep2_key: &str,
        _password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        None
    }

    fn set_default_account(&self, _script_hash: &UInt160) -> WalletResult<()> {
        Ok(())
    }

    fn sign(&self, _data: &[u8], _script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn unlock(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn lock(&self) {}

    fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn save(&self) -> WalletResult<()> {
        Ok(())
    }
}

struct TestWalletProvider {
    #[allow(clippy::type_complexity)]
    receiver: Mutex<Option<mpsc::Receiver<Option<Arc<dyn Wallet>>>>>,
}

impl TestWalletProvider {
    fn new() -> (Arc<Self>, mpsc::Sender<Option<Arc<dyn Wallet>>>) {
        let (tx, rx) = mpsc::channel();
        let provider = Arc::new(Self {
            receiver: Mutex::new(Some(rx)),
        });
        (provider, tx)
    }
}

impl WalletProvider for TestWalletProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn wallet_changed(&self) -> mpsc::Receiver<Option<Arc<dyn Wallet>>> {
        self.receiver
            .lock()
            .take()
            .expect("wallet changed receiver already taken")
    }

    fn get_wallet(&self) -> Option<Arc<dyn Wallet>> {
        None
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn local_node_defaults_match_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.seed_list.clear();
    let system = NeoSystem::new(settings, None, None).expect("system to start");

    let config = ChannelsConfig {
        min_desired_connections: 0,
        max_connections: 0,
        ..Default::default()
    };

    system.start_node(config).expect("start local node");
    sleep(Duration::from_millis(50)).await;

    let snapshot = system
        .local_node_state()
        .await
        .expect("local node snapshot");
    assert_eq!(snapshot.port(), 0);
    assert_eq!(snapshot.connected_peers_count(), 0);
    assert!(snapshot.remote_nodes().is_empty());
    assert!(system.peers().await.expect("peer query").is_empty());
    assert_eq!(system.unconnected_count().await.expect("unconnected"), 0);

    let _ = system.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn add_unconnected_peers_tracks_queue() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");

    let endpoints: Vec<SocketAddr> = vec![
        "127.0.0.1:20000".parse().unwrap(),
        "127.0.0.1:20001".parse().unwrap(),
    ];

    system
        .add_unconnected_peers(endpoints.clone())
        .expect("enqueue peers");

    let count = system.unconnected_count().await.expect("unconnected count");
    assert_eq!(count, endpoints.len());

    let mut returned = system.unconnected_peers().await.expect("unconnected peers");
    returned.sort();

    let mut expected = endpoints;
    expected.sort();
    assert_eq!(returned, expected);

    let _ = system.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_changed_handlers_receive_events() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let handler = Arc::new(EventProbe::default());
    system
        .register_wallet_changed_handler(handler.clone())
        .expect("register wallet handler");

    let (provider, tx) = TestWalletProvider::new();
    system
        .attach_wallet_provider(provider)
        .expect("attach wallet provider");

    tx.send(Some(Arc::new(DummyWallet::default()) as Arc<dyn Wallet>))
        .expect("send wallet");

    timeout(Duration::from_secs(1), async {
        while handler.wallet_changes() == 0 {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("wallet handler triggered");
}

#[tokio::test(flavor = "multi_thread")]
async fn local_node_actor_tracks_peers() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let endpoint: SocketAddr = "127.0.0.1:20333".parse().unwrap();

    system
        .add_peer(endpoint, Some(20333), 0, 0, 0)
        .expect("add peer should succeed");

    assert_eq!(system.peer_count().await.unwrap(), 1);
    let peers = system.peers().await.unwrap();
    assert_eq!(peers, vec![endpoint]);

    let snapshots = system.remote_node_snapshots().await.unwrap();
    assert_eq!(snapshots.len(), 1);

    assert!(system.remove_peer(endpoint).await.unwrap());
    assert_eq!(system.peer_count().await.unwrap(), 0);

    system.shutdown().await.expect("shutdown succeeds");
}

struct GenesisIndexCapture {
    seen: Arc<AtomicU32>,
}

impl CommittingHandler for GenesisIndexCapture {
    fn blockchain_committing_handler(
        &self,
        _system: &dyn Any,
        block: &neo_protocol::execution::Block,
        _snapshot: &neo_storage::persistence::DataCache,
        _executed: &[neo_protocol::execution::ApplicationExecuted],
    ) {
        self.seen.store(block.index(), Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn deferred_genesis_persist_notifies_handlers_registered_after_new() {
    let system = NeoSystem::new_with_state_service_deferring_genesis(
        ProtocolSettings::default(),
        None,
        None,
        None,
    )
    .expect("system to start");
    let seen = Arc::new(AtomicU32::new(u32::MAX));
    system
        .register_committing_handler(Arc::new(GenesisIndexCapture {
            seen: Arc::clone(&seen),
        }))
        .expect("register handler");

    system.persist_genesis_if_absent().expect("persist genesis");

    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(system.current_block_index(), 0);
    system.shutdown().await.expect("shutdown succeeds");
}
