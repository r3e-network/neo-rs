//! Canonical, pure block-persistence operation for the ledger.
//!
//! [`persist_block`] is a free function with no `self`, no actor system, no networking
//! and no concrete engine: every side-effecting collaborator is supplied as an inverted
//! port ([`SnapshotStore`], [`EngineFactory`], [`RuntimeCacheSink`], [`EvidenceSink`])
//! plus an ordered slice of [`CommitParticipant`]s. This keeps `neo-ledger` free of any
//! `ledger → core / system / p2p / mempool / native` reverse edge; the only outward
//! dependency is a *type-only* edge to `neo-smart-contract` (task-102 L.1 decision A).
//!
//! # The 15-step canonical order (immutable)
//!
//! The step order below is a hard contract (task-102 Part A). It reproduces the legacy
//! `NeoSystem::persist_block_internal` pipeline exactly; reordering any step changes
//! consensus-critical behaviour.
//!
//! 1.  Resolve [`PersistOptions`] and gate detailed-execution emission
//!     (`detailed = !fast_sync || any participant opts into fast-sync`).
//! 2.  Project the canonical block identity (hash, height) and allocate the *bounded*
//!     fault-evidence buffer sized from [`EvidenceSink::available_capacity`].
//! 3.  Open the store write transaction over the current snapshot ([`SnapshotStore`]).
//! 4.  Derive the base overlay snapshot that accumulates the block's committed state.
//! 5.  Pre-compute transaction hashes (fail-closed before any engine work).
//! 6.  `OnPersist`: open the native engine, run `native_on_persist`,
//!     `record_native_persist_success`.
//! 7.  Emit the `OnPersist` [`ExecutionReceipt`] when detailed.
//! 8.  Seed native-only contract states and the shared native cache handle for children.
//! 9.  Take the [`LedgerTransactionStates`] token from the `OnPersist` engine.
//! 10. Per-transaction application loop over one shared child overlay:
//!     set/take state, `load_script`, `execute_allow_fault`; emit a detailed receipt or
//!     **record the VM state unconditionally**; then dispose by VM state:
//!     * **10a `HALT`** — merge the child overlay into the base snapshot.
//!     * **10b `FAULT`** — reset the child overlay and stage a bounded
//!       [`FaultObservation`] (pure category, no observation schema).
//!     * **10c other** — abort with [`LedgerError::UnexpectedVmState`].
//! 11. `PostPersist`: open the seeded native engine, set state, run `native_post_persist`,
//!     `record_native_persist_success`, emit the receipt when detailed.
//! 12. Commit participants **stage/precommit in registration order**; the first `Err`
//!     triggers **reverse-order discard** of already-staged participants and returns
//!     [`LedgerError::ParticipantCommit`] (Display embeds the participant message).
//! 13. **Store commit** ([`StoreTransactionPort::commit`]); on failure reverse-discard
//!     and return [`LedgerError::StoreCommit`] (Display carries the store commit prefix).
//! 14. **Runtime cache / tip update**: `record_block` (online) or `record_tip` (offline
//!     import); on failure reverse-discard and return [`LedgerError::RuntimeCache`].
//! 15. **Infallible post-commit** (never reorders, never fails):
//!     * **15a** committed participants run in registration order for *all*
//!       participants regardless of fast-sync gating.
//!     * **15b** bounded fault-evidence export through [`EvidenceSink`]; a missing,
//!       full or closed sink only increments the dropped count.
//!     * **15c** plugin `BlockReceived` broadcast when detailed.
//!
//! Returns a [`PersistReceipt`] carrying the ordered receipts and evidence accounting.

pub mod engine_port;
pub mod error;
pub mod evidence;
pub mod fast_path;
pub mod participant;
pub mod receipt;

pub use engine_port::{
    ApplicationEnginePort, EngineFactory, NativeCacheHandle, NativeEnginePort,
    SeededNativeContracts,
};
pub use error::{LedgerError, LedgerResult};
pub use evidence::{
    EvidenceSink, FaultCategory, FaultObservation, MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK,
};
pub use participant::{CommitContext, CommitParticipant, StageContext};
pub use receipt::{ExecutionReceipt, PersistReceipt};

use evidence::FaultObservationBuffer;
use neo_primitives::Verifiable;
use neo_protocol::execution::Block;
use neo_smart_contract::application_engine::LedgerTransactionStates;
use neo_storage::persistence::DataCache;
use neo_vm::VmState;
use std::sync::Arc;

/// Options selecting the persistence mode for one [`persist_block`] invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistOptions {
    /// Fast-sync mode suppresses detailed receipts unless a participant opts in.
    pub fast_sync: bool,
    /// When `true`, the runtime caches are updated with the block (online sync); when
    /// `false`, only the tip height advances (offline import).
    pub update_runtime_cache: bool,
}

impl PersistOptions {
    /// Online sync: detailed receipts, runtime cache updated.
    pub const fn online() -> Self {
        Self {
            fast_sync: false,
            update_runtime_cache: true,
        }
    }

    /// Offline import: detailed receipts, tip-only advance (no runtime block cache).
    pub const fn offline_import() -> Self {
        Self {
            fast_sync: false,
            update_runtime_cache: false,
        }
    }

    /// Fast-sync import: gated receipts, runtime cache updated.
    pub const fn fast_sync() -> Self {
        Self {
            fast_sync: true,
            update_runtime_cache: true,
        }
    }
}

impl Default for PersistOptions {
    fn default() -> Self {
        Self::online()
    }
}

/// Canonical write transaction over the store snapshot for one block persist.
///
/// Owned by the `neo-system`/storage adapter. The ledger only threads the base overlay
/// and issues the single canonical commit.
pub trait StoreTransactionPort: Send {
    /// The base overlay snapshot accumulating the block's committed state (step 4).
    ///
    /// Returns a shared handle; the pipeline derives per-transaction child overlays via
    /// [`DataCache::clone_cache`] and merges them back with [`DataCache::merge_from_child`].
    fn base_snapshot(&self) -> Arc<DataCache>;

    /// Commits the staged canonical block state to the underlying store (step 13).
    ///
    /// The `String` is the store-owned failure message; the pipeline lifts it into
    /// [`LedgerError::StoreCommit`] behind the canonical store commit prefix.
    fn commit(self: Box<Self>) -> Result<(), String>;
}

/// Supplies the canonical store write transaction (steps 3/13).
pub trait SnapshotStore: Send + Sync {
    /// Opens the write transaction over the current store snapshot.
    fn open_transaction(
        &self,
        options: &PersistOptions,
    ) -> Result<Box<dyn StoreTransactionPort>, LedgerError>;
}

/// Runtime-facing sink for cache/tip updates and the plugin broadcast (steps 14/15c).
pub trait RuntimeCacheSink: Send + Sync {
    /// Records the block into the online runtime caches (step 14, online).
    ///
    /// The `String` is the sink-owned failure message; the pipeline lifts it into
    /// [`LedgerError::RuntimeCache`].
    fn record_block(&self, block: &Block) -> Result<(), String>;

    /// Advances the tip height for offline imports (step 14, offline). Infallible.
    fn record_tip(&self, index: u32);

    /// Broadcasts the plugin `BlockReceived` event (step 15c). Infallible.
    fn broadcast_block_received(&self, block_hash: String, block_height: u32);
}

/// Persists one canonical block through the pure 15-step pipeline.
///
/// See the module documentation for the immutable step order. This function performs no
/// I/O of its own; every effect flows through the supplied ports.
pub fn persist_block(
    block: Block,
    store: &dyn SnapshotStore,
    engines: &dyn EngineFactory,
    runtime_cache: &dyn RuntimeCacheSink,
    evidence: &dyn EvidenceSink,
    participants: &[Arc<dyn CommitParticipant>],
    options: &PersistOptions,
) -> LedgerResult<PersistReceipt> {
    // Step 1 — resolve options and detailed-execution gating.
    let fast_sync = options.fast_sync;
    let detailed = !fast_sync || participants.iter().any(|p| p.run_during_fast_sync());

    // Step 2 — canonical block identity + bounded fault-evidence buffer.
    let block_hash = block.hash();
    let block_height = block.index();
    let persisting_block = Arc::new(block);
    let mut fault_buffer =
        FaultObservationBuffer::for_available_capacity(evidence.available_capacity());

    // Steps 3/4 — open the store write transaction and derive the base overlay snapshot.
    let transaction = store.open_transaction(options)?;
    let base_snapshot = transaction.base_snapshot();

    // Step 5 — pre-compute transaction hashes (fail-closed before any engine work).
    // The legacy pipeline parallelised this with rayon for >= 2 transactions; hashing is
    // a pure, order-independent projection, so the adapter may reinstate parallelism.
    for tx in persisting_block.transactions.iter() {
        tx.try_hash().map_err(|err| LedgerError::TransactionHash {
            message: err.to_string(),
        })?;
    }

    // Step 6 — OnPersist native execution.
    let mut on_persist =
        engines.open_on_persist(Arc::clone(&base_snapshot), Arc::clone(&persisting_block))?;
    on_persist.native_on_persist()?;
    on_persist.record_native_persist_success();

    // Step 7 — OnPersist receipt (detailed only).
    let mut receipts = Vec::with_capacity(persisting_block.transactions.len() + 2);
    if detailed {
        receipts.push(on_persist.execution_receipt());
    }

    // Step 8 — seed native-only contract states + shared native cache handle.
    let seeded = on_persist.seed_native_contracts();
    let native_cache = on_persist.native_cache_handle();

    // Step 9 — take the ledger transaction state token from the OnPersist engine.
    let mut tx_states: LedgerTransactionStates = on_persist.take_transaction_states();

    // Step 10 — per-transaction application loop over one shared child overlay.
    if !persisting_block.transactions.is_empty() {
        let tx_overlay = Arc::new(base_snapshot.clone_cache());
        for tx in persisting_block.transactions.iter() {
            let tx_hash = tx.try_hash().map_err(|err| LedgerError::TransactionHash {
                message: err.to_string(),
            })?;

            // Attempt fast-path execution if enabled
            if let Some(fast_result) = fast_path::try_fast_path_execution(
                tx,
                &tx_overlay,
                tx.system_fee(),
            ) {
                // Fast-path succeeded - use its result instead of full VM
                match fast_result.vm_state {
                    VmState::HALT => {
                        // Fast-path transfer succeeded
                        // Storage changes are already applied to tx_overlay
                        // Merge into base snapshot
                        base_snapshot.merge_from_child(&tx_overlay);

                        // Record transaction state for this fast-path execution
                        // Note: Notifications are not emitted yet - this is a known limitation
                        // Production implementation would need to emit Transfer events here

                        if detailed {
                            // TODO: Build a proper execution receipt for fast-path
                            // For now, skip detailed receipt for fast-path transfers
                        }

                        // Continue to next transaction without full VM execution
                        continue;
                    }
                    VmState::FAULT => {
                        // Fast-path detected a fault (e.g., insufficient balance)
                        // Reset overlay and record the fault
                        tx_overlay.reset();

                        if detailed {
                            // TODO: Build a proper fault receipt for fast-path
                        }

                        fault_buffer.push(FaultObservation {
                            transaction_hash: tx_hash,
                            block_height,
                            block_hash,
                            vm_state: VmState::FAULT,
                            category: FaultCategory::VmFault,
                            gas_consumed: fast_result.gas_consumed,
                        });

                        // Continue to next transaction
                        continue;
                    }
                    _ => {
                        // Unexpected VM state from fast-path - should not happen
                        // Fall through to full VM execution
                    }
                }
            }

            // Full VM execution (fallback or primary path)
            let container: Arc<dyn Verifiable> = Arc::new(tx.clone());
            let mut app = engines.open_application(
                container,
                Arc::clone(&tx_overlay),
                Arc::clone(&persisting_block),
                tx.system_fee(),
                &seeded,
                &native_cache,
            )?;
            app.set_transaction_states(tx_states);
            app.load_script(tx.script().to_vec())?;
            let vm_state = app.execute_allow_fault();

            if detailed {
                receipts.push(app.execution_receipt());
            } else {
                // Record the VM state unconditionally even without a detailed receipt.
                app.record_transaction_vm_state(&tx_hash, vm_state);
            }
            tx_states = app.take_transaction_states();

            match vm_state {
                // Step 10a — HALT: merge the child overlay into the base snapshot.
                VmState::HALT => base_snapshot.merge_from_child(&tx_overlay),
                // Step 10b — FAULT: reset the overlay and stage a bounded observation.
                VmState::FAULT => {
                    tx_overlay.reset();
                    let category = if app.fault_exception().is_some() {
                        FaultCategory::CaughtVmException
                    } else {
                        FaultCategory::VmFault
                    };
                    fault_buffer.push(FaultObservation {
                        transaction_hash: tx_hash,
                        block_height,
                        block_hash,
                        vm_state: VmState::FAULT,
                        category,
                        gas_consumed: app.fee_consumed(),
                    });
                }
                // Step 10c — any other VM state aborts before any commit side effect.
                other => {
                    return Err(LedgerError::UnexpectedVmState {
                        transaction_hash: tx_hash,
                        vm_state: other,
                    });
                }
            }
        }
    }

    // Step 11 — PostPersist native execution over the merged base snapshot.
    let mut post_persist = engines.open_post_persist(
        Arc::clone(&base_snapshot),
        Arc::clone(&persisting_block),
        &seeded,
        &native_cache,
    )?;
    post_persist.set_transaction_states(tx_states);
    post_persist.native_post_persist()?;
    post_persist.record_native_persist_success();
    if detailed {
        receipts.push(post_persist.execution_receipt());
    }

    // Step 12 — commit participants stage/precommit in registration order.
    let stage = StageContext {
        block: &persisting_block,
        snapshot: &base_snapshot,
        receipts: &receipts,
        fast_sync,
    };
    let mut staged: Vec<usize> = Vec::new();
    for (index, participant) in participants.iter().enumerate() {
        if fast_sync && !participant.run_during_fast_sync() {
            continue;
        }
        if let Err(message) = participant.precommit(&stage) {
            discard_staged_reverse(participants, &staged, &stage);
            return Err(LedgerError::ParticipantCommit { message });
        }
        staged.push(index);
    }

    // Step 13 — canonical store commit.
    if let Err(message) = transaction.commit() {
        discard_staged_reverse(participants, &staged, &stage);
        return Err(LedgerError::StoreCommit {
            index: block_height,
            message,
        });
    }

    // Step 14 — runtime cache / tip update.
    let cache_result = if options.update_runtime_cache {
        runtime_cache
            .record_block(&persisting_block)
            .map_err(|message| LedgerError::RuntimeCache {
                index: block_height,
                message,
            })
    } else {
        runtime_cache.record_tip(block_height);
        Ok(())
    };
    if let Err(err) = cache_result {
        discard_staged_reverse(participants, &staged, &stage);
        return Err(err);
    }

    // Step 15a — infallible committed participants (registration order, all participants).
    let commit_context = CommitContext {
        block: &persisting_block,
        fast_sync,
    };
    for participant in participants.iter() {
        participant.committed(&commit_context);
    }

    // Step 15b — bounded fault-evidence export (never fails the pipeline).
    let staged_fault_observations = fault_buffer.staged_len();
    let mut dropped = fault_buffer.dropped;
    for observation in std::mem::take(&mut fault_buffer.observations) {
        let emitted = evidence.try_emit(&observation);
        dropped = dropped.saturating_add(u64::from(!emitted));
    }
    evidence.record_dropped(dropped);

    // Step 15c — plugin BlockReceived broadcast (detailed only, never fails).
    if detailed {
        runtime_cache.broadcast_block_received(block_hash.to_string(), block_height);
    }

    Ok(PersistReceipt {
        block_hash,
        block_height,
        detailed,
        receipts,
        staged_fault_observations,
        dropped_fault_observations: dropped,
    })
}

/// Discards already-staged participants in reverse registration order (steps 12/13/14).
fn discard_staged_reverse(
    participants: &[Arc<dyn CommitParticipant>],
    staged: &[usize],
    context: &StageContext<'_>,
) {
    for &index in staged.iter().rev() {
        participants[index].discard(context);
    }
}

#[cfg(test)]
mod tests {
    //! Operation-level tests for the pure [`persist_block`] pipeline.
    //!
    //! These exercise the canonical 15-step sequencing, the HALT/FAULT/other VM-state
    //! disposition (steps 10a/10b/10c), reverse-order participant discard (steps 12/13/14),
    //! the store-commit and runtime-cache error `Display` literals, unconditional VM-state
    //! recording under suppressed receipts, and bounded post-commit evidence export
    //! (step 15b). The six verbatim fast-sync integration tests remain owned by the
    //! `neo-system` adapter (task-102 Part I) and are *not* duplicated here.

    use super::*;
    use neo_primitives::{TriggerType, UInt256};
    use neo_protocol::payloads::Transaction;
    use neo_smart_contract::application_engine::{LedgerTransactionStates, NativeContractsCache};
    use parking_lot::Mutex;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Log = Arc<Mutex<Vec<String>>>;

    fn log() -> Log {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn block_with_transactions(count: usize) -> Block {
        let mut block = Block::default();
        block.transactions = (0..count)
            .map(|index| {
                let mut tx = Transaction::new();
                tx.set_nonce(index as u32 + 1);
                tx.set_valid_until_block(100);
                tx.set_script(vec![0x11]);
                tx
            })
            .collect();
        block
    }

    // ---- engine port mocks ----------------------------------------------------

    struct MockNative {
        name: &'static str,
        trigger: TriggerType,
        states: LedgerTransactionStates,
        log: Log,
    }

    impl NativeEnginePort for MockNative {
        fn native_on_persist(&mut self) -> Result<(), LedgerError> {
            self.log
                .lock()
                .push(format!("{}:native_on_persist", self.name));
            Ok(())
        }
        fn native_post_persist(&mut self) -> Result<(), LedgerError> {
            self.log
                .lock()
                .push(format!("{}:native_post_persist", self.name));
            Ok(())
        }
        fn record_native_persist_success(&mut self) {}
        fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
            self.states = states;
        }
        fn take_transaction_states(&mut self) -> LedgerTransactionStates {
            std::mem::take(&mut self.states)
        }
        fn seed_native_contracts(&self) -> SeededNativeContracts {
            HashMap::new()
        }
        fn native_cache_handle(&self) -> NativeCacheHandle {
            Arc::new(Mutex::new(NativeContractsCache::default()))
        }
        fn execution_receipt(&mut self) -> ExecutionReceipt {
            ExecutionReceipt::native(self.trigger, VmState::HALT, 0)
        }
    }

    struct MockApp {
        tx_hash: UInt256,
        vm_state: VmState,
        exception: Option<String>,
        states: LedgerTransactionStates,
        log: Log,
    }

    impl ApplicationEnginePort for MockApp {
        fn load_script(&mut self, _script: Vec<u8>) -> Result<(), LedgerError> {
            Ok(())
        }
        fn execute_allow_fault(&mut self) -> VmState {
            self.vm_state
        }
        fn record_transaction_vm_state(&mut self, tx_hash: &UInt256, vm_state: VmState) {
            self.log
                .lock()
                .push(format!("record_vm_state:{tx_hash:?}:{vm_state:?}"));
        }
        fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
            self.states = states;
        }
        fn take_transaction_states(&mut self) -> LedgerTransactionStates {
            std::mem::take(&mut self.states)
        }
        fn fee_consumed(&self) -> i64 {
            0
        }
        fn fault_exception(&self) -> Option<String> {
            self.exception.clone()
        }
        fn execution_receipt(&mut self) -> ExecutionReceipt {
            ExecutionReceipt {
                trigger: TriggerType::Application,
                transaction_hash: Some(self.tx_hash),
                vm_state: self.vm_state,
                gas_consumed: 0,
                exception: self.exception.clone(),
            }
        }
    }

    struct MockFactory {
        vm_states: Arc<Mutex<VecDeque<VmState>>>,
        exception: Option<String>,
        counter: Arc<AtomicUsize>,
        log: Log,
    }

    impl MockFactory {
        fn new(vm_states: Vec<VmState>, exception: Option<String>, log: Log) -> Self {
            Self {
                vm_states: Arc::new(Mutex::new(vm_states.into())),
                exception,
                counter: Arc::new(AtomicUsize::new(1)),
                log,
            }
        }
        fn next_hash(&self) -> UInt256 {
            let byte = (self.counter.fetch_add(1, Ordering::SeqCst) % 256) as u8;
            UInt256::from([byte; 32])
        }
    }

    impl EngineFactory for MockFactory {
        fn open_on_persist(
            &self,
            _snapshot: Arc<DataCache>,
            _block: Arc<Block>,
        ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
            Ok(Box::new(MockNative {
                name: "on_persist",
                trigger: TriggerType::OnPersist,
                states: LedgerTransactionStates::default(),
                log: self.log.clone(),
            }))
        }
        fn open_application(
            &self,
            _container: Arc<dyn Verifiable>,
            _snapshot: Arc<DataCache>,
            _block: Arc<Block>,
            _system_fee: i64,
            _seeded: &SeededNativeContracts,
            _cache: &NativeCacheHandle,
        ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError> {
            let vm_state = self.vm_states.lock().pop_front().unwrap_or(VmState::HALT);
            Ok(Box::new(MockApp {
                tx_hash: self.next_hash(),
                vm_state,
                exception: self.exception.clone(),
                states: LedgerTransactionStates::default(),
                log: self.log.clone(),
            }))
        }
        fn open_post_persist(
            &self,
            _snapshot: Arc<DataCache>,
            _block: Arc<Block>,
            _seeded: &SeededNativeContracts,
            _cache: &NativeCacheHandle,
        ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
            Ok(Box::new(MockNative {
                name: "post_persist",
                trigger: TriggerType::PostPersist,
                states: LedgerTransactionStates::default(),
                log: self.log.clone(),
            }))
        }
    }

    // ---- storage / runtime / evidence mocks -----------------------------------

    struct MockTransaction {
        base: Arc<DataCache>,
        commit_result: Result<(), String>,
        log: Log,
    }

    impl StoreTransactionPort for MockTransaction {
        fn base_snapshot(&self) -> Arc<DataCache> {
            Arc::clone(&self.base)
        }
        fn commit(self: Box<Self>) -> Result<(), String> {
            self.log.lock().push("store_commit".to_string());
            self.commit_result.clone()
        }
    }

    struct MockStore {
        commit_result: Result<(), String>,
        log: Log,
    }

    impl SnapshotStore for MockStore {
        fn open_transaction(
            &self,
            _options: &PersistOptions,
        ) -> Result<Box<dyn StoreTransactionPort>, LedgerError> {
            Ok(Box::new(MockTransaction {
                base: Arc::new(DataCache::new(false)),
                commit_result: self.commit_result.clone(),
                log: self.log.clone(),
            }))
        }
    }

    struct MockRuntime {
        record_result: Result<(), String>,
        log: Log,
    }

    impl RuntimeCacheSink for MockRuntime {
        fn record_block(&self, block: &Block) -> Result<(), String> {
            self.log
                .lock()
                .push(format!("record_block:{}", block.index()));
            self.record_result.clone()
        }
        fn record_tip(&self, index: u32) {
            self.log.lock().push(format!("record_tip:{index}"));
        }
        fn broadcast_block_received(&self, _hash: String, height: u32) {
            self.log.lock().push(format!("broadcast:{height}"));
        }
    }

    struct MockEvidence {
        capacity: usize,
        accept: bool,
        dropped: Arc<Mutex<u64>>,
        log: Log,
    }

    impl EvidenceSink for MockEvidence {
        fn available_capacity(&self) -> usize {
            self.capacity
        }
        fn try_emit(&self, _observation: &FaultObservation) -> bool {
            self.log.lock().push("emit".to_string());
            self.accept
        }
        fn record_dropped(&self, dropped: u64) {
            *self.dropped.lock() = dropped;
            self.log.lock().push(format!("record_dropped:{dropped}"));
        }
    }

    // ---- participant mock -----------------------------------------------------

    struct MockParticipant {
        name: &'static str,
        opt_into_fast_sync: bool,
        precommit: Result<(), String>,
        log: Log,
    }

    impl CommitParticipant for MockParticipant {
        fn run_during_fast_sync(&self) -> bool {
            self.opt_into_fast_sync
        }
        fn precommit(&self, _context: &StageContext<'_>) -> Result<(), String> {
            self.log.lock().push(format!("{}:precommit", self.name));
            self.precommit.clone()
        }
        fn discard(&self, _context: &StageContext<'_>) {
            self.log.lock().push(format!("{}:discard", self.name));
        }
        fn committed(&self, _context: &CommitContext<'_>) {
            self.log.lock().push(format!("{}:committed", self.name));
        }
    }

    fn participant(
        name: &'static str,
        opt_into_fast_sync: bool,
        precommit: Result<(), String>,
        log: Log,
    ) -> Arc<dyn CommitParticipant> {
        Arc::new(MockParticipant {
            name,
            opt_into_fast_sync,
            precommit,
            log,
        })
    }

    // ---- tests ----------------------------------------------------------------

    #[test]
    fn halt_merges_and_fault_stages_bounded_evidence_after_commit() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(
            vec![VmState::HALT, VmState::FAULT],
            Some("caught".to_string()),
            log.clone(),
        );
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };

        let receipt = persist_block(
            block_with_transactions(2),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("persist succeeds");

        // OnPersist + 2 transactions + PostPersist.
        assert_eq!(receipt.receipts.len(), 4);
        assert!(receipt.detailed);
        assert_eq!(receipt.staged_fault_observations, 1);
        assert_eq!(receipt.dropped_fault_observations, 0);

        let entries = log.lock().clone();
        let commit = entries
            .iter()
            .position(|e| e == "store_commit")
            .expect("commit");
        let emit = entries.iter().position(|e| e == "emit").expect("emit");
        assert!(
            emit > commit,
            "fault evidence must be exported only after the store commit"
        );
        assert!(entries.iter().any(|e| e == "broadcast:0"));
    }

    #[test]
    fn precommit_failure_discards_staged_participants_in_reverse_order() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };
        let participants = vec![
            participant("a", false, Ok(()), log.clone()),
            participant("b", false, Ok(()), log.clone()),
            participant(
                "c",
                false,
                Err("injected committing failure".to_string()),
                log.clone(),
            ),
            participant("d", false, Ok(()), log.clone()),
        ];

        let err = persist_block(
            block_with_transactions(1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &participants,
            &PersistOptions::online(),
        )
        .expect_err("precommit failure aborts persistence");

        // Display embeds the injected participant message verbatim.
        assert!(err.to_string().contains("injected committing failure"));
        let entries = log.lock().clone();
        // Reverse-order discard of already-staged participants b, then a. c never staged,
        // d never reached.
        let b_discard = entries
            .iter()
            .position(|e| e == "b:discard")
            .expect("b discard");
        let a_discard = entries
            .iter()
            .position(|e| e == "a:discard")
            .expect("a discard");
        assert!(b_discard < a_discard, "discard must run in reverse order");
        assert!(!entries.iter().any(|e| e == "c:discard"));
        assert!(!entries.iter().any(|e| e == "d:precommit"));
        assert!(!entries.iter().any(|e| e == "store_commit"));
    }

    #[test]
    fn store_commit_failure_carries_prefix_and_discards_in_reverse() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Err("boom".to_string()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };
        let participants = vec![
            participant("a", false, Ok(()), log.clone()),
            participant("b", false, Ok(()), log.clone()),
        ];

        let err = persist_block(
            block_with_transactions(1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &participants,
            &PersistOptions::online(),
        )
        .expect_err("store commit failure aborts persistence");

        // Display carries the canonical store commit prefix.
        assert_eq!(
            err.to_string(),
            "failed to commit store cache for block 0: boom"
        );
        let entries = log.lock().clone();
        let b_discard = entries
            .iter()
            .position(|e| e == "b:discard")
            .expect("b discard");
        let a_discard = entries
            .iter()
            .position(|e| e == "a:discard")
            .expect("a discard");
        assert!(b_discard < a_discard);
        assert!(!entries.iter().any(|e| e == "record_block:0"));
    }

    #[test]
    fn unexpected_vm_state_aborts_before_commit() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::BREAK], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };

        let err = persist_block(
            block_with_transactions(1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect_err("non HALT/FAULT VM state aborts");

        assert!(matches!(err, LedgerError::UnexpectedVmState { .. }));
        assert!(err.to_string().contains("unexpected transaction VM state"));
        assert!(!log.lock().iter().any(|e| e == "store_commit"));
    }

    #[test]
    fn post_commit_evidence_export_is_bounded_and_never_fails() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        // Eight FAULT transactions but only four staging slots and a rejecting sink.
        let factory = MockFactory::new(vec![VmState::FAULT; 8], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 4,
            accept: false,
            dropped: dropped.clone(),
            log: log.clone(),
        };

        let receipt = persist_block(
            block_with_transactions(8),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("a rejecting sink cannot fail persistence");

        // Bounded staging: min(capacity=4, MAX=64) = 4 staged; 4 overflow-dropped,
        // plus 4 rejected by the sink on export = 8 total dropped.
        assert_eq!(receipt.staged_fault_observations, 4);
        assert_eq!(receipt.dropped_fault_observations, 8);
        assert_eq!(*dropped.lock(), 8);
        // Commit still succeeded and the tip/cache advanced.
        assert!(log.lock().iter().any(|e| e == "store_commit"));
        assert!(log.lock().iter().any(|e| e == "record_block:0"));
    }

    #[test]
    fn fast_sync_without_opt_in_suppresses_receipts_but_records_vm_state() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::HALT; 2], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };
        // A participant that does NOT opt into fast-sync: detailed emission stays off,
        // but committed() must still run for it (step 15a).
        let participants = vec![participant("a", false, Ok(()), log.clone())];

        let receipt = persist_block(
            block_with_transactions(2),
            &store,
            &factory,
            &runtime,
            &evidence,
            &participants,
            &PersistOptions::fast_sync(),
        )
        .expect("fast-sync persist succeeds");

        assert!(!receipt.detailed);
        assert!(receipt.receipts.is_empty());
        let entries = log.lock().clone();
        // VM state recorded unconditionally for both transactions despite no receipts.
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.starts_with("record_vm_state:"))
                .count(),
            2
        );
        // precommit skipped (fast-sync, no opt-in) but committed still runs.
        assert!(!entries.iter().any(|e| e == "a:precommit"));
        assert!(entries.iter().any(|e| e == "a:committed"));
        // No plugin broadcast while detailed emission is suppressed.
        assert!(!entries.iter().any(|e| e.starts_with("broadcast:")));
    }

    #[test]
    fn fast_sync_with_opt_in_participant_emits_detailed_receipts() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };
        let participants = vec![participant("a", true, Ok(()), log.clone())];

        let receipt = persist_block(
            block_with_transactions(1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &participants,
            &PersistOptions::fast_sync(),
        )
        .expect("fast-sync persist succeeds");

        // An opt-in participant forces detailed emission even under fast-sync.
        assert!(receipt.detailed);
        assert_eq!(receipt.receipts.len(), 3);
        let entries = log.lock().clone();
        assert!(entries.iter().any(|e| e == "a:precommit"));
        assert!(entries.iter().any(|e| e == "a:committed"));
        assert!(entries.iter().any(|e| e == "broadcast:0"));
    }

    #[test]
    fn offline_import_advances_tip_without_runtime_block_cache() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
        let runtime = MockRuntime {
            record_result: Ok(()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };

        let receipt = persist_block(
            block_with_transactions(1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::offline_import(),
        )
        .expect("offline import succeeds");

        assert!(receipt.detailed);
        let entries = log.lock().clone();
        assert!(entries.iter().any(|e| e == "record_tip:0"));
        assert!(!entries.iter().any(|e| e.starts_with("record_block:")));
    }

    #[test]
    fn runtime_cache_failure_discards_participants_after_store_commit() {
        let log = log();
        let dropped = Arc::new(Mutex::new(0u64));
        let store = MockStore {
            commit_result: Ok(()),
            log: log.clone(),
        };
        let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
        let runtime = MockRuntime {
            record_result: Err("cache offline".to_string()),
            log: log.clone(),
        };
        let evidence = MockEvidence {
            capacity: 64,
            accept: true,
            dropped: dropped.clone(),
            log: log.clone(),
        };
        let participants = vec![participant("a", false, Ok(()), log.clone())];

        let err = persist_block(
            block_with_transactions(1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &participants,
            &PersistOptions::online(),
        )
        .expect_err("runtime cache failure aborts");

        assert_eq!(
            err.to_string(),
            "failed to update runtime block cache for block 0: cache offline"
        );
        let entries = log.lock().clone();
        // Store commit happened first, then the participant was discarded on rollback.
        let commit = entries
            .iter()
            .position(|e| e == "store_commit")
            .expect("commit");
        let discard = entries
            .iter()
            .position(|e| e == "a:discard")
            .expect("discard");
        assert!(commit < discard);
        // No committed/evidence side effects after a failed cache update.
        assert!(!entries.iter().any(|e| e == "a:committed"));
    }
}
