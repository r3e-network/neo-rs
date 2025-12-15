use super::*;
use crate::i_event_handlers::{
    ILogHandler, ILoggingHandler, INotifyHandler, ITransactionAddedHandler,
    ITransactionRemovedHandler, IWalletChangedHandler,
};
use crate::ledger::Block as LedgerBlock;
use crate::ledger::{
    block_header::BlockHeader as LedgerBlockHeader,
    transaction_removal_reason::TransactionRemovalReason,
    transaction_removed_event_args::TransactionRemovedEventArgs, Block,
};
use crate::neo_io::Serializable;
use crate::neo_system::converters::{convert_ledger_block, convert_ledger_header};
use crate::neo_system::relay::LEDGER_HYDRATION_WINDOW;
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::payloads::witness::Witness as PayloadWitness;
use crate::network::p2p::payloads::Transaction;
use crate::network::p2p::ChannelsConfig;
use crate::persistence::data_cache::DataCache;
use crate::persistence::i_store::IStore;
use crate::persistence::providers::memory_store::MemoryStore;
use crate::persistence::StoreCache;
use crate::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use crate::smart_contract::contract::Contract;
use crate::smart_contract::log_event_args::LogEventArgs;
use crate::smart_contract::native::trimmed_block::TrimmedBlock;
use crate::smart_contract::notify_event_args::NotifyEventArgs;
use crate::smart_contract::trigger_type::TriggerType;
use crate::wallets::key_pair::KeyPair;
use crate::wallets::IWalletProvider;
use crate::wallets::{Version, Wallet, WalletAccount, WalletError, WalletResult};
use crate::Witness;
use crate::{IVerifiable, UInt160, UInt256};
use async_trait::async_trait;
use lazy_static::lazy_static;
use neo_vm::StackItem;
use parking_lot::Mutex;
use std::any::Any;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use tokio::time::{sleep, timeout, Duration};

lazy_static! {
    static ref LOG_TEST_MUTEX: Mutex<()> = Mutex::new(());
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
    let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(store, true);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = HeaderCache::new();

    NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache);

    assert_eq!(ledger.current_height(), 0);
    assert_eq!(header_cache.count(), 0);
}

#[test]
fn hydrate_ledger_restores_height_and_headers() {
    let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
    let mut snapshot = store.get_snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).expect("mutable snapshot");

    // Persist two blocks (genesis index 0 and block index 1).
    let mut persist_block = |index: u32, nonce: u64| {
        let header = crate::ledger::block_header::BlockHeader {
            index,
            timestamp: index as u64,
            nonce,
            witnesses: vec![Witness::new()],
            ..Default::default()
        };
        let block = Block {
            header: header.clone(),
            transactions: Vec::new(),
        };
        let hash = block.hash();

        let key =
            crate::smart_contract::native::ledger_contract::keys::block_hash_storage_key(-4, index)
                .to_array();
        snapshot.put(key, hash.to_bytes().to_vec());

        let block_key =
            crate::smart_contract::native::ledger_contract::keys::block_storage_key(-4, &hash)
                .to_array();
        let mut writer = crate::neo_io::BinaryWriter::new();
        let trimmed = TrimmedBlock::from_block(&block);
        trimmed
            .serialize(&mut writer)
            .expect("trimmed block serialize");
        snapshot.put(block_key, writer.to_bytes());
        hash
    };

    let _genesis_hash = persist_block(0, 1);
    let hash = persist_block(1, 42);

    // Persist current block pointer.
    let current_key =
        crate::smart_contract::native::ledger_contract::keys::current_block_storage_key(-4)
            .to_array();
    let mut current_state = Vec::with_capacity(36);
    current_state.extend_from_slice(&hash.to_bytes());
    current_state.extend_from_slice(&1u32.to_le_bytes());
    snapshot.put(current_key, current_state);
    snapshot.commit();

    let store_cache = StoreCache::new_from_store(store, true);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = HeaderCache::new();

    NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache);
    assert_eq!(ledger.current_height(), 1);
    assert_eq!(header_cache.count(), 2);
    assert_eq!(ledger.block_hash_at(1), Some(hash));
}

#[test]
fn hydrate_ledger_respects_bounded_window() {
    let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
    let mut snapshot = store.get_snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).expect("mutable snapshot");

    // Persist a chain longer than the hydration window.
    let total_blocks = LEDGER_HYDRATION_WINDOW + 5;
    let mut last_hash = UInt256::zero();
    for index in 0..=total_blocks {
        let header = crate::ledger::block_header::BlockHeader {
            index,
            timestamp: index as u64,
            nonce: index as u64,
            witnesses: vec![Witness::new()],
            ..Default::default()
        };
        let block = Block {
            header: header.clone(),
            transactions: Vec::new(),
        };
        let hash = block.hash();

        let key =
            crate::smart_contract::native::ledger_contract::keys::block_hash_storage_key(-4, index)
                .to_array();
        snapshot.put(key, hash.to_bytes().to_vec());

        let block_key =
            crate::smart_contract::native::ledger_contract::keys::block_storage_key(-4, &hash)
                .to_array();
        let mut writer = crate::neo_io::BinaryWriter::new();
        let trimmed = TrimmedBlock::from_block(&block);
        trimmed.serialize(&mut writer).expect("serialize block");
        snapshot.put(block_key, writer.to_bytes());
        last_hash = hash;
    }

    // Persist current block pointer to the tip.
    let current_key =
        crate::smart_contract::native::ledger_contract::keys::current_block_storage_key(-4)
            .to_array();
    let mut current_state = Vec::with_capacity(36);
    current_state.extend_from_slice(&last_hash.to_bytes());
    current_state.extend_from_slice(&total_blocks.to_le_bytes());
    snapshot.put(current_key, current_state);
    snapshot.commit();

    let store_cache = StoreCache::new_from_store(store, true);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = HeaderCache::new();

    NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache);

    assert_eq!(ledger.current_height(), total_blocks);
    assert_eq!(ledger.block_hash_at(total_blocks), Some(last_hash));
    assert_eq!(header_cache.count(), LEDGER_HYDRATION_WINDOW as usize);
    // Genesis block should not be hydrated when the window is bounded.
    assert!(ledger.block_hash_at(0).is_none());
}

fn sample_u256(byte: u8) -> UInt256 {
    UInt256::from_bytes(&[byte; 32]).expect("uint256 from bytes")
}

fn sample_u160(byte: u8) -> UInt160 {
    UInt160::from_bytes(&[byte; 20]).expect("uint160 from bytes")
}

fn sample_ledger_header() -> LedgerBlockHeader {
    let witness = crate::Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
    LedgerBlockHeader {
        version: 1,
        previous_hash: sample_u256(1),
        merkle_root: sample_u256(2),
        timestamp: 42,
        nonce: 7,
        index: 10,
        primary_index: 3,
        next_consensus: sample_u160(5),
        witnesses: vec![witness],
    }
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
    added: AtomicUsize,
    removed: AtomicUsize,
    logs: AtomicUsize,
    logging: AtomicUsize,
    notify: AtomicUsize,
    wallet_changes: AtomicUsize,
}

impl EventProbe {
    fn added(&self) -> usize {
        self.added.load(Ordering::Relaxed)
    }

    fn removed(&self) -> usize {
        self.removed.load(Ordering::Relaxed)
    }

    fn logs(&self) -> usize {
        self.logs.load(Ordering::Relaxed)
    }

    fn logging(&self) -> usize {
        self.logging.load(Ordering::Relaxed)
    }

    fn notifies(&self) -> usize {
        self.notify.load(Ordering::Relaxed)
    }

    fn wallet_changes(&self) -> usize {
        self.wallet_changes.load(Ordering::Relaxed)
    }
}

impl ITransactionAddedHandler for EventProbe {
    fn memory_pool_transaction_added_handler(&self, _sender: &dyn Any, _tx: &Transaction) {
        self.added.fetch_add(1, Ordering::Relaxed);
    }
}

impl ITransactionRemovedHandler for EventProbe {
    fn memory_pool_transaction_removed_handler(
        &self,
        _sender: &dyn Any,
        _args: &TransactionRemovedEventArgs,
    ) {
        self.removed.fetch_add(1, Ordering::Relaxed);
    }
}

impl ILogHandler for EventProbe {
    fn application_engine_log_handler(
        &self,
        _sender: &ApplicationEngine,
        _log_event_args: &LogEventArgs,
    ) {
        self.logs.fetch_add(1, Ordering::Relaxed);
    }
}

impl ILoggingHandler for EventProbe {
    fn utility_logging_handler(&self, _source: &str, _level: LogLevel, _message: &str) {
        self.logging.fetch_add(1, Ordering::Relaxed);
    }
}

impl INotifyHandler for EventProbe {
    fn application_engine_notify_handler(
        &self,
        _sender: &ApplicationEngine,
        _notify_event_args: &NotifyEventArgs,
    ) {
        self.notify.fetch_add(1, Ordering::Relaxed);
    }
}

struct LoggingGuard;

impl LoggingGuard {
    fn install<F>(hook: F) -> Self
    where
        F: Fn(String, ExternalLogLevel, String) + Send + Sync + 'static,
    {
        ExtensionsUtility::set_logging(Some(Box::new(hook)));
        Self
    }
}

impl Drop for LoggingGuard {
    fn drop(&mut self) {
        ExtensionsUtility::set_logging(None);
    }
}

impl IWalletChangedHandler for EventProbe {
    fn i_wallet_provider_wallet_changed_handler(
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

#[async_trait]
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

    async fn change_password(
        &self,
        _old_password: &str,
        _new_password: &str,
    ) -> WalletResult<bool> {
        Ok(false)
    }

    fn contains(&self, _script_hash: &UInt160) -> bool {
        false
    }

    async fn create_account(&self, _private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    async fn create_account_with_contract(
        &self,
        _contract: Contract,
        _key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    async fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    async fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
        Ok(false)
    }

    async fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn get_account(&self, _script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        None
    }

    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        Vec::new()
    }

    async fn get_available_balance(&self, _asset_id: &UInt256) -> WalletResult<i64> {
        Ok(0)
    }

    async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
        Ok(0)
    }

    async fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    async fn import_nep2(
        &self,
        _nep2_key: &str,
        _password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        None
    }

    async fn set_default_account(&self, _script_hash: &UInt160) -> WalletResult<()> {
        Ok(())
    }

    async fn sign(&self, _data: &[u8], _script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        Err(WalletError::Other("not implemented".to_string()))
    }

    async fn sign_transaction(&self, _transaction: &mut Transaction) -> WalletResult<()> {
        Ok(())
    }

    async fn unlock(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn lock(&self) {}

    async fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    async fn save(&self) -> WalletResult<()> {
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

impl IWalletProvider for TestWalletProvider {
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
async fn transaction_event_handlers_receive_callbacks() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let handler = Arc::new(EventProbe::default());

    system
        .register_transaction_added_handler(handler.clone())
        .expect("register added");
    system
        .register_transaction_removed_handler(handler.clone())
        .expect("register removed");

    let tx = Transaction::default();
    let pool = system.mempool();
    let args = TransactionRemovedEventArgs {
        transactions: vec![tx.clone()],
        reason: TransactionRemovalReason::CapacityExceeded,
    };

    {
        let guard = pool.lock();
        if let Some(callback) = &guard.transaction_added {
            callback(&guard, &tx);
        }
        if let Some(callback) = &guard.transaction_removed {
            callback(&guard, &args);
        }
    }

    assert_eq!(handler.added(), 1);
    assert_eq!(handler.removed(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn log_and_logging_handlers_fire() {
    let _log_guard = LOG_TEST_MUTEX.lock();

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let handler = Arc::new(EventProbe::default());

    system
        .register_log_handler(handler.clone())
        .expect("register log handler");
    system
        .register_logging_handler(handler.clone())
        .expect("register logging handler");
    system
        .register_notify_handler(handler.clone())
        .expect("register notify handler");

    let system_ctx = system.context();
    NativeHelpers::attach_system_context(system_ctx.clone());
    let logging_ctx = Arc::downgrade(&system_ctx);
    let _logging_guard = LoggingGuard::install(move |source, level, message| {
        if let Some(ctx) = logging_ctx.upgrade() {
            let local_level: LogLevel = level;
            ctx.notify_logging_handlers(&source, local_level, &message);
        }
    });

    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("engine");
    engine.set_runtime_context(Some(system_ctx.clone()));

    // Push a log through the engine and invoke the logging hook.
    let container: Arc<dyn IVerifiable> = Arc::new(Transaction::default());
    let log_event = LogEventArgs::new(container, UInt160::default(), "hello".to_string());
    engine.push_log(log_event);
    ExtensionsUtility::set_log_level(ExternalLogLevel::Info);
    ExtensionsUtility::log("test", ExternalLogLevel::Info, "message");
    // Exercise both the Utility hook and direct notify; the hook increments log_counter.
    system_ctx.notify_logging_handlers("test", LogLevel::Info, "message");
    assert_eq!(handler.logs(), 1);
    assert!(handler.logging() >= 1);

    let notify = NotifyEventArgs::new(
        Arc::new(Transaction::default()) as Arc<dyn IVerifiable>,
        UInt160::default(),
        "evt".to_string(),
        vec![StackItem::from_int(1)],
    );
    engine.push_notification(notify);
    assert_eq!(handler.notifies(), 1);

    ExtensionsUtility::set_logging(None);
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
