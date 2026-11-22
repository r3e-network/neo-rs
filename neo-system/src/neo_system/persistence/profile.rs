use super::*;

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
    pub(super) profiling_enabled: AtomicBool,
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
    pub(super) fn record_strategy_outcome(
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
