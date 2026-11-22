use super::checkpoint::*;
use super::env::*;
use super::logs::*;
use super::run::*;
use super::session::*;
use super::*;
use crate::import_batching::ImportDrainPolicy;
use neo_io::Serializable;
use neo_primitives::UInt160;
use neo_primitives::UInt256;
use neo_protocol::payloads::Header;
use neo_protocol::payloads::block::Block;
use neo_protocol::witness::Witness;
use neo_rpc::application_logs::{ApplicationLogsService, PersistenceUnitOutcome};
use neo_storage::persistence::providers::RocksDBStoreProvider;
use neo_storage::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
use neo_storage::persistence::store::OnNewSnapshotDelegate;
use neo_storage::persistence::write_store::WriteStore;
use neo_storage::persistence::{
    SeekDirection, Store, StoreProvider, StoreSnapshot, storage::StorageConfig,
};
use neo_storage::{StorageError, StorageItem, StorageKey, StorageResult};
use neo_system::i_event_handlers::{CommittedHandler, CommittingHandler};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

fn write_acc_and_index() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("temporary directory");
    let acc_path = temp.path().join("chain.0.acc");
    let index_path = temp.path().join("chain.0.acc.offsets");
    let payloads: &[&[u8]] = &[&[0x10, 0x11], &[0x20, 0x21, 0x22], &[0x30]];

    let mut offsets = Vec::new();
    {
        let mut acc = File::create(&acc_path).expect("create ACC");
        acc.write_all(&10u32.to_le_bytes()).expect("ACC start");
        acc.write_all(&3u32.to_le_bytes()).expect("ACC count");
        for payload in payloads {
            offsets.push(acc.stream_position().expect("ACC offset"));
            acc.write_all(&(payload.len() as u32).to_le_bytes())
                .expect("block length");
            acc.write_all(payload).expect("block payload");
        }
    }

    let metadata = acc_path.metadata().expect("ACC metadata");
    let modified_ns = metadata
        .modified()
        .expect("ACC modification time")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("post-epoch timestamp")
        .as_nanos() as u64;
    {
        let mut index = File::create(&index_path).expect("create index");
        index.write_all(ACC_INDEX_MAGIC).expect("index magic");
        index.write_all(&10u32.to_le_bytes()).expect("index start");
        index.write_all(&3u32.to_le_bytes()).expect("index count");
        index
            .write_all(&metadata.len().to_le_bytes())
            .expect("index ACC size");
        index
            .write_all(&modified_ns.to_le_bytes())
            .expect("index ACC modification time");
        for offset in offsets {
            index
                .write_all(&offset.to_le_bytes())
                .expect("block offset");
        }
    }

    (temp, acc_path, index_path)
}

#[test]
fn offset_index_resumes_at_first_unpersisted_block() {
    let (_temp, acc_path, _index_path) = write_acc_and_index();
    let resume = indexed_resume_position(&acc_path, 10, 3, 11)
        .expect("valid offset index")
        .expect("indexed resume");

    assert_eq!(resume.first_item, 2);
    assert_eq!(resume.skipped, 2);
    let mut acc = File::open(&acc_path).expect("open ACC");
    acc.seek(SeekFrom::Start(resume.offset))
        .expect("seek to resume block");
    let mut size = [0u8; 4];
    acc.read_exact(&mut size).expect("resume block size");
    assert_eq!(u32::from_le_bytes(size), 1);
}

#[test]
fn stale_offset_index_is_rejected() {
    let (_temp, acc_path, index_path) = write_acc_and_index();
    let mut index = std::fs::OpenOptions::new()
        .write(true)
        .open(&index_path)
        .expect("open index");
    index
        .seek(SeekFrom::Start(8))
        .and_then(|_| index.write_all(&99u32.to_le_bytes()))
        .expect("corrupt index start");

    let error =
        indexed_resume_position(&acc_path, 10, 3, 11).expect_err("stale index must not be trusted");
    assert!(error.to_string().contains("stale or invalid"));
}

fn memory_system() -> Arc<NeoSystem> {
    NeoSystem::new(neo_protocol::ProtocolSettings::default(), None, None)
        .expect("memory-backed neo system")
}

fn hash_of(byte: u8) -> UInt256 {
    UInt256::from_bytes(&[byte; 32]).expect("32-byte hash")
}

#[tokio::test]
async fn stop_height_already_reached_imports_zero_blocks() {
    let system = memory_system();
    let start_height = system.current_block_index();

    // The header declares three records but no payload bytes follow it, so
    // any attempt to read a record would fail. Returning a summary is
    // therefore proof that the importer stopped before touching the
    // archive.
    let temp = tempfile::tempdir().expect("temporary directory");
    let acc_path = temp.path().join("chain.0.acc");
    {
        let mut acc = File::create(&acc_path).expect("create ACC");
        acc.write_all(&start_height.to_le_bytes())
            .expect("ACC start");
        acc.write_all(&3u32.to_le_bytes()).expect("ACC count");
    }

    let summary = import_acc_file_until(&system, &acc_path, None, Some(start_height))
        .expect("a stop height that is already met must not fail");

    assert_eq!(summary.imported, 0);
    assert_eq!(summary.skipped, 0);
    assert_eq!(summary.declared_start, start_height);
    assert_eq!(summary.declared_count, 3);
    assert_eq!(summary.final_height, start_height);
    assert_eq!(summary.elapsed_secs, 0.0);
    assert_eq!(summary.counters, ImportCounters::default());
    assert_eq!(summary.fast_path, FastPathDeltas::default());
    assert_eq!(summary.parallel, ParallelDeltas::default());
    assert_eq!(summary.profile.state_label(), "disabled");
    assert_eq!(system.current_block_index(), start_height);
}

#[test]
fn persisted_checkpoint_must_carry_the_exact_accepted_hash() {
    let accepted = hash_of(0x11);
    let error = verify_persisted_checkpoint(
        "unit-test",
        "disk",
        7,
        &accepted,
        PersistedCheckpoint {
            height: 7,
            hash: hash_of(0x22),
        },
    )
    .expect_err("a different hash at the right height must be rejected");

    let message = error.to_string();
    assert!(
        message.contains("block hash"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("does not match the last accepted block hash"),
        "unexpected error: {message}"
    );
}

#[test]
fn persisted_checkpoint_must_carry_the_accepted_height() {
    let accepted = hash_of(0x11);
    let error = verify_persisted_checkpoint(
        "unit-test",
        "local",
        7,
        &accepted,
        PersistedCheckpoint {
            height: 6,
            hash: accepted,
        },
    )
    .expect_err("a stale height must be rejected");

    assert!(
        error
            .to_string()
            .contains("does not match the last accepted block height"),
        "unexpected error: {error}"
    );
}

#[test]
fn persisted_checkpoint_matching_height_and_hash_is_accepted() {
    let accepted = hash_of(0x11);
    verify_persisted_checkpoint(
        "unit-test",
        "disk",
        7,
        &accepted,
        PersistedCheckpoint {
            height: 7,
            hash: accepted,
        },
    )
    .expect("the accepted block checkpoint must verify");
}

#[tokio::test]
async fn checkpoint_success_requires_a_durable_flush() {
    let system = memory_system();
    let expected = read_local_checkpoint(&system);

    // A best-effort `flush()` would silently succeed here; only the
    // fallible durable flush rejects a backend without durable writes.
    let error = flush_and_verify_checkpoint(
        &system,
        None,
        expected.height,
        &expected.hash,
        0,
        0,
        "unit-test",
    )
    .expect_err("a store without durable flush support must not report a checkpoint");

    assert!(
        error
            .to_string()
            .contains("durable chain-store flush failed"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn local_checkpoint_tracks_the_persisted_genesis_tip() {
    let system = memory_system();
    let checkpoint = read_local_checkpoint(&system);
    assert_eq!(checkpoint.height, system.current_block_index());

    let mut genesis = neo_ledger::create_genesis_block(system.settings());
    assert_eq!(checkpoint.hash, genesis.hash());
}

#[tokio::test]
async fn persistence_profiling_is_disabled_until_explicitly_requested() {
    let system = memory_system();
    let stats = system.execution_stats();
    let start = stats.profile_snapshot();
    assert!(!start.enabled);

    // Every requested stage collapses to a labelled zero rather than an
    // implied "measured zero cost" when profiling is off.
    let deltas = PersistenceProfileDeltas::between(&start, &stats.profile_snapshot());
    assert_eq!(deltas.state_label(), "disabled");
    assert_eq!(deltas.blocks, 0);
    assert_eq!(deltas.transaction_executions, 0);
    assert_eq!(deltas.snapshot_open_nanos, 0);
    assert_eq!(deltas.on_persist_nanos, 0);
    assert_eq!(deltas.transaction_loop_nanos, 0);
    assert_eq!(deltas.post_persist_nanos, 0);
    assert_eq!(deltas.participant_precommit_nanos, 0);
    assert_eq!(deltas.store_commit_nanos, 0);
    assert!(!stats.profile_snapshot().enabled);
}

/// Application-logs store wrapper that observes a checkpoint's durability
/// gate without changing what is stored.
///
/// Reads, snapshots and the real durable flush are delegated to the wrapped
/// store, so records land exactly where they do in production. Only the flush
/// is intercepted: it records what the logs database already held and how far
/// the durable watermark stood *before* the flush, which is what makes the
/// checkpoint ordering observable. Arming it makes the next flush fail, which
/// is how a test proves a checkpoint refuses to certify an undurable logs
/// database.
struct ObservedLogsStore {
    inner: Arc<dyn Store>,
    armed: AtomicBool,
    /// Whether the wrapped backend really implements an explicit durable
    /// flush, which decides what an unarmed flush reports.
    delegates_flush: bool,
    observations: Arc<Mutex<Vec<FlushObservation>>>,
    service: Mutex<Option<std::sync::Weak<ApplicationLogsService>>>,
}

/// What one durable flush saw before it ran.
#[derive(Debug, Clone, Copy)]
struct FlushObservation {
    /// Records already in the logs database when the flush started.
    stored_records: usize,
    /// Durable watermark when the flush started.
    watermark: Option<(u32, UInt256)>,
}

impl ObservedLogsStore {
    /// Wraps a backend that implements an explicit durable flush, so the flush
    /// this records is the one the backend actually runs.
    fn new(inner: Arc<dyn Store>, observations: Arc<Mutex<Vec<FlushObservation>>>) -> Self {
        Self {
            inner,
            armed: AtomicBool::new(false),
            delegates_flush: true,
            observations,
            service: Mutex::new(None),
        }
    }

    /// Wraps a backend that cannot fsync and stands in for one that can.
    ///
    /// A memory store has no durable-flush support to delegate to, so an
    /// unarmed flush reports success instead. That is what lets the
    /// memory-backed tests exercise the durability gate of a checkpoint
    /// without a real RocksDB directory.
    fn with_simulated_durability(
        inner: Arc<dyn Store>,
        observations: Arc<Mutex<Vec<FlushObservation>>>,
    ) -> Self {
        Self {
            delegates_flush: false,
            ..Self::new(inner, observations)
        }
    }

    /// Lets the wrapper read the durable watermark the service reports.
    fn watch(&self, service: &Arc<ApplicationLogsService>) {
        *self.service.lock().expect("service slot") = Some(Arc::downgrade(service));
    }

    /// Makes the next durable flush fail.
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Number of records currently in the wrapped logs database.
    ///
    /// The records use byte keys, so this reads through a snapshot, which is
    /// also the path ApplicationLogs itself reads.
    fn stored_records(&self) -> usize {
        self.inner
            .snapshot()
            .find(None::<&Vec<u8>>, SeekDirection::Forward)
            .count()
    }
}

// Byte-keyed reads always go through a snapshot: `Store` only promises the
// `StorageKey` shaped read trait plus byte-keyed writes, and ApplicationLogs
// reads its records through `store.snapshot()`. Delegating `snapshot()` to the
// wrapped store therefore keeps that path intact without claiming a byte-keyed
// read trait the wrapped `dyn Store` cannot provide.
impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for ObservedLogsStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        ReadOnlyStoreGeneric::<StorageKey, StorageItem>::try_get(&*self.inner, key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        ReadOnlyStoreGeneric::<StorageKey, StorageItem>::find(&*self.inner, key_prefix, direction)
    }
}

impl ReadOnlyStore for ObservedLogsStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for ObservedLogsStore {
    fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
        // ApplicationLogs only ever writes through a snapshot, so a direct
        // store write means the write path changed under this wrapper.
        Err(StorageError::invalid_operation(
            "observed logs store does not forward direct deletes",
        ))
    }

    fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
        Err(StorageError::invalid_operation(
            "observed logs store does not forward direct puts",
        ))
    }
}

impl Store for ObservedLogsStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.inner.snapshot()
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.inner.on_new_snapshot(handler);
    }

    fn try_flush_durable(&self) -> StorageResult<()> {
        // Count through a snapshot because the records use byte keys; the
        // snapshot sees exactly what the wrapped store would have written so far.
        let stored_records = self.stored_records();
        let watermark = self
            .service
            .lock()
            .expect("service slot")
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .and_then(|logs| logs.watermark());
        self.observations
            .lock()
            .expect("flush observations")
            .push(FlushObservation {
                stored_records,
                watermark,
            });
        if self.armed.load(Ordering::SeqCst) {
            return Err(StorageError::CommitFailed(
                "injected application logs durable flush failure".to_string(),
            ));
        }
        if self.delegates_flush {
            return self.inner.try_flush_durable();
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A RocksDB-backed node with ApplicationLogs registered.
///
/// The importer only reports completion after a *fallible* durable flush,
/// which a memory store does not implement, so every end-to-end import test
/// needs a real backend. ApplicationLogs is registered before genesis is
/// persisted, exactly like the node bootstrap does, so the log stream of an
/// import covers every height it also covers in production.
struct TestNode {
    chain_path: String,
    system: Arc<NeoSystem>,
    chain_store: Arc<dyn Store>,
    logs_store: Arc<dyn Store>,
    logs: Arc<ApplicationLogsService>,
}

impl TestNode {
    fn open(root: &Path) -> Self {
        Self::open_with_logs_store(root, |store| store)
    }

    /// Opens a node whose application-logs store is wrapped by `decorate`.
    ///
    /// The wrapper is how a test observes, or breaks, the logs durability gate
    /// of a checkpoint: the chain store stays a real RocksDB so its own
    /// fallible flush still succeeds.
    fn open_with_logs_store(
        root: &Path,
        decorate: impl FnOnce(Arc<dyn Store>) -> Arc<dyn Store>,
    ) -> Self {
        let settings = neo_protocol::ProtocolSettings::testnet();
        let chain_dir = root.join("chain");
        let logs_dir = root.join("application-logs");
        let chain_path = chain_dir.to_str().expect("ASCII chain path").to_string();
        let logs_path = logs_dir
            .to_str()
            .expect("ASCII application logs path")
            .to_string();

        let provider: Arc<dyn StoreProvider> = Arc::new(RocksDBStoreProvider::new(StorageConfig {
            path: chain_dir,
            read_only: false,
            ..Default::default()
        }));
        // The state service keeps its trie and root index in the same store
        // the importer writes, so the differential storage comparison below
        // covers the state roots as well.
        let state_settings = neo_system::state_service::StateServiceSettings {
            full_state: false,
            path: String::new(),
            network: settings.network,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: neo_primitives::UnhandledExceptionPolicy::Ignore,
        };
        let system = NeoSystem::new_with_state_service_deferring_genesis(
            settings.clone(),
            Some(provider),
            Some(chain_path.clone()),
            Some(state_settings),
        )
        .map_err(|err| anyhow::anyhow!(err.to_string()))
        .expect("rocksdb-backed neo system");

        let raw_logs_store = system
            .store_provider()
            .get_store(&logs_path)
            .map_err(|err| anyhow::anyhow!(err.to_string()))
            .expect("application logs store");
        let logs_store = decorate(raw_logs_store);
        let logs = Arc::new(ApplicationLogsService::new(
            neo_rpc::application_logs::ApplicationLogsSettings {
                enabled: true,
                network: settings.network,
                path: logs_path,
                ..Default::default()
            },
            Arc::clone(&logs_store),
        ));
        system
            .add_service::<ApplicationLogsService, _>(Arc::clone(&logs))
            .expect("register ApplicationLogs service");
        system
            .register_committing_handler(
                Arc::clone(&logs) as Arc<dyn CommittingHandler + Send + Sync>
            )
            .expect("register committing handler");
        system
            .register_committed_handler(
                Arc::clone(&logs) as Arc<dyn CommittedHandler + Send + Sync>
            )
            .expect("register committed handler");
        system
            .persist_genesis_if_absent()
            .map_err(|err| anyhow::anyhow!(err.to_string()))
            .expect("persist genesis");
        // Genesis records must be durable before the importer starts, exactly
        // as the node bootstrap leaves them. A wrapper that is armed to fail
        // only does so from the first checkpoint on, so this flush still runs.
        logs.try_flush_durable()
            .expect("genesis application logs flush");

        Self {
            chain_path,
            chain_store: system.store(),
            system,
            logs_store,
            logs,
        }
    }

    /// Hash of the tip an archive record must name as its predecessor.
    fn tip_hash(&self) -> UInt256 {
        read_local_checkpoint(&self.system).hash
    }

    fn import(
        &self,
        path: &Path,
        stop_height: Option<u32>,
        policy: ImportDrainPolicy,
    ) -> Result<ImportSummary> {
        import_acc_file_with_options(
            &self.system,
            path,
            Some(&self.chain_path),
            stop_height,
            Some(policy),
        )
    }

    fn chain_dump(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        dump_store(&self.chain_store)
    }

    fn logs_dump(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        dump_store(&self.logs_store)
    }

    async fn close(self) {
        let Self {
            system,
            chain_store,
            logs_store,
            logs,
            ..
        } = self;
        // The logs service owns a handle to the logs store, so it must go
        // before the store itself or a reopen keeps the backend locked.
        drop(logs);
        Arc::clone(&system).shutdown().await.expect("shutdown");
        // Every handle must be gone before a test reopens the same paths.
        drop(chain_store);
        drop(logs_store);
        drop(system);
    }
}

fn dump_store(store: &Arc<dyn Store>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let snapshot = store.snapshot();
    snapshot.find(None, SeekDirection::Forward).collect()
}

/// Builds an empty block on top of `prev_hash`.
fn empty_block(index: u32, prev_hash: UInt256) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(u64::from(index));
    header.witness = Witness::new();
    Block {
        header,
        transactions: Vec::new(),
    }
}

/// Builds `count` linked empty blocks starting at `first_index`.
fn empty_block_chain(first_index: u32, count: u32, first_prev: UInt256) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(count as usize);
    let mut prev_hash = first_prev;
    for item in 0..count {
        let mut block = empty_block(first_index + item, prev_hash);
        prev_hash = block.hash();
        blocks.push(block);
    }
    blocks
}

/// Writes an `.acc` archive holding exactly `blocks`.
fn write_archive(path: &Path, first_index: u32, blocks: &[Block]) {
    let mut acc = File::create(path).expect("create ACC");
    acc.write_all(&first_index.to_le_bytes())
        .expect("ACC start");
    acc.write_all(&(blocks.len() as u32).to_le_bytes())
        .expect("ACC count");
    for block in blocks {
        let mut writer = neo_io::BinaryWriter::new();
        block.serialize(&mut writer).expect("serialize block");
        let payload = writer.into_bytes();
        acc.write_all(&(payload.len() as u32).to_le_bytes())
            .expect("block length");
        acc.write_all(&payload).expect("block payload");
    }
}

/// Writes a linked run of empty blocks on top of the node tip.
fn write_empty_archive(path: &Path, node: &TestNode, count: u32) {
    let blocks = empty_block_chain(1, count, node.tip_hash());
    write_archive(path, 1, &blocks);
}

fn test_policy(empty_batch_cap: usize, flush_interval: u64) -> ImportDrainPolicy {
    // Progress windows are pushed out of reach so a run is only split by the
    // boundary under test.
    ImportDrainPolicy::new(empty_batch_cap, flush_interval, 100_000, 60)
}

/// A batched import must produce exactly the storage, state roots, receipts
/// and application logs that the same blocks produce when persisted alone.
#[tokio::test(flavor = "multi_thread")]
async fn batched_empty_import_matches_single_block_import() {
    let single_root = tempfile::tempdir().expect("temporary directory");
    let single = TestNode::open(single_root.path());
    let single_acc = single_root.path().join("chain.0.acc");
    write_empty_archive(&single_acc, &single, 7);
    let single_summary = single
        .import(&single_acc, None, test_policy(0, 10_000))
        .expect("single-block import");
    let single_storage = single.chain_dump();
    let single_logs = single.logs_dump();
    single.close().await;

    let batched_root = tempfile::tempdir().expect("temporary directory");
    let batched = TestNode::open(batched_root.path());
    let batched_acc = batched_root.path().join("chain.0.acc");
    write_empty_archive(&batched_acc, &batched, 7);
    let batched_summary = batched
        .import(&batched_acc, None, test_policy(3, 10_000))
        .expect("batched import");
    let batched_storage = batched.chain_dump();
    let batched_logs = batched.logs_dump();
    batched.close().await;

    assert_eq!(single_summary.imported, 7);
    assert_eq!(single_summary.counters.empty_block_batches, 0);
    assert_eq!(single_summary.counters.batched_empty_blocks, 0);
    assert_eq!(batched_summary.imported, 7);
    assert_eq!(batched_summary.final_height, single_summary.final_height);
    assert_eq!(batched_summary.counters.empty_blocks, 7);
    // Seven blocks with a cap of three drain as 3 + 3 + 1, so only the two
    // multi-block write-backs count as batches.
    assert_eq!(batched_summary.counters.empty_block_batches, 2);
    assert_eq!(batched_summary.counters.batched_empty_blocks, 6);

    assert_eq!(
        single_storage, batched_storage,
        "a batched import must leave byte-identical chain and state storage"
    );
    // Guard against a comparison that passes because both dumps are empty.
    assert!(
        single_storage.len() > 20,
        "the chain store must hold the imported chain, got {} keys",
        single_storage.len()
    );
    assert!(
        !single_logs.is_empty(),
        "the run must produce application logs to compare"
    );
    assert_eq!(
        single_logs, batched_logs,
        "a batched import must leave byte-identical application logs"
    );
    assert!(
        single_logs.len() > 7,
        "every imported height plus genesis must have a log record"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_run_is_persisted_exactly_at_the_cap() {
    let exact_root = tempfile::tempdir().expect("temporary directory");
    let exact = TestNode::open(exact_root.path());
    let exact_acc = exact_root.path().join("chain.0.acc");
    write_empty_archive(&exact_acc, &exact, 4);
    let exact_summary = exact
        .import(&exact_acc, None, test_policy(4, 10_000))
        .expect("import at the cap");
    exact.close().await;

    assert_eq!(exact_summary.imported, 4);
    assert_eq!(exact_summary.counters.empty_block_batches, 1);
    assert_eq!(exact_summary.counters.batched_empty_blocks, 4);

    // One block beyond the cap splits into a full batch and a single block.
    let overflow_root = tempfile::tempdir().expect("temporary directory");
    let overflow = TestNode::open(overflow_root.path());
    let overflow_acc = overflow_root.path().join("chain.0.acc");
    write_empty_archive(&overflow_acc, &overflow, 5);
    let overflow_summary = overflow
        .import(&overflow_acc, None, test_policy(4, 10_000))
        .expect("import past the cap");
    overflow.close().await;

    assert_eq!(overflow_summary.imported, 5);
    assert_eq!(overflow_summary.counters.empty_block_batches, 1);
    assert_eq!(overflow_summary.counters.batched_empty_blocks, 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn checkpoint_boundary_splits_a_run_below_the_cap() {
    let root = tempfile::tempdir().expect("temporary directory");
    let node = TestNode::open(root.path());
    let acc = root.path().join("chain.0.acc");
    write_empty_archive(&acc, &node, 7);

    // A cap of four would carry all seven blocks in two write-backs
    // (4 + 3). The checkpoint interval of three is what makes the run end at
    // three: a checkpoint is never crossed by a batch.
    let summary = node
        .import(&acc, None, test_policy(4, 3))
        .expect("import across checkpoints");
    node.close().await;

    assert_eq!(summary.imported, 7);
    assert_eq!(summary.counters.empty_block_batches, 2);
    assert_eq!(summary.counters.batched_empty_blocks, 6);
    assert_eq!(summary.final_height, 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_height_inside_a_run_drains_only_up_to_the_limit() {
    let root = tempfile::tempdir().expect("temporary directory");
    let node = TestNode::open(root.path());
    let acc = root.path().join("chain.0.acc");
    write_empty_archive(&acc, &node, 6);

    // A cap of six would persist the whole archive in one write-back; the
    // stop height truncates the run at the third block instead.
    let summary = node
        .import(&acc, Some(3), test_policy(6, 10_000))
        .expect("import up to the stop height");
    let tip = node.tip_hash();
    node.close().await;

    assert_eq!(summary.imported, 3);
    assert_eq!(summary.final_height, 3);
    assert_eq!(summary.counters.empty_block_batches, 1);
    assert_eq!(summary.counters.batched_empty_blocks, 3);
    assert_ne!(tip, UInt256::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_breaks_the_predecessor_chain_is_rejected_before_persistence() {
    let root = tempfile::tempdir().expect("temporary directory");
    let node = TestNode::open(root.path());
    let acc = root.path().join("chain.0.acc");
    let mut blocks = empty_block_chain(1, 4, node.tip_hash());
    // Block two now names a predecessor that is not block one, so the run is
    // not a linked chain even though every height is present.
    blocks[1].header.set_prev_hash(hash_of(0xEE));
    write_archive(&acc, 1, &blocks);

    let registers_before = node.logs_dump().len();
    let error = node
        .import(&acc, None, test_policy(4, 10_000))
        .expect_err("a broken predecessor chain must be rejected");
    let poisoned = node.system.is_persistence_poisoned();
    let height = node.system.current_block_index();
    let registers_after = node.logs_dump().len();
    node.close().await;

    // `{:#}` keeps the whole anyhow context chain, which is where the
    // rejected predecessor link is reported.
    let message = format!("{error:#}");
    assert!(
        message.contains("does not extend"),
        "unexpected error: {message}"
    );
    assert!(
        !poisoned,
        "a preflight rejection must not poison the system"
    );
    assert_eq!(height, 0, "no block of the rejected run may be accepted");
    assert!(
        registers_before > 0,
        "genesis must already have produced an application log"
    );
    assert_eq!(
        registers_after, registers_before,
        "a rejected run must not add application logs to the ones genesis produced"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_after_a_batched_import_skips_every_persisted_height() {
    let root = tempfile::tempdir().expect("temporary directory");
    let acc = root.path().join("chain.0.acc");

    let first = TestNode::open(root.path());
    write_empty_archive(&acc, &first, 6);
    let first_summary = first
        .import(&acc, None, test_policy(3, 10_000))
        .expect("batched import");
    let first_storage = first.chain_dump();
    let first_logs = first.logs_dump();
    first.close().await;

    // Reopening the same paths is what a restart does: the importer must see
    // the batched tip and skip exactly the heights it already persisted.
    let second = TestNode::open(root.path());
    assert_eq!(second.system.current_block_index(), 6);
    let second_summary = second
        .import(&acc, None, test_policy(3, 10_000))
        .expect("resumed import");
    let second_storage = second.chain_dump();
    let second_logs = second.logs_dump();
    second.close().await;

    assert_eq!(first_summary.imported, 6);
    assert_eq!(first_summary.counters.empty_block_batches, 2);
    assert_eq!(second_summary.imported, 0);
    assert_eq!(second_summary.skipped, 6);
    assert_eq!(second_summary.final_height, 6);
    assert_eq!(second_summary.counters.empty_block_batches, 0);
    assert_eq!(
        first_storage, second_storage,
        "a resumed import must not rewrite the persisted chain"
    );
    assert_eq!(
        first_logs, second_logs,
        "a resumed import must not add application logs"
    );
}

#[tokio::test]
async fn logs_unit_is_a_no_op_without_a_registered_logs_service() {
    let system = memory_system();
    let unit = LogsUnit::begin(&system).expect("open a unit without a logs service");
    unit.finish().expect("finish a unit without a logs service");
    LogsUnit::begin(&system)
        .expect("open a unit without a logs service")
        .abort();
}

/// An ApplicationLogs service attached to a system for checkpoint tests.
struct AttachedLogs {
    logs: Arc<ApplicationLogsService>,
    store: Arc<ObservedLogsStore>,
}

/// Registers an ApplicationLogs service on `system` and returns it with the
/// memory store its commits land in.
///
/// The store is wrapped so its durable flush is fallible exactly as a RocksDB
/// logs store is, and so the flush itself can be observed or armed to fail.
fn attach_logs_service(system: &Arc<NeoSystem>) -> AttachedLogs {
    let store = Arc::new(ObservedLogsStore::with_simulated_durability(
        Arc::new(neo_storage::persistence::providers::MemoryStore::new()),
        Arc::new(Mutex::new(Vec::new())),
    ));
    let logs = Arc::new(ApplicationLogsService::new(
        neo_rpc::application_logs::ApplicationLogsSettings {
            enabled: true,
            network: system.settings().network,
            ..Default::default()
        },
        Arc::clone(&store) as Arc<dyn Store>,
    ));
    system
        .add_service::<ApplicationLogsService, _>(Arc::clone(&logs))
        .expect("register ApplicationLogs service");
    store.watch(&logs);
    AttachedLogs { logs, store }
}

/// Stages one block's records inside the open unit exactly the way the
/// committing and committed callbacks do, including the boundary the
/// watermark is taken from.
fn stage_block(logs: &ApplicationLogsService, system: &Arc<NeoSystem>, block: &Block) {
    let execution = neo_protocol::execution::Block::from(block);
    let snapshot = neo_storage::persistence::PersistenceDataCache::new(false);
    logs.blockchain_committing_handler(system.as_ref(), &execution, &snapshot, &[]);
    logs.blockchain_committed_handler(system.as_ref(), &execution);
}

#[tokio::test]
async fn logs_watermark_verification_rejects_records_without_a_watermark() {
    let system = memory_system();
    let logs = attach_logs_service(&system).logs;

    let error = verify_logs_watermark(&logs, 1, &hash_of(0x01), 3, "unit-test")
        .expect_err("committed records without a watermark must not verify");
    assert!(
        error
            .to_string()
            .contains("without a durable block watermark"),
        "unexpected error: {error}"
    );

    // Nothing committed is not a failure: a plugin that stopped on its
    // exception policy leaves exactly this state and must not fail an import.
    verify_logs_watermark(&logs, 1, &hash_of(0x01), 0, "unit-test")
        .expect("an empty flush needs no watermark");
}

#[tokio::test]
async fn logs_watermark_verification_rejects_a_watermark_ahead_of_the_chain() {
    let system = memory_system();
    let logs = attach_logs_service(&system).logs;
    let session = LogsSession::begin(&system, 0, &UInt256::default()).expect("open session");

    let mut block = empty_block(9, hash_of(0x09));
    let block_hash = block.hash();
    assert_eq!(
        logs.begin_persistence_unit(),
        PersistenceUnitOutcome::Started
    );
    stage_block(&logs, &system, &block);
    logs.finish_persistence_unit().expect("finish unit");
    logs.flush_accepted().expect("flush accepted");
    assert_eq!(
        logs.watermark(),
        None,
        "a commit alone must not certify durability"
    );
    logs.try_flush_durable().expect("durable logs flush");
    assert_eq!(logs.watermark(), Some((9, block_hash)));

    let error = verify_logs_watermark(&logs, 8, &hash_of(0x08), 0, "unit-test")
        .expect_err("logs ahead of the chain must not verify");
    assert!(
        error
            .to_string()
            .contains("is ahead of the checkpoint height"),
        "unexpected error: {error}"
    );

    // The same watermark matches the checkpoint it actually describes.
    verify_logs_watermark(&logs, 9, &block_hash, 1, "unit-test")
        .expect("the flushed watermark must match its own checkpoint");
    session.end().expect("end session");
}

/// The checkpoint flushes the chain first, so a chain flush that cannot
/// become durable must leave the logs exactly where they were.
#[tokio::test]
async fn a_failed_chain_flush_leaves_the_logs_watermark_unmoved() {
    let system = memory_system();
    let attached = attach_logs_service(&system);
    let logs = &attached.logs;
    let session = LogsSession::begin(&system, 0, &UInt256::default()).expect("open session");
    assert!(logs.import_session_active());

    let mut block = empty_block(1, hash_of(0x01));
    assert_eq!(
        logs.begin_persistence_unit(),
        PersistenceUnitOutcome::Started
    );
    stage_block(logs, &system, &block);
    logs.finish_persistence_unit().expect("finish unit");
    assert_eq!(logs.flush_telemetry().pending_records, 1);

    let checkpoint = read_local_checkpoint(&system);
    let error = flush_and_verify_checkpoint(
        &system,
        None,
        checkpoint.height,
        &checkpoint.hash,
        1,
        0,
        "ordering-test",
    )
    .expect_err("a memory store cannot report a durable chain flush");
    assert!(
        error
            .to_string()
            .contains("durable chain-store flush failed"),
        "unexpected error: {error}"
    );

    // Step two of the checkpoint is the first that can fail it, so the logs
    // never advanced: the record is still pending and nothing was written.
    assert_eq!(logs.watermark(), None);
    assert_eq!(logs.flush_telemetry().pending_records, 1);
    assert!(
        attached.store.stored_records() == 0,
        "a failed chain flush must not commit application logs"
    );

    // Closing the session still writes the record, so nothing was lost, but a
    // closing commit is not a durable flush: the watermark stays where the
    // last confirmed flush left it.
    let deltas = session.end().expect("end session");
    assert_eq!(deltas.flush_count, 1);
    assert_eq!(deltas.pending_records, 0);
    assert!(
        attached.store.stored_records() > 0,
        "the closing commit must write the record it inherited"
    );
    assert_eq!(
        logs.watermark(),
        None,
        "a closing commit without a durable flush must not certify a block"
    );
}

/// A logs durable flush that fails must fail the checkpoint instead of
/// certifying it.
///
/// The chain store is a real RocksDB, so its own fallible flush succeeds and
/// the checkpoint really does reach the logs step. That is what makes the
/// failure evidence about the logs gate rather than about the chain.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_logs_durable_flush_fails_the_checkpoint() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store_slot: Arc<Mutex<Option<Arc<ObservedLogsStore>>>> = Arc::new(Mutex::new(None));
    let node = {
        let slot = Arc::clone(&store_slot);
        TestNode::open_with_logs_store(root.path(), move |inner| {
            let store = Arc::new(ObservedLogsStore::new(
                inner,
                Arc::new(Mutex::new(Vec::new())),
            ));
            *slot.lock().expect("store slot") = Some(Arc::clone(&store));
            store as Arc<dyn Store>
        })
    };
    let observed = store_slot
        .lock()
        .expect("store slot")
        .clone()
        .expect("observed logs store");
    observed.watch(&node.logs);
    // `open` flushed the genesis records itself; every later flush fails.
    observed.observations.lock().expect("observations").clear();
    observed.arm();

    let acc = root.path().join("chain.0.acc");
    write_empty_archive(&acc, &node, 2);
    let error = node
        .import(&acc, None, test_policy(1, 1))
        .expect_err("a logs durable flush failure must fail the checkpoint");
    let message = format!("{error:#}");
    let height = node.system.current_block_index();
    let watermark = node.logs.watermark();
    let observations = std::mem::take(&mut *observed.observations.lock().expect("observations"));
    drop(observed);
    node.close().await;

    assert!(
        message.contains("durable application-logs flush failed"),
        "unexpected error: {message}"
    );
    assert!(
        height >= 1,
        "the chain flush of the checkpoint still succeeded, so the logs step is what failed"
    );
    assert_eq!(
        watermark, None,
        "the watermark must not advance when the flush that would confirm it failed"
    );
    assert_eq!(
        observations.len(),
        2,
        "the checkpoint's flush plus the best-effort flush of the error path"
    );
    assert!(
        observations[0].stored_records > 0,
        "the records were committed before the flush that failed"
    );
    assert_eq!(
        observations[0].watermark, None,
        "the failing checkpoint flush must not see a promoted watermark"
    );
}

/// The checkpoint order is chain flush, logs commit, logs durable flush, then
/// the watermark, and only then may it be reported successful.
#[tokio::test(flavor = "multi_thread")]
async fn checkpoint_promotes_the_logs_watermark_only_after_the_durable_flush() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store_slot: Arc<Mutex<Option<Arc<ObservedLogsStore>>>> = Arc::new(Mutex::new(None));
    let node = {
        let slot = Arc::clone(&store_slot);
        TestNode::open_with_logs_store(root.path(), move |inner| {
            let store = Arc::new(ObservedLogsStore::new(
                inner,
                Arc::new(Mutex::new(Vec::new())),
            ));
            *slot.lock().expect("store slot") = Some(Arc::clone(&store));
            store as Arc<dyn Store>
        })
    };
    let observed = store_slot
        .lock()
        .expect("store slot")
        .clone()
        .expect("observed logs store");
    observed.watch(&node.logs);
    observed.observations.lock().expect("observations").clear();

    let acc = root.path().join("chain.0.acc");
    write_empty_archive(&acc, &node, 3);
    let summary = node
        .import(&acc, None, test_policy(1, 3))
        .expect("an import whose logs flush succeeds");
    let observations = std::mem::take(&mut *observed.observations.lock().expect("observations"));
    let watermark = node.logs.watermark();
    let tip = node.tip_hash();
    drop(observed);
    node.close().await;

    assert_eq!(
        summary.imported, 3,
        "success is reported only after the checkpoint gates passed"
    );
    assert_eq!(
        observations.len(),
        1,
        "the run is exactly one checkpoint interval"
    );
    assert!(
        observations[0].stored_records > 0,
        "the logs commit must precede the durable flush"
    );
    assert_eq!(
        observations[0].watermark, None,
        "the watermark must still be unadvanced while the flush runs"
    );
    assert_eq!(
        watermark,
        Some((3, tip)),
        "the watermark advances only once the flush succeeded"
    );
}

/// Ten blocks inside one checkpoint interval cost exactly one logs commit.
#[tokio::test(flavor = "multi_thread")]
async fn import_coalesces_application_logs_into_one_commit() {
    let root = tempfile::tempdir().expect("temporary directory");
    let node = TestNode::open(root.path());
    let acc = root.path().join("chain.0.acc");
    write_empty_archive(&acc, &node, 10);

    // The checkpoint interval reaches past the archive, so only the final
    // checkpoint commits and the session closes on an empty pending set.
    let summary = node
        .import(&acc, None, test_policy(0, 10_000))
        .expect("coalesced import");
    let logs = node.logs_dump();
    node.close().await;

    assert_eq!(summary.imported, 10);
    assert_eq!(
        summary.logs.flush_count, 1,
        "one checkpoint interval must cost one logs commit"
    );
    assert_eq!(summary.logs.flushed_records, 10);
    assert!(summary.logs.flushed_bytes > 0);
    assert_eq!(
        summary.logs.pending_records, 0,
        "closing the session must leave nothing pending"
    );
    assert!(
        logs.len() > 10,
        "genesis plus every imported height has a log record, got {}",
        logs.len()
    );
}

/// A checkpoint interval inside the archive commits once per interval, never
/// once per block.
#[tokio::test(flavor = "multi_thread")]
async fn periodic_checkpoints_commit_application_logs_once_each() {
    let root = tempfile::tempdir().expect("temporary directory");
    let node = TestNode::open(root.path());
    let acc = root.path().join("chain.0.acc");
    write_empty_archive(&acc, &node, 9);

    let summary = node
        .import(&acc, None, test_policy(0, 3))
        .expect("import across checkpoints");
    node.close().await;

    assert_eq!(summary.imported, 9);
    assert_eq!(
        summary.logs.flush_count, 3,
        "three checkpoints must cost three logs commits"
    );
    assert_eq!(summary.logs.flushed_records, 9);
    assert_eq!(summary.logs.pending_records, 0);
}
