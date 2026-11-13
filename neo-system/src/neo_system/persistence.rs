//! Concrete adapters between `NeoSystem` and the canonical ledger persistence pipeline.

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

thread_local! {
    /// Diagnostic-only block classification shared with nested MPT/store sinks.
    /// The guard is installed only when persistence profiling is explicitly enabled.
    static PROFILE_EMPTY_BLOCK: Cell<bool> = const { Cell::new(false) };
}

struct EmptyBlockProfileGuard {
    previous: bool,
}

impl EmptyBlockProfileGuard {
    fn new(empty: bool) -> Self {
        let previous = PROFILE_EMPTY_BLOCK.with(|current| current.replace(empty));
        Self { previous }
    }
}

impl Drop for EmptyBlockProfileGuard {
    fn drop(&mut self) {
        PROFILE_EMPTY_BLOCK.with(|current| current.set(self.previous));
    }
}

use neo_actors::EventStreamHandle;
use neo_crypto::mpt_trie::MptProfileSnapshot;
use neo_ledger::persistence::{
    ApplicationEnginePort, CommitParticipant, EngineFactory, EvidenceSink, ExecutionReceipt,
    FaultCategory, FaultObservation, LedgerError, NativeCacheHandle, NativeEnginePort,
    PersistOptions, PersistenceProfiler, PersistenceStage, RuntimeCacheSink, SeededNativeContracts,
    SnapshotStore, StoreTransactionPort,
};
use neo_primitives::{CallFlags, TriggerType, UInt160, UInt256, Verifiable};
use neo_protocol::execution::{ApplicationExecuted, Block};
use neo_smart_contract::application_engine::{
    ApplicationEngine, EngineNativeServices, LedgerTransactionStates, TEST_MODE_GAS,
};
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::persistence::data_cache::DataCacheConfig;
use neo_storage::persistence::{
    DataCache, StoreCommitProfiler, StoreCommitStage, StoreTransaction, store::Store,
};
use neo_vm::VmState;
use neo_vm::fast_path::{Nep17Parser, TransferParams};
use parking_lot::Mutex;

/// Selects the transaction execution engine used by block persistence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Always execute through the canonical VM interpreter.
    #[default]
    Canonical,
    /// Use direct native dispatch only after strict script and state eligibility checks.
    Auto,
    /// Dual-execute eligible transfers on isolated snapshots and reject any divergence.
    Verify,
}

impl ExecutionMode {
    /// Returns the stable configuration value for this mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Auto => "auto",
            Self::Verify => "verify",
        }
    }
}

const PROFILE_SNAPSHOT_OPEN: usize = 0;
const PROFILE_TRANSACTION_HASH: usize = 1;
const PROFILE_ON_PERSIST: usize = 2;
const PROFILE_TRANSACTION_LOOP: usize = 3;
const PROFILE_TRANSACTION_VM: usize = 4;
const PROFILE_POST_PERSIST: usize = 5;
const PROFILE_PARTICIPANT_PRECOMMIT: usize = 6;
const PROFILE_STORE_COMMIT: usize = 7;
const PROFILE_RUNTIME_UPDATE: usize = 8;
const PROFILE_POST_COMMIT: usize = 9;
const PROFILE_STATE_CHANGE_EXTRACTION: usize = 10;
const PROFILE_MPT: usize = 11;
const PROFILE_STORE_WRITER_SNAPSHOT_OPEN: usize = 12;
const PROFILE_STORE_CHANGE_APPLICATION: usize = 13;
const PROFILE_STORE_WRITE_BATCH_BUILD: usize = 14;
const PROFILE_STORE_DATABASE_WRITE: usize = 15;
const PROFILE_STORE_CACHE_INVALIDATION: usize = 16;
const PROFILE_MPT_UPDATE: usize = 17;
const PROFILE_MPT_COMMIT: usize = 18;
const PROFILE_MPT_ROOT_HASH: usize = 19;
const PROFILE_MPT_HASH: usize = 20;
const PROFILE_MPT_SERIALIZATION: usize = 21;
const PROFILE_MPT_HASH256: usize = 22;
const PROFILE_MPT_NODE_CACHE: usize = 23;
const PROFILE_MPT_NODE_READ: usize = 24;
const PROFILE_MPT_NODE_DESERIALIZE: usize = 25;
const PROFILE_MPT_NODE_ENCODE: usize = 26;
const PROFILE_MPT_NODE_WRITE: usize = 27;
const PROFILE_STAGE_COUNT: usize = 28;

/// Aggregated opt-in wall-clock timings for the real persistence pipeline.
/// `transaction_vm` is nested in `transaction_loop`; `mpt` and
/// `state_change_extraction` are nested in `participant_precommit`; all
/// `store_*` stages are nested in `store_commit`. MPT operation, hashing,
/// serialization, and node-I/O timers also overlap as documented by `MptProfile`.
#[derive(Clone, Debug, Default)]
pub struct ExecutionProfileSnapshot {
    pub enabled: bool,
    pub blocks: u64,
    pub transaction_executions: u64,
    pub snapshot_open: Duration,
    pub transaction_hash: Duration,
    pub on_persist: Duration,
    pub transaction_loop: Duration,
    pub transaction_vm: Duration,
    pub post_persist: Duration,
    pub participant_precommit: Duration,
    pub store_commit: Duration,
    pub runtime_update: Duration,
    pub post_commit: Duration,
    pub state_change_extraction: Duration,
    pub mpt: Duration,
    pub store_writer_snapshot_open: Duration,
    pub store_change_application: Duration,
    pub store_write_batch_build: Duration,
    pub store_database_write: Duration,
    pub store_cache_invalidation: Duration,
    pub mpt_update: Duration,
    pub mpt_commit: Duration,
    pub mpt_root_hash: Duration,
    pub mpt_hash: Duration,
    pub mpt_serialization: Duration,
    pub mpt_hash256: Duration,
    pub mpt_node_cache: Duration,
    pub mpt_node_read: Duration,
    pub mpt_node_deserialize: Duration,
    pub mpt_node_encode: Duration,
    pub mpt_node_write: Duration,
    /// The same wall-clock stages restricted to blocks with zero transactions.
    /// Subtract these from the aggregate to obtain the non-empty-block cost.
    pub empty_blocks: EmptyBlockProfileSnapshot,
    /// Aggregated by stable four-byte syscall hash. Nested host calls overlap.
    pub syscalls: Vec<SyscallProfile>,
    /// Inclusive dynamic-call boundaries; nested calls overlap their callers.
    pub contract_calls: Vec<ContractCallProfile>,
    /// Executed opcode counts from both the canonical and fast interpreters.
    pub opcodes: Vec<OpcodeProfile>,
    /// Instructions actually executed by the fast interpreter.
    pub external_instructions: u64,
}

#[derive(Clone, Debug, Default)]
pub struct EmptyBlockProfileSnapshot {
    pub blocks: u64,
    pub snapshot_open: Duration,
    pub on_persist: Duration,
    pub transaction_loop: Duration,
    pub post_persist: Duration,
    pub participant_precommit: Duration,
    pub state_change_extraction: Duration,
    pub mpt: Duration,
    pub mpt_update: Duration,
    pub mpt_commit: Duration,
    pub mpt_hash: Duration,
    pub mpt_serialization: Duration,
    pub mpt_hash256: Duration,
    pub mpt_node_cache: Duration,
    pub mpt_node_read: Duration,
    pub mpt_node_deserialize: Duration,
    pub mpt_node_encode: Duration,
    pub mpt_node_write: Duration,
    pub store_commit: Duration,
    pub store_database_write: Duration,
    pub runtime_update: Duration,
    pub post_commit: Duration,
}

#[derive(Clone, Debug)]
pub struct OpcodeProfile {
    pub opcode: u8,
    pub count: u64,
}

#[derive(Clone, Debug)]
pub struct SyscallProfile {
    pub hash: u32,
    pub calls: u64,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct ContractCallProfile {
    pub contract_hash: String,
    pub method: String,
    pub argument_count: usize,
    pub calls: u64,
    pub elapsed: Duration,
}

/// Process-local execution counters. These are observability data, never consensus state.
#[derive(Debug)]
pub struct ExecutionStats {
    direct_transfers: AtomicU64,
    profiling_enabled: AtomicBool,
    profile_nanos: [AtomicU64; PROFILE_STAGE_COUNT],
    empty_profile_nanos: [AtomicU64; PROFILE_STAGE_COUNT],
    profiled_blocks: AtomicU64,
    profiled_empty_blocks: AtomicU64,
    profiled_transaction_executions: AtomicU64,
    syscall_profile: Mutex<BTreeMap<u32, (u64, u64)>>,
    contract_call_profile: Mutex<BTreeMap<(String, String, usize), (u64, u64)>>,
    opcode_profile: [AtomicU64; 256],
    external_instructions: AtomicU64,
}

impl Default for ExecutionStats {
    fn default() -> Self {
        Self {
            direct_transfers: AtomicU64::default(),
            profiling_enabled: AtomicBool::default(),
            profile_nanos: std::array::from_fn(|_| AtomicU64::default()),
            empty_profile_nanos: std::array::from_fn(|_| AtomicU64::default()),
            profiled_blocks: AtomicU64::default(),
            profiled_empty_blocks: AtomicU64::default(),
            profiled_transaction_executions: AtomicU64::default(),
            syscall_profile: Mutex::default(),
            contract_call_profile: Mutex::default(),
            opcode_profile: std::array::from_fn(|_| AtomicU64::default()),
            external_instructions: AtomicU64::default(),
        }
    }
}

impl ExecutionStats {
    /// Number of transactions actually executed by direct native NEP-17 dispatch.
    pub fn direct_transfer_count(&self) -> u64 {
        self.direct_transfers.load(Ordering::Relaxed)
    }

    /// Enables or disables detailed persistence timing. Enabling starts a new sample.
    pub fn set_profiling(&self, enabled: bool) {
        if enabled {
            for value in &self.profile_nanos {
                value.store(0, Ordering::Relaxed);
            }
            for value in &self.empty_profile_nanos {
                value.store(0, Ordering::Relaxed);
            }
            for value in &self.opcode_profile {
                value.store(0, Ordering::Relaxed);
            }
            self.profiled_blocks.store(0, Ordering::Relaxed);
            self.profiled_empty_blocks.store(0, Ordering::Relaxed);
            self.profiled_transaction_executions
                .store(0, Ordering::Relaxed);
            self.external_instructions.store(0, Ordering::Relaxed);
            self.syscall_profile.lock().clear();
            self.contract_call_profile.lock().clear();
        }
        self.profiling_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Returns a consistent-enough observability snapshot; it is not consensus state.
    pub fn profile_snapshot(&self) -> ExecutionProfileSnapshot {
        let duration =
            |index: usize| Duration::from_nanos(self.profile_nanos[index].load(Ordering::Relaxed));
        let empty_duration = |index: usize| {
            Duration::from_nanos(self.empty_profile_nanos[index].load(Ordering::Relaxed))
        };
        ExecutionProfileSnapshot {
            enabled: self.profiling_enabled.load(Ordering::Relaxed),
            blocks: self.profiled_blocks.load(Ordering::Relaxed),
            transaction_executions: self.profiled_transaction_executions.load(Ordering::Relaxed),
            snapshot_open: duration(PROFILE_SNAPSHOT_OPEN),
            transaction_hash: duration(PROFILE_TRANSACTION_HASH),
            on_persist: duration(PROFILE_ON_PERSIST),
            transaction_loop: duration(PROFILE_TRANSACTION_LOOP),
            transaction_vm: duration(PROFILE_TRANSACTION_VM),
            post_persist: duration(PROFILE_POST_PERSIST),
            participant_precommit: duration(PROFILE_PARTICIPANT_PRECOMMIT),
            store_commit: duration(PROFILE_STORE_COMMIT),
            runtime_update: duration(PROFILE_RUNTIME_UPDATE),
            post_commit: duration(PROFILE_POST_COMMIT),
            state_change_extraction: duration(PROFILE_STATE_CHANGE_EXTRACTION),
            mpt: duration(PROFILE_MPT),
            store_writer_snapshot_open: duration(PROFILE_STORE_WRITER_SNAPSHOT_OPEN),
            store_change_application: duration(PROFILE_STORE_CHANGE_APPLICATION),
            store_write_batch_build: duration(PROFILE_STORE_WRITE_BATCH_BUILD),
            store_database_write: duration(PROFILE_STORE_DATABASE_WRITE),
            store_cache_invalidation: duration(PROFILE_STORE_CACHE_INVALIDATION),
            mpt_update: duration(PROFILE_MPT_UPDATE),
            mpt_commit: duration(PROFILE_MPT_COMMIT),
            mpt_root_hash: duration(PROFILE_MPT_ROOT_HASH),
            mpt_hash: duration(PROFILE_MPT_HASH),
            mpt_serialization: duration(PROFILE_MPT_SERIALIZATION),
            mpt_hash256: duration(PROFILE_MPT_HASH256),
            mpt_node_cache: duration(PROFILE_MPT_NODE_CACHE),
            mpt_node_read: duration(PROFILE_MPT_NODE_READ),
            mpt_node_deserialize: duration(PROFILE_MPT_NODE_DESERIALIZE),
            mpt_node_encode: duration(PROFILE_MPT_NODE_ENCODE),
            mpt_node_write: duration(PROFILE_MPT_NODE_WRITE),
            empty_blocks: EmptyBlockProfileSnapshot {
                blocks: self.profiled_empty_blocks.load(Ordering::Relaxed),
                snapshot_open: empty_duration(PROFILE_SNAPSHOT_OPEN),
                on_persist: empty_duration(PROFILE_ON_PERSIST),
                transaction_loop: empty_duration(PROFILE_TRANSACTION_LOOP),
                post_persist: empty_duration(PROFILE_POST_PERSIST),
                participant_precommit: empty_duration(PROFILE_PARTICIPANT_PRECOMMIT),
                state_change_extraction: empty_duration(PROFILE_STATE_CHANGE_EXTRACTION),
                mpt: empty_duration(PROFILE_MPT),
                mpt_update: empty_duration(PROFILE_MPT_UPDATE),
                mpt_commit: empty_duration(PROFILE_MPT_COMMIT),
                mpt_hash: empty_duration(PROFILE_MPT_HASH),
                mpt_serialization: empty_duration(PROFILE_MPT_SERIALIZATION),
                mpt_hash256: empty_duration(PROFILE_MPT_HASH256),
                mpt_node_cache: empty_duration(PROFILE_MPT_NODE_CACHE),
                mpt_node_read: empty_duration(PROFILE_MPT_NODE_READ),
                mpt_node_deserialize: empty_duration(PROFILE_MPT_NODE_DESERIALIZE),
                mpt_node_encode: empty_duration(PROFILE_MPT_NODE_ENCODE),
                mpt_node_write: empty_duration(PROFILE_MPT_NODE_WRITE),
                store_commit: empty_duration(PROFILE_STORE_COMMIT),
                store_database_write: empty_duration(PROFILE_STORE_DATABASE_WRITE),
                runtime_update: empty_duration(PROFILE_RUNTIME_UPDATE),
                post_commit: empty_duration(PROFILE_POST_COMMIT),
            },
            syscalls: self
                .syscall_profile
                .lock()
                .iter()
                .map(|(&hash, &(calls, nanos))| SyscallProfile {
                    hash,
                    calls,
                    elapsed: Duration::from_nanos(nanos),
                })
                .collect(),
            contract_calls: self
                .contract_call_profile
                .lock()
                .iter()
                .map(
                    |((hash, method, argument_count), &(calls, nanos))| ContractCallProfile {
                        contract_hash: hash.clone(),
                        method: method.clone(),
                        argument_count: *argument_count,
                        calls,
                        elapsed: Duration::from_nanos(nanos),
                    },
                )
                .collect(),
            opcodes: self
                .opcode_profile
                .iter()
                .enumerate()
                .filter_map(|(opcode, count)| {
                    let count = count.load(Ordering::Relaxed);
                    (count > 0).then_some(OpcodeProfile {
                        opcode: opcode as u8,
                        count,
                    })
                })
                .collect(),
            external_instructions: self.external_instructions.load(Ordering::Relaxed),
        }
    }

    /// Records state-root substages when a diagnostic sink is attached.
    pub fn record_state_root_profile(
        &self,
        change_extraction: Duration,
        mpt: Duration,
        mpt_detail: MptProfileSnapshot,
    ) {
        if self.profiling_enabled.load(Ordering::Relaxed) {
            self.record_duration(PROFILE_STATE_CHANGE_EXTRACTION, change_extraction);
            self.record_duration(PROFILE_MPT, mpt);
            self.record_duration(PROFILE_MPT_UPDATE, mpt_detail.update);
            self.record_duration(PROFILE_MPT_COMMIT, mpt_detail.commit);
            self.record_duration(PROFILE_MPT_ROOT_HASH, mpt_detail.root_hash);
            self.record_duration(PROFILE_MPT_HASH, mpt_detail.hash);
            self.record_duration(PROFILE_MPT_SERIALIZATION, mpt_detail.serialization);
            self.record_duration(PROFILE_MPT_HASH256, mpt_detail.hash256);
            self.record_duration(PROFILE_MPT_NODE_CACHE, mpt_detail.node_cache);
            self.record_duration(PROFILE_MPT_NODE_READ, mpt_detail.node_read);
            self.record_duration(PROFILE_MPT_NODE_DESERIALIZE, mpt_detail.node_deserialize);
            self.record_duration(PROFILE_MPT_NODE_ENCODE, mpt_detail.node_encode);
            self.record_duration(PROFILE_MPT_NODE_WRITE, mpt_detail.node_write);
        }
    }

    fn record_duration(&self, index: usize, elapsed: Duration) {
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        self.profile_nanos[index].fetch_add(nanos, Ordering::Relaxed);
        if PROFILE_EMPTY_BLOCK.with(Cell::get) {
            self.empty_profile_nanos[index].fetch_add(nanos, Ordering::Relaxed);
        }
    }

    fn record_direct_transfer(&self) {
        self.direct_transfers.fetch_add(1, Ordering::Relaxed);
    }
}

impl PersistenceProfiler for ExecutionStats {
    fn record(&self, stage: PersistenceStage, elapsed: Duration) {
        if !self.profiling_enabled.load(Ordering::Relaxed) {
            return;
        }
        let index = match stage {
            PersistenceStage::SnapshotOpen => {
                self.profiled_blocks.fetch_add(1, Ordering::Relaxed);
                if PROFILE_EMPTY_BLOCK.with(Cell::get) {
                    self.profiled_empty_blocks.fetch_add(1, Ordering::Relaxed);
                }
                PROFILE_SNAPSHOT_OPEN
            }
            PersistenceStage::TransactionHash => PROFILE_TRANSACTION_HASH,
            PersistenceStage::OnPersist => PROFILE_ON_PERSIST,
            PersistenceStage::TransactionLoop => PROFILE_TRANSACTION_LOOP,
            PersistenceStage::TransactionVm => {
                self.profiled_transaction_executions
                    .fetch_add(1, Ordering::Relaxed);
                PROFILE_TRANSACTION_VM
            }
            PersistenceStage::PostPersist => PROFILE_POST_PERSIST,
            PersistenceStage::ParticipantPrecommit => PROFILE_PARTICIPANT_PRECOMMIT,
            PersistenceStage::StoreCommit => PROFILE_STORE_COMMIT,
            PersistenceStage::RuntimeUpdate => PROFILE_RUNTIME_UPDATE,
            PersistenceStage::PostCommit => PROFILE_POST_COMMIT,
        };
        self.record_duration(index, elapsed);
    }
}

impl StoreCommitProfiler for ExecutionStats {
    fn record(&self, stage: StoreCommitStage, elapsed: Duration) {
        if !self.profiling_enabled.load(Ordering::Relaxed) {
            return;
        }
        let index = match stage {
            StoreCommitStage::WriterSnapshotOpen => PROFILE_STORE_WRITER_SNAPSHOT_OPEN,
            StoreCommitStage::ChangeApplication => PROFILE_STORE_CHANGE_APPLICATION,
            StoreCommitStage::WriteBatchBuild => PROFILE_STORE_WRITE_BATCH_BUILD,
            StoreCommitStage::DatabaseWrite => PROFILE_STORE_DATABASE_WRITE,
            StoreCommitStage::CacheInvalidation => PROFILE_STORE_CACHE_INVALIDATION,
        };
        self.record_duration(index, elapsed);
    }
}

impl neo_smart_contract::application_engine::ApplicationEngineProfiler for ExecutionStats {
    fn record_syscall(&self, hash: u32, elapsed: Duration) {
        if !self.profiling_enabled.load(Ordering::Relaxed) {
            return;
        }
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        let mut profile = self.syscall_profile.lock();
        let entry = profile.entry(hash).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(nanos);
    }

    fn record_opcode(&self, opcode: u8) {
        if self.profiling_enabled.load(Ordering::Relaxed) {
            self.opcode_profile[usize::from(opcode)].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_external_instruction(&self) {
        if self.profiling_enabled.load(Ordering::Relaxed) {
            self.external_instructions.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_contract_call(
        &self,
        contract_hash: UInt160,
        method: &str,
        argument_count: usize,
        elapsed: Duration,
    ) {
        if !self.profiling_enabled.load(Ordering::Relaxed) {
            return;
        }
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        let key = (contract_hash.to_string(), method.to_owned(), argument_count);
        let mut profile = self.contract_call_profile.lock();
        let entry = profile.entry(key).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(nanos);
    }
}

impl std::str::FromStr for ExecutionMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "canonical" => Ok(Self::Canonical),
            "auto" => Ok(Self::Auto),
            "verify" => Ok(Self::Verify),
            other => Err(format!(
                "unsupported execution mode '{other}'; expected canonical, auto, or verify"
            )),
        }
    }
}

use super::context::{
    NeoSystemContext, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionExceptionCategory,
    TransactionExecutionObservation, TransactionExecutionStage,
};
use super::core::NeoSystem;
use crate::adapters::HandlerParticipantBridge;
use crate::events::PluginEvent;

fn engine_error(error: impl std::fmt::Display) -> LedgerError {
    LedgerError::Engine {
        message: error.to_string(),
    }
}

struct SystemSnapshotStore {
    store: Arc<dyn Store>,
    commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
    empty_block_batch: Option<Arc<EmptyBlockWriteBack>>,
}

impl SystemSnapshotStore {
    fn new(store: Arc<dyn Store>, commit_profiler: Option<Arc<dyn StoreCommitProfiler>>) -> Self {
        Self {
            store,
            commit_profiler,
            empty_block_batch: None,
        }
    }

    fn for_empty_block_batch(
        store: Arc<dyn Store>,
        commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
        expected_blocks: usize,
    ) -> Self {
        let write_back =
            EmptyBlockWriteBack::new(store.clone(), commit_profiler.clone(), expected_blocks);
        Self {
            store,
            commit_profiler,
            empty_block_batch: Some(Arc::new(write_back)),
        }
    }

    fn finish_empty_block_batch(self) -> Result<(), String> {
        let batch = self
            .empty_block_batch
            .ok_or_else(|| "empty-block write-back is not active".to_string())?;
        let batch = Arc::try_unwrap(batch)
            .map_err(|_| "empty-block write-back still has live transaction handles".to_string())?;
        batch.finish()
    }
}

impl SnapshotStore for SystemSnapshotStore {
    fn open_transaction(
        &self,
        _options: &PersistOptions,
    ) -> Result<Box<dyn StoreTransactionPort>, LedgerError> {
        if let Some(batch) = &self.empty_block_batch {
            return Ok(Box::new(SystemStoreTransaction {
                transaction: None,
                base_snapshot: Arc::clone(&batch.base_snapshot),
                empty_block_batch: Some(Arc::clone(batch)),
            }));
        }

        let config = DataCacheConfig {
            track_reads_in_write_cache: true,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        };
        let mut transaction =
            StoreTransaction::from_snapshot_with_config(self.store.snapshot(), config);
        transaction.set_commit_profiler(self.commit_profiler.clone());
        let base_snapshot = Arc::new(transaction.cache().data_cache().clone());
        Ok(Box::new(SystemStoreTransaction {
            transaction: Some(transaction),
            base_snapshot,
            empty_block_batch: None,
        }))
    }
}

/// A flat write-back cache shared only by a bounded run of transaction-free blocks.
///
/// Every canonical block commit harvests just that block's delta and then uses
/// `DataCache::commit` to retain visible values as hot `None` entries. No database
/// write occurs until `finish`, which applies the coalesced final state in one
/// physical transaction. Dropping this object before `finish` writes nothing.
struct EmptyBlockWriteBack {
    transaction: Mutex<Option<StoreTransaction>>,
    base_snapshot: Arc<DataCache>,
    changes: Mutex<BTreeMap<neo_storage::StorageKey, Option<neo_storage::StorageItem>>>,
    committed_blocks: AtomicU64,
    expected_blocks: usize,
}

impl EmptyBlockWriteBack {
    fn new(
        store: Arc<dyn Store>,
        commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
        expected_blocks: usize,
    ) -> Self {
        let config = DataCacheConfig {
            track_reads_in_write_cache: true,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        };
        let mut transaction = StoreTransaction::from_snapshot_with_config(store.snapshot(), config);
        transaction.set_commit_profiler(commit_profiler);
        let base_snapshot = Arc::new(transaction.cache().data_cache().clone());
        Self {
            transaction: Mutex::new(Some(transaction)),
            base_snapshot,
            changes: Mutex::new(BTreeMap::new()),
            committed_blocks: AtomicU64::new(0),
            expected_blocks,
        }
    }

    fn commit_logical_block(&self) -> Result<(), String> {
        let mut changes = self.changes.lock();
        self.base_snapshot
            .for_each_tracked_change(|key, item, state| {
                let value = match state {
                    neo_storage::persistence::TrackState::Added
                    | neo_storage::persistence::TrackState::Changed => Some(item.clone()),
                    neo_storage::persistence::TrackState::Deleted => None,
                    neo_storage::persistence::TrackState::None
                    | neo_storage::persistence::TrackState::NotFound => return,
                };
                changes.insert(key.clone(), value);
            });
        // Preserve all visible values as hot entries while clearing only the
        // tracked delta. The next block therefore sees its predecessor without
        // inheriting that predecessor's pending MPT/storage changes.
        self.base_snapshot.commit();
        if self.base_snapshot.has_pending_changes() {
            return Err("logical empty-block commit left pending changes".to_string());
        }
        self.committed_blocks.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        let committed = self.committed_blocks.load(Ordering::Relaxed);
        if committed != self.expected_blocks as u64 {
            return Err(format!(
                "empty-block batch committed {committed} logical blocks; expected {}",
                self.expected_blocks
            ));
        }
        if self.base_snapshot.has_pending_changes() {
            return Err("empty-block batch finished with uncommitted logical changes".to_string());
        }

        let mut transaction = self
            .transaction
            .into_inner()
            .ok_or_else(|| "empty-block physical transaction was already consumed".to_string())?;
        for (key, value) in self.changes.into_inner() {
            match value {
                Some(item) => transaction
                    .cache_mut()
                    .try_update(key, item)
                    .map_err(|error| error.to_string())?,
                None => transaction
                    .cache_mut()
                    .try_delete(key)
                    .map_err(|error| error.to_string())?,
            }
        }
        transaction.commit().map_err(|error| error.to_string())
    }
}

struct SystemStoreTransaction {
    transaction: Option<StoreTransaction>,
    base_snapshot: Arc<DataCache>,
    empty_block_batch: Option<Arc<EmptyBlockWriteBack>>,
}

impl StoreTransactionPort for SystemStoreTransaction {
    fn base_snapshot(&self) -> Arc<DataCache> {
        Arc::clone(&self.base_snapshot)
    }

    fn commit(mut self: Box<Self>) -> Result<(), String> {
        if let Some(batch) = self.empty_block_batch {
            return batch.commit_logical_block();
        }
        self.transaction
            .take()
            .expect("store transaction committed once")
            .commit()
            .map_err(|error| error.to_string())
    }
}

struct SystemRuntimeCache {
    context: Arc<NeoSystemContext>,
}

impl RuntimeCacheSink for SystemRuntimeCache {
    fn record_block(&self, block: &Block) -> Result<(), String> {
        self.context
            .record_block(block.to_payload_block())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn record_tip(&self, index: u32) {
        self.context.ledger_handle().record_tip(index);
    }

    fn broadcast_block_received(&self, block_hash: String, block_height: u32) {
        self.context
            .broadcast_plugin_event(PluginEvent::BlockReceived {
                block_hash,
                block_height,
            });
    }
}

struct SystemEvidenceSink {
    context: Arc<NeoSystemContext>,
}

impl EvidenceSink for SystemEvidenceSink {
    fn available_capacity(&self) -> usize {
        self.context.transaction_execution_sink_available_capacity()
    }

    fn try_emit(&self, observation: &FaultObservation) -> bool {
        let observed_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let exception_category = match observation.category {
            FaultCategory::CaughtVmException => {
                TransactionExecutionExceptionCategory::CaughtVmException
            }
            FaultCategory::VmFault => TransactionExecutionExceptionCategory::VmFault,
        };
        self.context
            .try_emit_transaction_execution_observation(TransactionExecutionObservation {
                transaction_hash: observation.transaction_hash,
                block_height: observation.block_height,
                block_hash: observation.block_hash,
                execution_stage: TransactionExecutionStage::Application,
                vm_state: observation.vm_state,
                exception_category,
                gas_consumed: observation.gas_consumed,
                schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
                rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
                observed_at_unix_ms,
            })
    }

    fn record_dropped(&self, dropped: u64) {
        self.context
            .record_dropped_transaction_execution_observations(
                usize::try_from(dropped).unwrap_or(usize::MAX),
            );
    }
}

#[derive(Default)]
struct ExecutionRecorder {
    executions: Mutex<Vec<ApplicationExecuted>>,
    event_stream: Mutex<Option<EventStreamHandle>>,
}

impl ExecutionRecorder {
    fn with_event_stream(event_stream: EventStreamHandle) -> Self {
        Self {
            executions: Mutex::new(Vec::new()),
            event_stream: Mutex::new(Some(event_stream)),
        }
    }

    fn capture(&self, engine: &mut ApplicationEngine) -> ExecutionReceipt {
        let transaction = engine.script_container().and_then(|container| {
            container
                .as_any()
                .downcast_ref::<neo_protocol::payloads::Transaction>()
                .cloned()
        });
        let transaction_hash = transaction.as_ref().map(|transaction| transaction.hash());
        if let Some(hash) = transaction_hash {
            let state = engine.state();
            engine.record_transaction_vm_state(&hash, state);
        }

        let receipt = ExecutionReceipt {
            trigger: engine.trigger(),
            transaction_hash,
            vm_state: engine.state(),
            gas_consumed: engine.fee_consumed(),
            exception: engine.fault_exception().map(str::to_owned),
        };
        let executed = ApplicationExecuted::new(
            transaction,
            receipt.trigger,
            receipt.vm_state,
            receipt.exception.clone(),
            receipt.gas_consumed,
            engine.result_stack().to_vec(),
            engine.notifications().to_vec(),
            engine.logs().to_vec(),
        );
        if let Some(event_stream) = self.event_stream.lock().as_ref() {
            event_stream.publish(executed.clone());
        }
        self.executions.lock().push(executed);
        receipt
    }

    fn snapshot(&self) -> Vec<ApplicationExecuted> {
        self.executions.lock().clone()
    }
}

struct SystemEngineFactory {
    settings: neo_protocol::ProtocolSettings,
    native_services: EngineNativeServices,
    native_method_cache: NativeCacheHandle,
    recorder: Arc<ExecutionRecorder>,
    execution_mode: ExecutionMode,
    stats: Arc<ExecutionStats>,
    // A factory is constructed for exactly one persist_block_internal call.
    // All engines for that block share one immutable payload projection.
    payload_block: std::sync::OnceLock<Arc<neo_protocol::payloads::Block>>,
}

impl SystemEngineFactory {
    fn payload_block(&self, block: &Block) -> Arc<neo_protocol::payloads::Block> {
        Arc::clone(
            self.payload_block
                .get_or_init(|| Arc::new(block.to_payload_block())),
        )
    }

    fn engine_port(&self, mut engine: ApplicationEngine) -> SystemEnginePort {
        engine.set_profiler(Some(Arc::clone(&self.stats)
            as Arc<
                dyn neo_smart_contract::application_engine::ApplicationEngineProfiler,
            >));
        SystemEnginePort {
            engine,
            recorder: Arc::clone(&self.recorder),
            execution_mode: self.execution_mode,
            stats: Arc::clone(&self.stats),
            script: None,
            transfer: None,
            verification: None,
            verification_error: None,
        }
    }
}

impl EngineFactory for SystemEngineFactory {
    fn open_on_persist(
        &self,
        snapshot: Arc<DataCache>,
        block: Arc<Block>,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
        let engine = ApplicationEngine::new_with_shared_block(
            TriggerType::OnPersist,
            None,
            snapshot,
            Some(self.payload_block(&block)),
            self.settings.clone(),
            TEST_MODE_GAS,
            None,
            self.native_services.clone(),
        )
        .map_err(engine_error)?
        .with_native_contract_cache_handle(Arc::clone(&self.native_method_cache));
        Ok(Box::new(self.engine_port(engine)))
    }

    fn open_application(
        &self,
        container: Arc<dyn Verifiable>,
        snapshot: Arc<DataCache>,
        block: Arc<Block>,
        system_fee: i64,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError> {
        let payload_block = self.payload_block(&block);
        let verification =
            (self.execution_mode == ExecutionMode::Verify).then(|| VerificationContext {
                container: Arc::clone(&container),
                snapshot: Arc::clone(&snapshot),
                payload_block: Arc::clone(&payload_block),
                system_fee,
                seeded: seeded.clone(),
                cache: Arc::clone(cache),
                settings: self.settings.clone(),
                native_services: self.native_services.clone(),
            });
        let engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::Application,
            Some(container),
            snapshot,
            Some(payload_block),
            self.settings.clone(),
            system_fee,
            seeded.clone(),
            Arc::clone(cache),
            None,
            self.native_services.clone(),
        )
        .map_err(engine_error)?;
        let mut port = self.engine_port(engine);
        port.verification = verification;
        Ok(Box::new(port))
    }

    fn open_post_persist(
        &self,
        snapshot: Arc<DataCache>,
        block: Arc<Block>,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
        let engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::PostPersist,
            None,
            snapshot,
            Some(self.payload_block(&block)),
            self.settings.clone(),
            TEST_MODE_GAS,
            seeded.clone(),
            Arc::clone(cache),
            None,
            self.native_services.clone(),
        )
        .map_err(engine_error)?;
        Ok(Box::new(self.engine_port(engine)))
    }
}

#[derive(Clone)]
struct VerificationContext {
    container: Arc<dyn Verifiable>,
    snapshot: Arc<DataCache>,
    payload_block: Arc<neo_protocol::payloads::Block>,
    system_fee: i64,
    seeded: SeededNativeContracts,
    cache: NativeCacheHandle,
    settings: neo_protocol::ProtocolSettings,
    native_services: EngineNativeServices,
}

impl VerificationContext {
    fn execute_canonical(&self, script: Vec<u8>) -> Result<ApplicationEngine, String> {
        let isolated = Arc::new(self.snapshot.fork_isolated());
        let mut engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::Application,
            Some(Arc::clone(&self.container)),
            isolated,
            Some(Arc::clone(&self.payload_block)),
            self.settings.clone(),
            self.system_fee,
            self.seeded.clone(),
            Arc::clone(&self.cache),
            None,
            self.native_services.clone(),
        )
        .map_err(|error| error.to_string())?;
        engine
            .load_script(script, CallFlags::ALL, None)
            .map_err(|error| error.to_string())?;
        engine.execute_allow_fault();
        Ok(engine)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ExecutionFingerprint {
    state: VmState,
    fee_consumed: i64,
    exception: Option<String>,
    result_stack: Vec<neo_vm::StackItem>,
    notifications: Vec<(UInt160, String, Vec<neo_vm::StackItem>)>,
    logs: Vec<(UInt160, String)>,
    storage: Vec<(Vec<u8>, u8, Vec<u8>)>,
}

impl ExecutionFingerprint {
    fn capture(engine: &ApplicationEngine) -> Self {
        let mut storage: Vec<_> = engine
            .original_snapshot_cache()
            .tracked_items()
            .into_iter()
            .map(|(key, entry)| (key.to_array(), entry.state as u8, entry.item.to_value()))
            .collect();
        storage.sort_by(|left, right| left.0.cmp(&right.0));
        Self {
            state: engine.state(),
            fee_consumed: engine.fee_consumed(),
            exception: engine.fault_exception().map(str::to_owned),
            result_stack: engine.result_stack().to_vec(),
            notifications: engine
                .notifications()
                .iter()
                .map(|event| {
                    (
                        event.script_hash,
                        event.event_name.clone(),
                        event.state.clone(),
                    )
                })
                .collect(),
            logs: engine
                .logs()
                .iter()
                .map(|event| (event.script_hash, event.message.clone()))
                .collect(),
            storage,
        }
    }
}

struct SystemEnginePort {
    engine: ApplicationEngine,
    recorder: Arc<ExecutionRecorder>,
    execution_mode: ExecutionMode,
    stats: Arc<ExecutionStats>,
    script: Option<Vec<u8>>,
    transfer: Option<TransferParams>,
    verification: Option<VerificationContext>,
    verification_error: Option<String>,
}

impl SystemEnginePort {
    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        self.engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()))
    }

    fn eligible_transfer(&mut self) -> Option<TransferParams> {
        let params = self.transfer.clone()?;
        self.engine
            .can_execute_direct_nep17_transfer(&params)
            .then_some(params)
    }

    fn execute_auto(&mut self) -> VmState {
        match self.eligible_transfer() {
            Some(params) => {
                self.stats.record_direct_transfer();
                self.engine.execute_direct_nep17_transfer(&params)
            }
            None => self.engine.execute_allow_fault(),
        }
    }

    fn execute_verified(&mut self) -> VmState {
        if self.eligible_transfer().is_none() {
            return self.engine.execute_allow_fault();
        }
        let Some(verification) = self.verification.clone() else {
            self.verification_error = Some("missing isolated verification context".to_string());
            return self.engine.execute_allow_fault();
        };
        let Some(script) = self.script.clone() else {
            self.verification_error = Some("missing script for isolated verification".to_string());
            return self.engine.execute_allow_fault();
        };
        let canonical = match verification.execute_canonical(script) {
            Ok(engine) => engine,
            Err(error) => {
                self.verification_error = Some(format!(
                    "failed to construct isolated canonical verification: {error}"
                ));
                return self.engine.execute_allow_fault();
            }
        };
        let direct_state = self.execute_auto();
        let expected = ExecutionFingerprint::capture(&canonical);
        let actual = ExecutionFingerprint::capture(&self.engine);
        if expected != actual {
            self.verification_error = Some(format!(
                "direct/canonical execution divergence: expected {expected:?}, got {actual:?}"
            ));
        }
        // Keep the canonical engine alive through fingerprint capture; its
        // isolated snapshot is never committed into the ledger overlay.
        drop(canonical);
        direct_state
    }
}

impl NativeEnginePort for SystemEnginePort {
    fn native_on_persist(&mut self) -> Result<(), LedgerError> {
        self.engine.native_on_persist().map_err(engine_error)
    }

    fn native_post_persist(&mut self) -> Result<(), LedgerError> {
        self.engine.native_post_persist().map_err(engine_error)
    }

    fn record_native_persist_success(&mut self) {
        self.engine.record_native_persist_success();
    }

    fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
        self.engine.set_state(states);
    }

    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        SystemEnginePort::take_transaction_states(self)
    }

    fn seed_native_contracts(&self) -> SeededNativeContracts {
        let native_hashes: HashSet<_> = self
            .engine
            .native_contracts()
            .into_iter()
            .map(|contract| contract.hash())
            .collect();
        let mut seeded = self.engine.contracts_shared_snapshot();
        seeded.retain(|hash, _| native_hashes.contains(hash));
        seeded
    }

    fn native_cache_handle(&self) -> NativeCacheHandle {
        self.engine.native_contract_cache_handle()
    }

    fn execution_receipt(&mut self) -> ExecutionReceipt {
        self.recorder.capture(&mut self.engine)
    }
}

impl ApplicationEnginePort for SystemEnginePort {
    fn load_script(&mut self, script: Vec<u8>) -> Result<(), LedgerError> {
        self.transfer = Nep17Parser::parse_transfer_script(&script).ok();
        self.script = Some(script.clone());
        self.verification_error = None;
        self.engine
            .load_script(script, CallFlags::ALL, None)
            .map_err(engine_error)
    }

    fn execute_allow_fault(&mut self) -> VmState {
        match self.execution_mode {
            ExecutionMode::Canonical => self.engine.execute_allow_fault(),
            ExecutionMode::Auto => self.execute_auto(),
            ExecutionMode::Verify => self.execute_verified(),
        }
    }

    fn verify_execution(&self) -> Result<(), LedgerError> {
        match &self.verification_error {
            Some(message) => Err(LedgerError::Engine {
                message: message.clone(),
            }),
            None => Ok(()),
        }
    }

    fn record_transaction_vm_state(&mut self, transaction_hash: &UInt256, vm_state: VmState) {
        self.engine
            .record_transaction_vm_state(transaction_hash, vm_state);
    }

    fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
        self.engine.set_state(states);
    }

    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        SystemEnginePort::take_transaction_states(self)
    }

    fn fee_consumed(&self) -> i64 {
        self.engine.fee_consumed()
    }

    fn fault_exception(&self) -> Option<String> {
        self.engine.fault_exception().map(str::to_owned)
    }

    fn execution_receipt(&mut self) -> ExecutionReceipt {
        self.recorder.capture(&mut self.engine)
    }
}

impl NeoSystem {
    /// Persists a block through the configured execution and commit pipeline.
    pub fn persist_block(&self, block: Block) -> Result<(), String> {
        self.persist_block_internal(block, true, self.execution_mode())
            .map(|_| ())
    }

    /// Persists a block with an explicitly selected execution engine.
    pub fn persist_block_with_execution_mode(
        &self,
        block: Block,
        execution_mode: ExecutionMode,
    ) -> Result<(), String> {
        self.persist_block_internal(block, true, execution_mode)
            .map(|_| ())
    }

    /// Persists a block and returns the actual native and transaction execution receipts.
    pub fn persist_block_with_receipts(
        &self,
        block: Block,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        self.persist_block_internal(block, true, self.execution_mode())
    }

    /// Persists with receipts and an explicitly selected execution engine.
    pub fn persist_block_with_receipts_and_execution_mode(
        &self,
        block: Block,
        execution_mode: ExecutionMode,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        self.persist_block_internal(block, true, execution_mode)
    }

    /// Persists an imported block without retaining it in online runtime caches.
    pub fn persist_block_without_runtime_cache(
        &self,
        block: neo_protocol::payloads::Block,
    ) -> CoreResult<Vec<ApplicationExecuted>> {
        self.persist_block_without_runtime_cache_with_execution_mode(block, self.execution_mode())
    }

    /// Persists an imported block with an explicitly selected execution engine.
    pub fn persist_block_without_runtime_cache_with_execution_mode(
        &self,
        block: neo_protocol::payloads::Block,
        execution_mode: ExecutionMode,
    ) -> CoreResult<Vec<ApplicationExecuted>> {
        self.persist_block_internal(Block::from(&block), false, execution_mode)
            .map_err(CoreError::system)
    }

    /// Maximum number of logical blocks retained by one empty-block write-back cache.
    pub const MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS: usize = 4096;

    /// Persists a bounded run of transaction-free blocks through the canonical pipeline.
    ///
    /// Logical state, receipts and state roots are produced for every height, while
    /// physical writes are coalesced into one atomic store transaction. Any error
    /// requires discarding this `NeoSystem`: committed callbacks from earlier logical
    /// blocks may already have advanced process-local state even though the batch's
    /// physical transaction was not committed.
    pub fn persist_consecutive_empty_blocks_without_runtime_cache(
        &self,
        mut blocks: Vec<neo_protocol::payloads::Block>,
    ) -> Result<Vec<Vec<ApplicationExecuted>>, String> {
        if !(2..=Self::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS).contains(&blocks.len()) {
            return Err(format!(
                "empty-block write-back requires 2..={} blocks",
                Self::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS
            ));
        }
        if blocks.iter().any(|block| !block.transactions.is_empty()) {
            return Err("empty-block write-back rejected a transaction-bearing block".to_string());
        }
        let mut previous: Option<(u32, neo_primitives::UInt256)> = None;
        for block in &mut blocks {
            if let Some((previous_index, previous_hash)) = previous
                && (block.index() != previous_index + 1 || block.prev_hash() != &previous_hash)
            {
                return Err("empty-block write-back requires contiguous block hashes".to_string());
            }
            previous = Some((block.index(), block.hash()));
        }

        let commit_profiler = self
            .execution_stats
            .profiling_enabled
            .load(Ordering::Relaxed)
            .then(|| Arc::clone(&self.execution_stats) as Arc<dyn StoreCommitProfiler>);
        let store =
            SystemSnapshotStore::for_empty_block_batch(self.store(), commit_profiler, blocks.len());
        let execution_mode = self.execution_mode();
        let mut executions = Vec::with_capacity(blocks.len());
        for block in blocks {
            executions.push(self.persist_block_with_store(
                Block::from(&block),
                false,
                execution_mode,
                &store,
            )?);
        }
        store.finish_empty_block_batch()?;
        Ok(executions)
    }

    fn persist_block_internal(
        &self,
        block: Block,
        update_runtime_cache: bool,
        execution_mode: ExecutionMode,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        let commit_profiler = self
            .execution_stats
            .profiling_enabled
            .load(Ordering::Relaxed)
            .then(|| Arc::clone(&self.execution_stats) as Arc<dyn StoreCommitProfiler>);
        let store = SystemSnapshotStore::new(self.store(), commit_profiler);
        self.persist_block_with_store(block, update_runtime_cache, execution_mode, &store)
    }

    fn persist_block_with_store(
        &self,
        block: Block,
        update_runtime_cache: bool,
        execution_mode: ExecutionMode,
        store: &SystemSnapshotStore,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        let context = self.context();
        let recorder = Arc::new(ExecutionRecorder::with_event_stream(context.event_stream()));
        let engines = SystemEngineFactory {
            settings: self.settings().clone(),
            native_services: self.native_services.clone(),
            native_method_cache: self.native_method_cache_handle(),
            recorder: Arc::clone(&recorder),
            execution_mode,
            stats: Arc::clone(&self.execution_stats),
            payload_block: std::sync::OnceLock::new(),
        };
        let runtime_cache = SystemRuntimeCache {
            context: Arc::clone(&context),
        };
        let evidence = SystemEvidenceSink {
            context: Arc::clone(&context),
        };
        let participants = self.commit_participants(Arc::clone(&recorder))?;
        let options = PersistOptions {
            fast_sync: context.is_fast_sync_mode(),
            update_runtime_cache,
        };

        let profiler = self
            .execution_stats
            .profiling_enabled
            .load(Ordering::Relaxed)
            .then_some(self.execution_stats.as_ref() as &dyn PersistenceProfiler);
        let _empty_block_profile =
            profiler.map(|_| EmptyBlockProfileGuard::new(block.transactions.is_empty()));
        neo_ledger::persist_block_profiled(
            block,
            store,
            &engines,
            &runtime_cache,
            &evidence,
            &participants,
            &options,
            profiler,
        )
        .map_err(|error| error.to_string())?;
        Ok(recorder.snapshot())
    }

    fn commit_participants(
        &self,
        recorder: Arc<ExecutionRecorder>,
    ) -> Result<Vec<Arc<dyn CommitParticipant>>, String> {
        let system = self
            .self_ref
            .lock()
            .upgrade()
            .ok_or_else(|| "NeoSystem is not available for block persistence".to_string())?;
        let context = self.context();
        let committing = { context.committing_handlers.read().clone() };
        let committed = { context.committed_handlers.read().clone() };
        let mut participants = Vec::with_capacity(committing.len() + committed.len());

        for handler in committing {
            let run_during_fast_sync = handler.run_during_fast_sync();
            let system_for_commit = Arc::clone(&system);
            let system_for_rollback = Arc::clone(&system);
            let rollback_handler = Arc::clone(&handler);
            let recorder_for_commit = Arc::clone(&recorder);
            let participant = HandlerParticipantBridge::new(
                move |stage| {
                    let executed = recorder_for_commit.snapshot();
                    handler
                        .try_blockchain_committing_handler(
                            system_for_commit.as_ref(),
                            stage.block,
                            stage.snapshot,
                            &executed,
                        )
                        .map_err(|error| error.to_string())
                },
                |_| {},
            )
            .with_rollback(move |stage| {
                if let Err(error) = rollback_handler.blockchain_rollback_handler(
                    system_for_rollback.as_ref(),
                    stage.block,
                    stage.snapshot,
                ) {
                    tracing::warn!(
                        target: "neo::persistence",
                        index = stage.block.index(),
                        %error,
                        "commit participant rollback failed"
                    );
                }
            })
            .with_fast_sync(run_during_fast_sync);
            participants.push(Arc::new(participant) as Arc<dyn CommitParticipant>);
        }

        for handler in committed {
            let system_for_commit = Arc::clone(&system);
            let participant = HandlerParticipantBridge::new(
                |_| Ok(()),
                move |commit| {
                    handler.blockchain_committed_handler(system_for_commit.as_ref(), commit.block);
                },
            );
            participants.push(Arc::new(participant) as Arc<dyn CommitParticipant>);
        }

        Ok(participants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_ledger::{HeaderCache, LedgerContext};
    use neo_native_contracts::ledger_contract::LedgerContract;
    use neo_native_contracts::native::NativeHelpers;
    use neo_native_contracts::native::composition::default_native_services;
    use neo_native_contracts::{ContractManagement, GasToken};
    use neo_primitives::{UInt160, WitnessScope};
    use neo_protocol::ProtocolSettings;
    use neo_protocol::payloads::{Header, Signer, Transaction};
    use neo_protocol::witness::Witness;
    use neo_smart_contract::application_engine::NativeContract;
    use neo_vm::{OpCode, ScriptBuilder};
    use num_bigint::BigInt;

    fn transfer_block(
        system: &NeoSystem,
        from: UInt160,
        recipient: UInt160,
        amount: i64,
        witness: UInt160,
    ) -> neo_protocol::payloads::Block {
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::PUSHNULL);
        builder.emit_push_int(amount);
        builder.emit_push(&recipient.to_bytes());
        builder.emit_push(&from.to_bytes());
        builder.emit_push_int(4);
        builder.emit_opcode(OpCode::PACK);
        builder.emit_push_int(CallFlags::ALL.bits() as i64);
        builder.emit_push(b"transfer");
        builder.emit_push(&GasToken::new().hash().to_bytes());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("syscall");
        builder.emit_opcode(OpCode::ASSERT);

        let mut transaction = Transaction::new();
        transaction.set_nonce(1);
        transaction.set_system_fee(TEST_MODE_GAS);
        transaction.set_network_fee(0);
        transaction.set_valid_until_block(100);
        transaction.set_signers(vec![Signer::new(witness, WitnessScope::CALLED_BY_ENTRY)]);
        transaction.set_witnesses(vec![Witness::new()]);
        transaction.set_script(builder.to_array());

        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(system.genesis_block().hash());
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(1);
        header.witness = Witness::new();
        let mut block = neo_protocol::payloads::Block {
            header,
            transactions: vec![transaction],
        };
        block.rebuild_merkle_root();
        block
    }

    fn execution_observation(executions: &[ApplicationExecuted]) -> Vec<String> {
        executions
            .iter()
            .map(|execution| {
                format!(
                    "{:?}|{:?}|{}|{:?}|{:?}|{:?}|{:?}",
                    execution.trigger,
                    execution.vm_state,
                    execution.gas_consumed,
                    execution.exception,
                    execution.stack,
                    execution.notifications,
                    execution.logs
                )
            })
            .collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn block_level_transfer_modes_match_and_faults_roll_back() {
        let settings = ProtocolSettings::testnet();
        let sender = NativeHelpers::get_bft_address(&settings.standby_validators());
        let recipient = UInt160::from_bytes(&[0x42; 20]).expect("recipient");
        let stranger = UInt160::from_bytes(&[0x24; 20]).expect("stranger");
        let mut expected_success = None;
        let mut expected_faults = None;

        for mode in [
            ExecutionMode::Canonical,
            ExecutionMode::Auto,
            ExecutionMode::Verify,
        ] {
            let system = NeoSystem::new(settings.clone(), None, None).expect("system");
            let block = transfer_block(&system, sender, recipient, 1_000, sender);
            let before = GasToken::new().balance_of_snapshot(&system.store_cache(), &sender);
            let executions = system
                .persist_block_with_receipts_and_execution_mode(Block::from(&block), mode)
                .expect("successful transfer");
            let success = execution_observation(&executions);
            assert_eq!(executions[1].vm_state, VmState::HALT);
            assert_eq!(
                GasToken::new().balance_of_snapshot(&system.store_cache(), &recipient),
                BigInt::from(1_000)
            );
            assert_eq!(
                before - GasToken::new().balance_of_snapshot(&system.store_cache(), &sender),
                BigInt::from(TEST_MODE_GAS + 1_000)
            );
            if let Some(expected) = &expected_success {
                assert_eq!(expected, &success, "success differs in {}", mode.as_str());
            } else {
                expected_success = Some(success);
            }

            let sender_after_success =
                GasToken::new().balance_of_snapshot(&system.store_cache(), &sender);
            let mut fault_observations = Vec::new();
            let mut previous_hash = block.hash().expect("block hash");
            for (offset, (from, amount)) in
                [(sender, i64::MAX), (stranger, 1)].into_iter().enumerate()
            {
                // The fee payer remains funded; the second case faults because
                // the transfer source is not a transaction witness.
                let mut fault = transfer_block(&system, from, recipient, amount, sender);
                fault.header.set_index(2 + offset as u32);
                fault.header.set_prev_hash(previous_hash);
                fault.rebuild_merkle_root();
                previous_hash = fault.hash().expect("fault block hash");
                let executions = system
                    .persist_block_with_receipts_and_execution_mode(Block::from(&fault), mode)
                    .expect("faulting transactions are persisted with rollback");
                assert_eq!(executions[1].vm_state, VmState::FAULT);
                fault_observations.push(execution_observation(&executions));
            }
            // Fault rolls back token movements, not the canonical block fee burn.
            assert_eq!(
                sender_after_success
                    - GasToken::new().balance_of_snapshot(&system.store_cache(), &sender),
                BigInt::from(2 * TEST_MODE_GAS)
            );
            assert_eq!(
                GasToken::new().balance_of_snapshot(&system.store_cache(), &recipient),
                BigInt::from(1_000)
            );
            if let Some(expected) = &expected_faults {
                assert_eq!(
                    expected,
                    &fault_observations,
                    "fault differs in {}",
                    mode.as_str()
                );
            } else {
                expected_faults = Some(fault_observations);
            }
            system.shutdown().await.expect("shutdown");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn consecutive_empty_blocks_keep_native_pipeline_across_committee_refresh() {
        let settings = ProtocolSettings::testnet();
        let block_count = 2 * settings.committee_members_count() as u32 + 2;
        let system = NeoSystem::new(settings, None, None).expect("system");
        let mut previous_hash = system.genesis_block().hash();

        for index in 1..=block_count {
            let mut header = Header::new();
            header.set_index(index);
            header.set_prev_hash(previous_hash);
            header.set_next_consensus(UInt160::zero());
            header.set_timestamp(u64::from(index));
            header.witness = Witness::new();
            let payload = neo_protocol::payloads::Block {
                header,
                transactions: Vec::new(),
            };
            previous_hash = payload.hash().expect("empty block hash");
            let executions = system
                .persist_block_with_receipts_and_execution_mode(
                    Block::from(&payload),
                    ExecutionMode::Auto,
                )
                .expect("empty block");
            assert_eq!(executions.len(), 2, "OnPersist/PostPersist must both run");
            assert!(
                executions
                    .iter()
                    .all(|execution| execution.vm_state == VmState::HALT)
            );
            assert_eq!(system.current_block_index(), index);
        }
        system.shutdown().await.expect("shutdown");
    }

    #[test]
    fn execution_recorder_is_empty_before_capture() {
        let recorder = ExecutionRecorder::default();
        assert!(recorder.snapshot().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn persisted_store_rehydrates_ledger_tip_and_native_contract_cache() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let genesis_hash = system.genesis_block().hash();
        let store_cache = system.store_cache();

        // Use a fresh registry to prove that hydration restores persisted native
        // metadata rather than observing cache entries created during execution.
        let native_services = default_native_services();
        let management = native_services
            .registry
            .get(&ContractManagement::contract_hash())
            .expect("ContractManagement in registry");
        let management = management
            .as_any()
            .downcast_ref::<ContractManagement>()
            .expect("concrete ContractManagement");
        let ledger_hash = LedgerContract::new().hash();
        assert!(management.get_contract(&ledger_hash).unwrap().is_none());
        crate::neo_system::storage::hydrate_native_contracts(
            &store_cache,
            &native_services,
            system.settings(),
        )
        .expect("hydrate nonempty native storage");
        assert!(management.get_contract(&ledger_hash).unwrap().is_some());

        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(genesis_hash);
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(1);
        header.witness = Witness::new();
        let payload = neo_protocol::payloads::Block {
            header,
            transactions: Vec::new(),
        };
        let block = Block::from(&payload);
        let block_hash = block.hash();
        system.persist_block(block).expect("persist block");

        let store_cache = system.store_cache();
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        crate::neo_system::storage::hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect("rehydrate persisted chain");
        assert_eq!(ledger.current_height(), 1);
        assert_eq!(ledger.block_hash_at(0), Some(genesis_hash));
        assert_eq!(ledger.block_hash_at(1), Some(block_hash));
        assert_eq!(header_cache.count(), 2);
        system.shutdown().await.expect("shutdown");
    }
}
