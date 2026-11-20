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
    SnapshotStore, SpeculativeExecution, StoreTransactionPort, TransactionLoopStrategy,
};
use neo_primitives::{CallFlags, TriggerType, UInt160, UInt256, Verifiable};
use neo_protocol::execution::{ApplicationExecuted, Block};
use neo_smart_contract::application_engine::execution_strategy::{
    CanonicalVerificationSource, ExecutionMode, ExecutionReport, ExecutionRequest,
    ExecutionStrategy, ExecutionStrategyObserver, TransferParams, execute_application_script,
    parse_fast_path_candidate,
};
use neo_smart_contract::application_engine::{
    ApplicationEngine, EngineNativeServices, LedgerTransactionStates, TEST_MODE_GAS,
};
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::persistence::data_cache::DataCacheConfig;
use neo_storage::persistence::{
    PersistenceDataCache, StoreCommitProfiler, StoreCommitStage, StoreTransaction, store::Store,
};
use neo_vm::VmState;
use parking_lot::Mutex;

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

/// How the profiled persistence stages nest inside one another.
///
/// Every stage in [`ExecutionProfileSnapshot`] is measured where the pipeline
/// already does the work; several of them therefore overlap. Consumers that
/// publish these timings (for example the offline `.acc` import summary) must
/// label the nesting instead of adding stages together, because a sum would
/// count the same wall-clock time more than once:
///
/// - `transaction_vm` runs inside `transaction_loop`;
/// - `state_change_extraction` and `mpt` run inside `participant_precommit`
///   (the state-root staging path);
/// - `store_database_write` runs inside `store_commit` (the chain commit).
///
/// All requested visibilities already exist as stages; no additional timers are
/// needed to observe snapshot creation, OnPersist, the transaction loop,
/// PostPersist, state-root staging, or the chain commit.
pub const PERSISTENCE_STAGE_NESTING: &str = "transaction_vm<transaction_loop; state_change_extraction+mpt<participant_precommit; \
     store_database_write<store_commit";

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

/// Observation-only projection of the fast-path counters.
///
/// Every field counts events the execution strategy already decided; reading it
/// never changes an execution result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FastPathStatsSnapshot {
    /// Direct native dispatches that actually ran.
    pub direct_attempts: u64,
    /// Direct native dispatches that ended in `HALT`.
    pub direct_halts: u64,
    /// Direct native dispatches that ended in a canonical `FAULT`.
    pub direct_faults: u64,
    /// Parsed transfers rejected by the engine-side eligibility checks.
    pub eligibility_declines: u64,
    /// Verify-mode executions that fell back to the canonical interpreter.
    pub canonical_fallbacks: u64,
    /// Dual executions whose fingerprints diverged.
    pub verification_mismatches: u64,
}

/// Observation-only projection of the speculative scheduler counters.
///
/// Every field counts something the scheduler already decided. Reading the
/// snapshot never changes an execution result, and none of these values reach
/// consensus state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParallelExecutionSnapshot {
    /// Blocks whose transactions were executed speculatively before validation.
    pub speculative_blocks: u64,
    /// Transactions executed speculatively (including later discarded runs).
    pub speculative_transactions: u64,
    /// Speculative runs rejected by the ordered validation.
    pub conflicts: u64,
    /// Transactions re-executed serially against the committed state.
    pub reexecutions: u64,
    /// Blocks that were routed to the serial loop (empty, single, or fallback).
    pub serial_fallbacks: u64,
    /// Wall-clock nanoseconds spent inside the speculative scheduler.
    pub parallel_nanos: u64,
    /// Wall-clock nanoseconds spent inside the serial loop while parallel was enabled.
    pub serial_nanos: u64,
}

/// Process-local execution counters. These are observability data, never consensus state.
#[derive(Debug)]
pub struct ExecutionStats {
    direct_transfers: AtomicU64,
    fast_path_declines: AtomicU64,
    direct_halts: AtomicU64,
    direct_faults: AtomicU64,
    canonical_fallbacks: AtomicU64,
    verification_mismatches: AtomicU64,
    speculative_blocks: AtomicU64,
    speculative_transactions: AtomicU64,
    speculative_conflicts: AtomicU64,
    speculative_reexecutions: AtomicU64,
    speculative_serial_fallbacks: AtomicU64,
    speculative_parallel_nanos: AtomicU64,
    speculative_serial_nanos: AtomicU64,
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
            fast_path_declines: AtomicU64::default(),
            direct_halts: AtomicU64::default(),
            direct_faults: AtomicU64::default(),
            canonical_fallbacks: AtomicU64::default(),
            verification_mismatches: AtomicU64::default(),
            speculative_blocks: AtomicU64::default(),
            speculative_transactions: AtomicU64::default(),
            speculative_conflicts: AtomicU64::default(),
            speculative_reexecutions: AtomicU64::default(),
            speculative_serial_fallbacks: AtomicU64::default(),
            speculative_parallel_nanos: AtomicU64::default(),
            speculative_serial_nanos: AtomicU64::default(),
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
    /// Number of transactions dispatched by direct native NEP-17 dispatch.
    ///
    /// Attempts that faulted are included: they still ran the fast path and
    /// produced a receipt. Transfers declined by eligibility checks are not.
    pub fn direct_transfer_count(&self) -> u64 {
        self.direct_transfers.load(Ordering::Relaxed)
    }

    /// Returns a consistent-enough snapshot of the fast-path counters.
    ///
    /// This is observability data only; nothing in the consensus path reads it.
    pub fn fast_path_snapshot(&self) -> FastPathStatsSnapshot {
        FastPathStatsSnapshot {
            direct_attempts: self.direct_transfers.load(Ordering::Relaxed),
            direct_halts: self.direct_halts.load(Ordering::Relaxed),
            direct_faults: self.direct_faults.load(Ordering::Relaxed),
            eligibility_declines: self.fast_path_declines.load(Ordering::Relaxed),
            canonical_fallbacks: self.canonical_fallbacks.load(Ordering::Relaxed),
            verification_mismatches: self.verification_mismatches.load(Ordering::Relaxed),
        }
    }

    /// Returns a consistent-enough snapshot of the speculative scheduler counters.
    ///
    /// This is observability data only; nothing in the consensus path reads it.
    pub fn parallel_snapshot(&self) -> ParallelExecutionSnapshot {
        ParallelExecutionSnapshot {
            speculative_blocks: self.speculative_blocks.load(Ordering::Relaxed),
            speculative_transactions: self.speculative_transactions.load(Ordering::Relaxed),
            conflicts: self.speculative_conflicts.load(Ordering::Relaxed),
            reexecutions: self.speculative_reexecutions.load(Ordering::Relaxed),
            serial_fallbacks: self.speculative_serial_fallbacks.load(Ordering::Relaxed),
            parallel_nanos: self.speculative_parallel_nanos.load(Ordering::Relaxed),
            serial_nanos: self.speculative_serial_nanos.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_speculative_block(&self) {
        self.speculative_blocks.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_speculative_transactions(&self, count: u64) {
        self.speculative_transactions
            .fetch_add(count, Ordering::Relaxed);
    }

    pub(crate) fn record_speculative_conflict(&self) {
        self.speculative_conflicts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_speculative_reexecution(&self) {
        self.speculative_reexecutions
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_serial_fallback(&self) {
        self.speculative_serial_fallbacks
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_parallel_elapsed(&self, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.speculative_parallel_nanos
            .fetch_add(nanos, Ordering::Relaxed);
    }

    pub(crate) fn record_serial_elapsed(&self, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.speculative_serial_nanos
            .fetch_add(nanos, Ordering::Relaxed);
    }

    /// Classifies one finished application execution into the fast-path counters.
    ///
    /// `strategy` names the interpreter that actually ran and `verification_error`
    /// carries the reason recorded by the strategy layer, so every bucket here is
    /// derived from a fact the execution already produced.
    fn record_strategy_outcome(
        &self,
        strategy: ExecutionStrategy,
        state: VmState,
        verification_error: Option<&str>,
    ) {
        match strategy {
            ExecutionStrategy::DirectTransfer => {
                match state {
                    VmState::HALT => {
                        self.direct_halts.fetch_add(1, Ordering::Relaxed);
                    }
                    VmState::FAULT => {
                        self.direct_faults.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
                if verification_error.is_some() {
                    self.verification_mismatches.fetch_add(1, Ordering::Relaxed);
                }
            }
            ExecutionStrategy::Canonical => {
                // A canonical run that recorded a reason is the verify-mode
                // fallback: the direct path was skipped because the isolated
                // comparison context was unavailable.
                if verification_error.is_some() {
                    self.canonical_fallbacks.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
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

    fn bump_direct_transfer_attempt(&self) {
        self.direct_transfers.fetch_add(1, Ordering::Relaxed);
    }
}

impl ExecutionStrategyObserver for ExecutionStats {
    fn record_direct_transfer_attempt(&self) {
        self.bump_direct_transfer_attempt();
    }

    fn record_fast_path_decline(&self) {
        self.fast_path_declines.fetch_add(1, Ordering::Relaxed);
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
/// `PersistenceDataCache::commit` to retain visible values as hot `None` entries. No database
/// write occurs until `finish`, which applies the coalesced final state in one
/// physical transaction. Dropping this object before `finish` writes nothing.
struct EmptyBlockWriteBack {
    transaction: Mutex<Option<StoreTransaction>>,
    base_snapshot: Arc<PersistenceDataCache>,
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
    base_snapshot: Arc<PersistenceDataCache>,
    empty_block_batch: Option<Arc<EmptyBlockWriteBack>>,
}

impl StoreTransactionPort for SystemStoreTransaction {
    fn base_snapshot(&self) -> Arc<PersistenceDataCache> {
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

    /// Projects the receipt the canonical serial loop would report for `engine`.
    ///
    /// This performs no bookkeeping: it neither marks the transaction state nor
    /// publishes an observation, so a speculative run can ask for the receipt
    /// before its commit order is known.
    fn receipt(engine: &mut ApplicationEngine) -> ExecutionReceipt {
        let transaction_hash = engine
            .script_container()
            .and_then(|container| {
                container
                    .as_any()
                    .downcast_ref::<neo_protocol::payloads::Transaction>()
                    .cloned()
            })
            .as_ref()
            .map(neo_protocol::payloads::Transaction::hash);
        ExecutionReceipt {
            trigger: engine.trigger(),
            transaction_hash,
            vm_state: engine.state(),
            gas_consumed: engine.fee_consumed(),
            exception: engine.fault_exception().map(str::to_owned),
        }
    }

    /// Builds the observation record for `engine` and marks the transaction state.
    ///
    /// The returned record is *not* published; callers either [`Self::publish`] it
    /// immediately (serial path) or hold it until the ordered validation releases it
    /// (speculative path), which is what keeps observation order equal to block order.
    fn project(&self, engine: &mut ApplicationEngine) -> (ApplicationExecuted, ExecutionReceipt) {
        let receipt = Self::receipt(engine);
        if let Some(hash) = receipt.transaction_hash {
            let state = engine.state();
            engine.record_transaction_vm_state(&hash, state);
        }
        let transaction = engine.script_container().and_then(|container| {
            container
                .as_any()
                .downcast_ref::<neo_protocol::payloads::Transaction>()
                .cloned()
        });
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
        (executed, receipt)
    }

    /// Releases one observation record to the event stream and the block snapshot.
    fn publish(&self, executed: ApplicationExecuted) {
        if let Some(event_stream) = self.event_stream.lock().as_ref() {
            event_stream.publish(executed.clone());
        }
        self.executions.lock().push(executed);
    }

    fn capture(&self, engine: &mut ApplicationEngine) -> ExecutionReceipt {
        let (executed, receipt) = self.project(engine);
        self.publish(executed);
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
            pending_capture: None,
        }
    }
}

impl EngineFactory for SystemEngineFactory {
    fn open_on_persist(
        &self,
        snapshot: Arc<PersistenceDataCache>,
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
        snapshot: Arc<PersistenceDataCache>,
        block: Arc<Block>,
        system_fee: i64,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError> {
        let payload_block = self.payload_block(&block);
        // Only `verify` dual-executes, so only it pays for an isolated source.
        let verification =
            self.execution_mode
                .requires_isolated_source()
                .then(|| VerificationContext {
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
        snapshot: Arc<PersistenceDataCache>,
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
    snapshot: Arc<PersistenceDataCache>,
    payload_block: Arc<neo_protocol::payloads::Block>,
    system_fee: i64,
    seeded: SeededNativeContracts,
    cache: NativeCacheHandle,
    settings: neo_protocol::ProtocolSettings,
    native_services: EngineNativeServices,
}

impl CanonicalVerificationSource for VerificationContext {
    fn execute_canonical(&self, script: &[u8]) -> Result<ApplicationEngine, String> {
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
            .load_script(script.to_vec(), CallFlags::ALL, None)
            .map_err(|error| error.to_string())?;
        engine.execute_allow_fault();
        Ok(engine)
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
    /// Observation record produced by a speculative run and held back until the
    /// ordered validation decides the transaction's position in commit order.
    pending_capture: Option<ApplicationExecuted>,
}

impl SystemEnginePort {
    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        self.engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()))
    }

    /// Delegates the execution-strategy decision to the smart-contract layer
    /// and retains only the failure surfaced by dual execution.
    fn execute_application(&mut self) -> VmState {
        let request = ExecutionRequest {
            mode: self.execution_mode,
            transfer: self.transfer.as_ref(),
            script: self.script.as_deref(),
            canonical_source: self
                .verification
                .as_ref()
                .map(|source| source as &dyn CanonicalVerificationSource),
            observer: Some(self.stats.as_ref() as &dyn ExecutionStrategyObserver),
        };
        let ExecutionReport {
            state,
            strategy,
            verification_error,
        } = execute_application_script(&mut self.engine, &request);
        self.stats
            .record_strategy_outcome(strategy, state, verification_error.as_deref());
        if let Some(message) = verification_error {
            self.verification_error = Some(message);
        }
        state
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
        self.transfer = parse_fast_path_candidate(&script);
        self.script = Some(script.clone());
        self.verification_error = None;
        self.engine
            .load_script(script, CallFlags::ALL, None)
            .map_err(engine_error)
    }

    fn execute_allow_fault(&mut self) -> VmState {
        self.execute_application()
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

    fn enable_rw_set_tracking_at_generation(&mut self, generation: u64) {
        self.engine.enable_rw_set_tracking_at_generation(generation);
    }

    fn capture_speculative(&mut self, detailed: bool) -> Option<SpeculativeExecution> {
        let rw_set = self.engine.take_rw_set()?;
        let receipt = if detailed {
            let (executed, receipt) = self.recorder.project(&mut self.engine);
            self.pending_capture = Some(executed);
            receipt
        } else {
            ExecutionRecorder::receipt(&mut self.engine)
        };
        Some(SpeculativeExecution { receipt, rw_set })
    }

    fn publish_capture(&mut self) {
        if let Some(executed) = self.pending_capture.take() {
            self.recorder.publish(executed);
        }
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
    ///
    /// This method enforces that requirement itself instead of leaving it to callers.
    /// Once a batch has started, the first failure poisons this instance: the
    /// process-local tip, state root and callback state may be ahead of durable
    /// storage, so every later persistence call (including this one) is rejected with
    /// a poison error until the instance is dropped and storage is reopened.
    pub fn persist_consecutive_empty_blocks_without_runtime_cache(
        &self,
        mut blocks: Vec<neo_protocol::payloads::Block>,
    ) -> Result<Vec<Vec<ApplicationExecuted>>, String> {
        self.ensure_persistence_live()?;
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
        // Every step below may advance process-local state before the batch's
        // physical commit, so any failure must poison this instance.
        let result = (|| {
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
        })();
        if result.is_err() {
            self.poison_persistence();
        }
        result
    }

    fn persist_block_internal(
        &self,
        block: Block,
        update_runtime_cache: bool,
        execution_mode: ExecutionMode,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        self.ensure_persistence_live()?;
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
        let parallel_loop = self
            .is_parallel_execution_enabled()
            .then(|| super::ParallelTransactionLoop::new(Arc::clone(&self.execution_stats)));
        neo_ledger::persist_block_with_loop_strategy(
            block,
            store,
            &engines,
            &runtime_cache,
            &evidence,
            &participants,
            &options,
            profiler,
            parallel_loop
                .as_ref()
                .map(|scheduler| scheduler as &dyn TransactionLoopStrategy),
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

/// Attaches or detaches the state-root timing sink that feeds MPT stage timings
/// into [`ExecutionStats`].
///
/// Observation only: the sink copies durations the commit pipeline already
/// measured and the statistics only record them while profiling is enabled.
/// Installing or removing it cannot change an execution result, consensus
/// decision, or commit ordering.
pub(crate) fn install_state_root_profile_sink(
    handlers: &crate::state_service::StateServiceCommitHandlers,
    stats: &Arc<ExecutionStats>,
    enabled: bool,
) {
    if !enabled {
        handlers.set_profile_sink(None);
        return;
    }
    let stats = Arc::clone(stats);
    let sink: crate::state_service::StateRootProfileSink = Arc::new(move |profile| {
        stats.record_state_root_profile(profile.change_extraction, profile.mpt, profile.mpt_detail);
    });
    handlers.set_profile_sink(Some(sink));
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
    use neo_storage::error::{StorageError, StorageResult};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{
        ReadOnlyStore, ReadOnlyStoreGeneric, SeekDirection, StoreSnapshot, WriteStore,
        store::OnNewSnapshotDelegate, store_provider::StoreProvider,
    };
    use neo_storage::{StorageItem, StorageKey};
    use neo_vm::{OpCode, ScriptBuilder};
    use num_bigint::BigInt;
    use std::any::Any;

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

    /// A memory store whose snapshots can fail the physical commit on demand.
    ///
    /// Clones share the underlying map and the failure switch, so every transaction
    /// opened through this store observes the same commit behaviour.
    #[derive(Clone)]
    struct FlakyStore {
        inner: MemoryStore,
        fail_commits: Arc<AtomicBool>,
    }

    struct FlakyStoreProvider {
        store: FlakyStore,
    }

    impl StoreProvider for FlakyStoreProvider {
        fn name(&self) -> &str {
            "Flaky"
        }

        fn get_store(&self, _path: &str) -> StorageResult<Arc<dyn Store>> {
            Ok(Arc::new(self.store.clone()))
        }
    }

    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FlakyStore {
        fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
            self.inner.try_get(key)
        }

        fn find(
            &self,
            key_prefix: Option<&StorageKey>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            self.inner.find(key_prefix, direction)
        }
    }

    impl ReadOnlyStore for FlakyStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for FlakyStore {
        fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
            self.inner.delete(key)
        }

        fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
            self.inner.put(key, value)
        }
    }

    impl Store for FlakyStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            Arc::new(FlakySnapshot {
                inner: self.inner.snapshot(),
                store: self.clone(),
            })
        }

        fn on_new_snapshot(&self, _handler: OnNewSnapshotDelegate) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FlakySnapshot {
        inner: Arc<dyn StoreSnapshot>,
        store: FlakyStore,
    }

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FlakySnapshot {
        fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
            self.inner.try_get(key)
        }

        fn find(
            &self,
            key_prefix: Option<&Vec<u8>>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            self.inner.find(key_prefix, direction)
        }
    }

    impl WriteStore<Vec<u8>, Vec<u8>> for FlakySnapshot {
        fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
            Arc::get_mut(&mut self.inner)
                .ok_or_else(shared_snapshot_error)?
                .delete(key)
        }

        fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
            Arc::get_mut(&mut self.inner)
                .ok_or_else(shared_snapshot_error)?
                .put(key, value)
        }
    }

    /// Each test snapshot is created and consumed by a single writer; a shared
    /// handle means the fixture was misused, so fail loudly instead.
    fn shared_snapshot_error() -> StorageError {
        StorageError::InvalidOperation {
            message: "flaky test snapshot is shared".to_string(),
        }
    }

    impl StoreSnapshot for FlakySnapshot {
        fn store(&self) -> Arc<dyn Store> {
            Arc::new(self.store.clone())
        }

        fn try_commit(&mut self) -> Result<(), StorageError> {
            if self.store.fail_commits.load(Ordering::SeqCst) {
                return Err(StorageError::InvalidOperation {
                    message: "injected physical commit failure".to_string(),
                });
            }
            Arc::get_mut(&mut self.inner)
                .ok_or_else(shared_snapshot_error)?
                .try_commit()
        }
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
            // The direct-dispatch counter now crosses the strategy port, so a
            // miswired observer would silently stop reporting here.
            match mode {
                ExecutionMode::Canonical => assert_eq!(
                    system.execution_stats().direct_transfer_count(),
                    0,
                    "canonical mode must not report a direct dispatch"
                ),
                ExecutionMode::Auto | ExecutionMode::Verify => assert!(
                    system.execution_stats().direct_transfer_count() > 0,
                    "{} must report a direct dispatch",
                    mode.as_str()
                ),
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

    /// Builds an empty single-witness block on top of `prev_hash`.
    fn empty_block_at(
        index: u32,
        prev_hash: neo_primitives::UInt256,
    ) -> neo_protocol::payloads::Block {
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(prev_hash);
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(u64::from(index));
        header.witness = Witness::new();
        neo_protocol::payloads::Block {
            header,
            transactions: Vec::new(),
        }
    }

    /// State-service settings that persist a state root for every height.
    fn state_service_settings(
        settings: &ProtocolSettings,
    ) -> crate::state_service::StateServiceSettings {
        crate::state_service::StateServiceSettings {
            full_state: false,
            path: String::new(),
            network: settings.network,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: crate::state_service::UnhandledExceptionPolicy::Ignore,
        }
    }

    /// Empty-block differential across execution engines: for one identical empty
    /// block, every mode must produce the same receipts, ledger pointer and state
    /// root. This pins the canonical empty-block outcome the hook fast paths must
    /// keep reproducing.
    #[tokio::test(flavor = "multi_thread")]
    async fn empty_block_receipts_roots_and_pointers_match_across_execution_modes() {
        let settings = ProtocolSettings::testnet();
        let mut expected: Option<(
            Vec<String>,
            neo_primitives::UInt256,
            neo_primitives::UInt256,
        )> = None;

        for mode in [
            ExecutionMode::Canonical,
            ExecutionMode::Auto,
            ExecutionMode::Verify,
        ] {
            let system = NeoSystem::new_with_state_service(
                settings.clone(),
                None,
                None,
                Some(state_service_settings(&settings)),
            )
            .expect("system");
            let block = empty_block_at(1, system.genesis_block().hash());
            let block_hash = block.hash().expect("empty block hash");

            let executions = system
                .persist_block_with_receipts_and_execution_mode(Block::from(&block), mode)
                .expect("empty block");

            assert_eq!(
                executions.len(),
                2,
                "OnPersist/PostPersist must both run in {}",
                mode.as_str()
            );
            assert!(
                executions
                    .iter()
                    .all(|execution| execution.vm_state == VmState::HALT),
                "empty block faulted in {}",
                mode.as_str()
            );
            assert!(
                executions[0].notifications.is_empty(),
                "an empty OnPersist must not notify in {}",
                mode.as_str()
            );

            let ledger = LedgerContract::new();
            let store_cache = system.store_cache();
            assert_eq!(ledger.current_index(&store_cache).expect("ledger index"), 1);
            assert_eq!(
                ledger.current_hash(&store_cache).expect("ledger hash"),
                block_hash
            );

            let handlers = system.state_commit_handlers().expect("state handlers");
            let root = handlers
                .get_state_root(1)
                .expect("a state root record per height");
            assert_eq!(root.index, 1);
            assert_eq!(
                handlers.current_root_hash(),
                Some(root.root_hash),
                "process-local root must match the persisted root in {}",
                mode.as_str()
            );

            let observation = (
                execution_observation(&executions),
                block_hash,
                root.root_hash,
            );
            match &expected {
                Some(expected) => assert_eq!(
                    expected,
                    &observation,
                    "empty block differs in {}",
                    mode.as_str()
                ),
                None => expected = Some(observation),
            }

            system.shutdown().await.expect("shutdown");
        }
    }

    /// Consecutive-empty-block regression: the optimized hooks must not remove any
    /// per-height transition. Every empty block still runs both persist triggers,
    /// advances the ledger pointer, persists a state-root record, and NEO still
    /// refreshes the committee at the committee-size boundary.
    #[tokio::test(flavor = "multi_thread")]
    async fn consecutive_empty_blocks_keep_height_dependent_transitions() {
        let settings = ProtocolSettings::testnet();
        let committee = u32::try_from(settings.committee_members_count()).expect("committee size");
        let block_count = 2 * committee + 1;
        let system = NeoSystem::new_with_state_service(
            settings.clone(),
            None,
            None,
            Some(state_service_settings(&settings)),
        )
        .expect("system");
        let handlers = system.state_commit_handlers().expect("state handlers");
        let neo = neo_native_contracts::NeoToken::new();
        let mut previous_hash = system.genesis_block().hash();
        let mut refreshed_committee: Option<Vec<Vec<u8>>> = None;
        let mut refresh_heights = Vec::new();

        for index in 1..=block_count {
            let payload = empty_block_at(index, previous_hash);
            previous_hash = payload.hash().expect("empty block hash");
            let executions = system
                .persist_block_with_receipts_and_execution_mode(
                    Block::from(&payload),
                    ExecutionMode::Auto,
                )
                .expect("empty block");

            assert_eq!(
                executions.len(),
                2,
                "OnPersist/PostPersist must both run at height {index}"
            );
            assert!(
                executions
                    .iter()
                    .all(|execution| execution.vm_state == VmState::HALT),
                "empty block faulted at height {index}"
            );
            // No fee burn, no notary or oracle reward, no token transfer: an
            // empty block's OnPersist emits no notification.
            assert!(
                executions[0].notifications.is_empty(),
                "empty OnPersist emitted a notification at height {index}"
            );

            // Ledger pointer transition for every height.
            assert_eq!(system.current_block_index(), index);
            let ledger = LedgerContract::new();
            let store_cache = system.store_cache();
            assert_eq!(
                ledger.current_index(&store_cache).expect("ledger index"),
                index,
                "ledger index must follow the tip at height {index}"
            );
            assert_eq!(
                ledger.current_hash(&store_cache).expect("ledger hash"),
                previous_hash,
                "ledger hash must follow the tip at height {index}"
            );

            // A state-root record is persisted for every height, even when the
            // root value is unchanged because the block had no transactions.
            let root = handlers
                .get_state_root(index)
                .unwrap_or_else(|| panic!("no state root record at height {index}"));
            assert_eq!(root.index, index);
            assert_eq!(handlers.current_root_hash(), Some(root.root_hash));

            // NEO committee state across the committee-size boundary. Genesis
            // initialises it and every refresh height rewrites it, so on an empty
            // chain it must stay present, non-empty and drift-free.
            let committee_now = neo
                .committee_from_snapshot(&store_cache)
                .map(|keys| {
                    keys.iter()
                        .map(|key| key.as_bytes().to_vec())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| panic!("committee missing at height {index}"));
            assert!(
                !committee_now.is_empty(),
                "committee empty at height {index}"
            );
            match &refreshed_committee {
                Some(expected) => assert_eq!(
                    expected, &committee_now,
                    "committee drifted at height {index}"
                ),
                None => refreshed_committee = Some(committee_now),
            }
            if index % committee == 0 {
                refresh_heights.push(index);
            }
        }

        assert_eq!(
            refresh_heights,
            vec![committee, 2 * committee],
            "committee refresh heights must stay on the committee-size boundary"
        );
        assert_eq!(system.current_block_index(), block_count);
        system.shutdown().await.expect("shutdown");
    }

    #[test]
    fn execution_recorder_is_empty_before_capture() {
        let recorder = ExecutionRecorder::default();
        assert!(recorder.snapshot().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_block_batch_physical_commit_failure_poisons_system() {
        let settings = ProtocolSettings::default();
        let fail_commits = Arc::new(AtomicBool::new(false));
        let provider: Arc<dyn StoreProvider> = Arc::new(FlakyStoreProvider {
            store: FlakyStore {
                inner: MemoryStore::new(),
                fail_commits: Arc::clone(&fail_commits),
            },
        });
        // Genesis is persisted while the store is healthy, so only the batch's
        // single physical commit fails.
        let system = NeoSystem::new(settings, Some(provider), None).expect("system");
        assert!(!system.is_persistence_poisoned());

        let mut blocks = Vec::new();
        let mut previous_hash = system.genesis_block().hash();
        for index in 1..=2u32 {
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
            blocks.push(payload);
        }

        fail_commits.store(true, Ordering::SeqCst);
        let error = expect_err(
            system.persist_consecutive_empty_blocks_without_runtime_cache(blocks.clone()),
        );
        assert!(
            error.contains("injected physical commit failure"),
            "unexpected error: {error}"
        );
        assert!(
            system.is_persistence_poisoned(),
            "a failed batch commit must poison the system"
        );

        let error = expect_err(system.persist_block(Block::from(&blocks[0])));
        assert!(
            error.to_lowercase().contains("poisoned"),
            "unexpected error: {error}"
        );
        let error =
            expect_err(system.persist_consecutive_empty_blocks_without_runtime_cache(blocks));
        assert!(
            error.to_lowercase().contains("poisoned"),
            "unexpected error: {error}"
        );
        system.shutdown().await.expect("shutdown");
    }

    /// Unwraps a persistence error without requiring `Debug` on the success value.
    fn expect_err<T>(result: Result<T, String>) -> String {
        match result {
            Ok(_) => panic!("expected a persistence error"),
            Err(error) => error,
        }
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
        neo_ledger::hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect("rehydrate persisted chain");
        assert_eq!(ledger.current_height(), 1);
        assert_eq!(ledger.block_hash_at(0), Some(genesis_hash));
        assert_eq!(ledger.block_hash_at(1), Some(block_hash));
        assert_eq!(header_cache.count(), 2);
        system.shutdown().await.expect("shutdown");
    }

    #[test]
    fn fast_path_counters_classify_each_outcome_once() {
        let stats = ExecutionStats::default();

        stats.record_strategy_outcome(ExecutionStrategy::DirectTransfer, VmState::HALT, None);
        stats.record_strategy_outcome(ExecutionStrategy::DirectTransfer, VmState::FAULT, None);
        stats.record_strategy_outcome(
            ExecutionStrategy::DirectTransfer,
            VmState::HALT,
            Some("direct fingerprint diverged"),
        );
        stats.record_strategy_outcome(
            ExecutionStrategy::Canonical,
            VmState::HALT,
            Some("missing isolated verification context"),
        );
        // A canonical run without a recorded reason is the configured canonical
        // mode, not a fallback.
        stats.record_strategy_outcome(ExecutionStrategy::Canonical, VmState::HALT, None);

        let snapshot = stats.fast_path_snapshot();
        assert_eq!(snapshot.direct_halts, 2);
        assert_eq!(snapshot.direct_faults, 1);
        assert_eq!(snapshot.canonical_fallbacks, 1);
        assert_eq!(snapshot.verification_mismatches, 1);
        // Outcome classification never invents direct attempts; the strategy
        // layer reports those before the result is known.
        assert_eq!(snapshot.direct_attempts, 0);
    }

    #[test]
    fn fast_path_declines_are_counted_from_the_observer_port() {
        let stats = ExecutionStats::default();

        stats.record_fast_path_decline();
        stats.record_fast_path_decline();
        stats.record_direct_transfer_attempt();

        let snapshot = stats.fast_path_snapshot();
        assert_eq!(snapshot.eligibility_declines, 2);
        assert_eq!(snapshot.direct_attempts, 1);
        assert_eq!(stats.direct_transfer_count(), 1);
    }

    #[test]
    fn profiling_disabled_keeps_mpt_timings_at_zero() {
        let stats = ExecutionStats::default();
        stats.record_state_root_profile(
            Duration::from_millis(10),
            Duration::from_millis(20),
            MptProfileSnapshot {
                update: Duration::from_millis(5),
                hash: Duration::from_millis(2),
                ..MptProfileSnapshot::default()
            },
        );

        let profile = stats.profile_snapshot();
        assert!(!profile.enabled);
        assert_eq!(profile.mpt, Duration::ZERO);
        assert_eq!(profile.mpt_update, Duration::ZERO);

        stats.set_profiling(true);
        stats.record_state_root_profile(
            Duration::from_millis(10),
            Duration::from_millis(20),
            MptProfileSnapshot {
                update: Duration::from_millis(5),
                hash: Duration::from_millis(2),
                ..MptProfileSnapshot::default()
            },
        );

        let profile = stats.profile_snapshot();
        assert!(profile.enabled);
        assert_eq!(profile.mpt, Duration::from_millis(20));
        assert_eq!(profile.mpt_update, Duration::from_millis(5));
        assert_eq!(profile.mpt_hash, Duration::from_millis(2));
    }

    #[test]
    fn state_root_profile_sink_follows_the_profiling_switch() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let cache = Arc::new(neo_storage::persistence::StoreCache::new(store));
        let handlers = crate::state_service::StateServiceCommitHandlers::new(cache, false)
            .expect("state handlers");
        let stats = Arc::new(ExecutionStats::default());

        assert!(!handlers.profile_sink_registered());

        install_state_root_profile_sink(&handlers, &stats, true);
        assert!(handlers.profile_sink_registered());

        install_state_root_profile_sink(&handlers, &stats, false);
        assert!(!handlers.profile_sink_registered());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn execution_profiling_switch_is_observation_only() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        // This instance runs without a state service, so the switch only flips
        // the profiling gate and must never touch an execution result.
        assert!(system.state_commit_handlers().is_none());
        assert!(!system.execution_stats().profile_snapshot().enabled);

        system.set_execution_profiling(true);
        assert!(system.execution_stats().profile_snapshot().enabled);
        assert!(system.state_commit_handlers().is_none());

        system.set_execution_profiling(false);
        assert!(!system.execution_stats().profile_snapshot().enabled);

        system.shutdown().await.expect("shutdown");
    }

    #[test]
    fn persistence_stage_nesting_names_every_overlapping_stage() {
        // Consumers such as the offline `.acc` import summary publish each stage
        // as its own measurement, so the rule that overlapping stages are never
        // summed has to stay discoverable at the definition site.
        for relation in [
            "transaction_vm<transaction_loop",
            "state_change_extraction+mpt<participant_precommit",
            "store_database_write<store_commit",
        ] {
            assert!(
                PERSISTENCE_STAGE_NESTING.contains(relation),
                "PERSISTENCE_STAGE_NESTING lost the {relation} relation"
            );
        }
    }
}
