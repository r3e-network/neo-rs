//! Speculative parallel transaction execution with ordered commit (Task 45 / Phase 7B).
//!
//! Task 44 made every application engine able to report the exact storage reads, range
//! reads and writes it performed (`neo_vm::RwSetTracker`), for free when unused. This
//! module turns that into a scheduler.
//!
//! # Shape
//!
//! The canonical pipeline executes transactions strictly serially over one shared child
//! overlay (`neo_ledger::persistence::execute_serially`). [`ParallelTransactionLoop`]
//! keeps that implementation available and adds a second one:
//!
//! 1. **Speculative phase** — every transaction runs on its own thread against its own
//!    overlay of the *pre-block* snapshot (`DataCache::clone_cache`), with read-write set
//!    recording enabled. Nothing is committed and nothing is published.
//! 2. **Ordered validation phase** — the captured runs are walked in block order. A run
//!    is committed when its read set does not intersect the writes already committed by
//!    earlier transactions in this block; otherwise the transaction is re-executed
//!    serially against the current committed state, exactly like the serial loop would.
//!
//! # Why this is deterministic
//!
//! * Overlays are per-transaction and are folded into the base snapshot in block order,
//!   so the final merged state is the serial result regardless of the thread schedule.
//! * Every speculative run starts from generation zero on the same pre-block snapshot,
//!   so validation compares *structure* (did anyone write a key this run read?) rather
//!   than versions. Versions cannot answer that question here: a run that reads its own
//!   write observes the same version a predecessor's write would have.
//! * Detailed observations are published only during ordered validation, so the
//!   `ApplicationExecuted` sequence, the receipts and the fault observations all follow
//!   block order even though execution did not.
//! * A faulted speculative run never merges its overlay and never contributes its writes
//!   to the committed set, matching the serial `reset()` disposal.
//! * Any unexpected error during speculation discards every speculative result and
//!   replays the whole block through the serial loop. Nothing has been merged at that
//!   point, so the fallback is the canonical path, not an approximation.
//!
//! # Isolation of the native cache
//!
//! Each speculative run receives a private `NativeContractsCache`. The shared cache is
//! mutable and would let one transaction observe another transaction's contract
//! deployment. A private cache misses into the snapshot instead, where the read is
//! recorded, so any difference caused by a concurrent transaction becomes a detected
//! conflict rather than a silently stale hit.

use std::sync::Arc;
use std::time::Instant;

use neo_ledger::persistence::{
    ApplicationEnginePort, EngineFactory, FaultCategory, FaultObservation, LedgerError,
    LedgerResult, MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK, NativeCacheHandle, PersistenceStage,
    SpeculativeExecution, TransactionLoopContext, TransactionLoopOutcome, TransactionLoopStrategy,
    execute_serially,
};
use neo_primitives::{UInt256, Verifiable};
use neo_protocol::payloads::Transaction;
use neo_smart_contract::application_engine::{LedgerTransactionStates, NativeContractsCache};
use neo_storage::persistence::DataCache;
use neo_vm::{RwSetTracker, VmState};
use parking_lot::Mutex;
use rayon::prelude::*;

use super::persistence::ExecutionStats;

/// Generation every speculative run is stamped with.
///
/// All runs observe the same pre-block snapshot, so they share one generation and
/// validation relies on structural conflict detection rather than versions.
const SPECULATION_GENERATION: u64 = 0;

/// One transaction executed ahead of commit order.
struct SpeculatedTransaction {
    /// Isolated overlay the run wrote into; merged only if the run is accepted.
    overlay: Arc<DataCache>,
    /// The engine port, kept alive so the retained observation can be released in order.
    port: Box<dyn ApplicationEnginePort>,
    /// Receipt and read-write set captured by the run.
    execution: SpeculativeExecution,
}

/// Speculative parallel scheduler for step 10 of the persistence pipeline.
///
/// Construct one per block, hand it to
/// [`neo_ledger::persistence::persist_block_with_loop_strategy`], and drop it. The
/// counters it feeds live in [`ExecutionStats`] and only ever observe decisions the
/// scheduler already made.
pub struct ParallelTransactionLoop {
    stats: Arc<ExecutionStats>,
    min_transactions: usize,
}

impl ParallelTransactionLoop {
    /// Minimum block size worth speculating.
    ///
    /// A single transaction cannot overlap with anything, so it takes the serial path
    /// and avoids the overlay and thread-pool overhead entirely.
    pub const MIN_PARALLEL_TRANSACTIONS: usize = 2;

    /// Creates a scheduler reporting to `stats`.
    pub fn new(stats: Arc<ExecutionStats>) -> Self {
        Self {
            stats,
            min_transactions: Self::MIN_PARALLEL_TRANSACTIONS,
        }
    }

    /// Overrides the minimum block size that triggers speculation.
    ///
    /// The value is clamped to at least [`Self::MIN_PARALLEL_TRANSACTIONS`]: running a
    /// single transaction "in parallel" is strictly slower and cannot change a result.
    pub fn with_min_transactions(mut self, min_transactions: usize) -> Self {
        self.min_transactions = min_transactions.max(Self::MIN_PARALLEL_TRANSACTIONS);
        self
    }

    /// Runs the canonical serial loop while counting it as a serial block.
    fn run_serially(
        &self,
        engines: &dyn EngineFactory,
        context: &TransactionLoopContext<'_>,
        transaction_states: LedgerTransactionStates,
    ) -> LedgerResult<TransactionLoopOutcome> {
        let started = Instant::now();
        let outcome = execute_serially(engines, context, transaction_states);
        self.stats.record_serial_elapsed(started.elapsed());
        self.stats.record_serial_fallback();
        outcome
    }
}

impl TransactionLoopStrategy for ParallelTransactionLoop {
    fn execute(
        &self,
        engines: &dyn EngineFactory,
        context: &TransactionLoopContext<'_>,
        transaction_states: LedgerTransactionStates,
    ) -> LedgerResult<TransactionLoopOutcome> {
        let transactions = &context.block.transactions;
        if transactions.len() < self.min_transactions {
            return self.run_serially(engines, context, transaction_states);
        }

        let started = Instant::now();
        // Every run observes the same pre-block state token; the ordered phase applies
        // the marks in block order, exactly like the serial loop does.
        let seed = transaction_states.clone();
        let speculations: Vec<Option<SpeculatedTransaction>> = transactions
            .par_iter()
            .map(|transaction| speculate(engines, context, transaction, &seed))
            .collect();

        // A partially speculative block is not a partially committed block: nothing has
        // been merged yet, so the canonical serial loop is a complete fallback.
        if speculations.iter().any(Option::is_none) {
            self.stats.record_parallel_elapsed(started.elapsed());
            return self.run_serially(engines, context, transaction_states);
        }

        self.stats.record_speculative_block();
        self.stats
            .record_speculative_transactions(u64::try_from(transactions.len()).unwrap_or(u64::MAX));

        let result = self.validate(engines, context, transaction_states, speculations);
        self.stats.record_parallel_elapsed(started.elapsed());
        result
    }
}

impl ParallelTransactionLoop {
    /// Commits the speculative runs in block order, replaying whatever conflicts.
    fn validate(
        &self,
        engines: &dyn EngineFactory,
        context: &TransactionLoopContext<'_>,
        transaction_states: LedgerTransactionStates,
        speculations: Vec<Option<SpeculatedTransaction>>,
    ) -> LedgerResult<TransactionLoopOutcome> {
        let block = context.block;
        let block_hash = block.hash();
        let block_height = block.index();
        let fault_limit = context
            .fault_capacity
            .min(MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK);

        let mut states = transaction_states;
        let mut committed = RwSetTracker::new();
        let mut outcome = TransactionLoopOutcome::default();
        let mut dropped_faults = 0u64;

        for (index, speculation) in speculations.into_iter().enumerate() {
            let tx_hash = context.transaction_hashes[index];
            let transaction = &block.transactions[index];

            let accepted = speculation
                .filter(|candidate| !candidate.execution.rw_set.has_conflict(&committed));

            let Some(candidate) = accepted else {
                self.stats.record_speculative_conflict();
                self.stats.record_speculative_reexecution();
                replay_serially(
                    engines,
                    context,
                    transaction,
                    &tx_hash,
                    &mut states,
                    &mut committed,
                    &mut outcome,
                    fault_limit,
                    &mut dropped_faults,
                    block_hash,
                    block_height,
                )?;
                continue;
            };

            let SpeculatedTransaction {
                overlay,
                mut port,
                execution,
            } = candidate;
            let receipt = execution.receipt;
            let receipt_exception = receipt.exception.is_some();
            let receipt_gas = receipt.gas_consumed;

            match receipt.vm_state {
                // Step 10a — HALT: release the observation, then merge the overlay. The
                // publication order matches the serial loop, which captures the receipt
                // before it merges.
                VmState::HALT => {
                    port.publish_capture();
                    // The state token is cumulative across the block, so the mark has to
                    // be applied here, in block order, instead of carrying the engine's
                    // own copy forward.
                    states.mark_vm_state(&tx_hash, receipt.vm_state);
                    if context.detailed {
                        outcome.receipts.push(receipt);
                    }
                    context.base_snapshot.merge_from_child(&overlay);
                    committed.merge_committed(&execution.rw_set);
                }
                // Step 10b — FAULT: the overlay is dropped instead of merged, matching
                // the serial `reset()`. The run's writes never materialise, so they must
                // not enter the committed set either.
                VmState::FAULT => {
                    port.publish_capture();
                    states.mark_vm_state(&tx_hash, receipt.vm_state);
                    if context.detailed {
                        outcome.receipts.push(receipt);
                    }
                    drop(overlay);
                    stage_fault(
                        &mut outcome.faults,
                        &mut dropped_faults,
                        fault_limit,
                        FaultObservation {
                            transaction_hash: tx_hash,
                            block_height,
                            block_hash,
                            vm_state: VmState::FAULT,
                            category: fault_category(receipt_exception),
                            gas_consumed: receipt_gas,
                        },
                    );
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

        outcome.transaction_states = states;
        outcome.dropped_faults = dropped_faults;
        Ok(outcome)
    }
}

/// Executes one transaction against its own overlay of the pre-block snapshot.
///
/// Returns `None` when the run cannot produce a usable speculative result. Any such
/// block falls back to the serial loop; the caller never sees a partial speculation.
fn speculate(
    engines: &dyn EngineFactory,
    context: &TransactionLoopContext<'_>,
    transaction: &Transaction,
    states: &LedgerTransactionStates,
) -> Option<SpeculatedTransaction> {
    let overlay = Arc::new(context.base_snapshot.clone_cache());
    // Private cache: see the module-level "Isolation of the native cache" note.
    let native_cache: NativeCacheHandle = Arc::new(Mutex::new(NativeContractsCache::default()));
    let container: Arc<dyn Verifiable> = Arc::new(transaction.clone());

    let mut port = engines
        .open_application(
            container,
            Arc::clone(&overlay),
            Arc::clone(context.block),
            transaction.system_fee(),
            context.seeded,
            &native_cache,
        )
        .ok()?;
    port.set_transaction_states(states.clone());
    port.enable_rw_set_tracking_at_generation(SPECULATION_GENERATION);
    port.load_script(transaction.script().to_vec()).ok()?;

    let started = Instant::now();
    port.execute_allow_fault();
    if let Some(profiler) = context.profiler {
        profiler.record(PersistenceStage::TransactionVm, started.elapsed());
    }
    port.verify_execution().ok()?;

    let execution = port.capture_speculative(context.detailed)?;
    Some(SpeculatedTransaction {
        overlay,
        port,
        execution,
    })
}

/// Re-executes one conflicting transaction against the current committed state.
///
/// This is the serial loop body for a single transaction, including the read-write set
/// capture: a replayed transaction's writes have to enter the committed set, otherwise a
/// later transaction that speculatively read one of those keys would not be detected as
/// stale.
#[allow(clippy::too_many_arguments)]
fn replay_serially(
    engines: &dyn EngineFactory,
    context: &TransactionLoopContext<'_>,
    transaction: &Transaction,
    tx_hash: &UInt256,
    states: &mut LedgerTransactionStates,
    committed: &mut RwSetTracker,
    outcome: &mut TransactionLoopOutcome,
    fault_limit: usize,
    dropped_faults: &mut u64,
    block_hash: UInt256,
    block_height: u32,
) -> LedgerResult<()> {
    let overlay = Arc::new(context.base_snapshot.clone_cache());
    let container: Arc<dyn Verifiable> = Arc::new(transaction.clone());
    let mut port = engines.open_application(
        container,
        Arc::clone(&overlay),
        Arc::clone(context.block),
        transaction.system_fee(),
        context.seeded,
        context.native_cache,
    )?;
    port.set_transaction_states(states.clone());
    port.enable_rw_set_tracking_at_generation(SPECULATION_GENERATION);
    port.load_script(transaction.script().to_vec())?;

    let started = Instant::now();
    let vm_state = port.execute_allow_fault();
    if let Some(profiler) = context.profiler {
        profiler.record(PersistenceStage::TransactionVm, started.elapsed());
    }
    port.verify_execution()?;

    if !context.detailed {
        // `capture_speculative(false)` projects the receipt without touching the state
        // token, so the replay records the VM state exactly like the serial loop does.
        port.record_transaction_vm_state(tx_hash, vm_state);
    }
    let execution =
        port.capture_speculative(context.detailed)
            .ok_or_else(|| LedgerError::Engine {
                message: "replayed transaction produced no read-write set".to_string(),
            })?;
    port.publish_capture();
    let receipt = execution.receipt;
    let receipt_exception = receipt.exception.is_some();
    let receipt_gas = receipt.gas_consumed;
    *states = port.take_transaction_states();
    if context.detailed {
        outcome.receipts.push(receipt);
    }

    match vm_state {
        VmState::HALT => {
            context.base_snapshot.merge_from_child(&overlay);
            committed.merge_committed(&execution.rw_set);
        }
        VmState::FAULT => {
            overlay.reset();
            stage_fault(
                &mut outcome.faults,
                dropped_faults,
                fault_limit,
                FaultObservation {
                    transaction_hash: *tx_hash,
                    block_height,
                    block_hash,
                    vm_state: VmState::FAULT,
                    category: fault_category(receipt_exception),
                    gas_consumed: receipt_gas,
                },
            );
        }
        other => {
            return Err(LedgerError::UnexpectedVmState {
                transaction_hash: *tx_hash,
                vm_state: other,
            });
        }
    }
    Ok(())
}

/// Classifies a faulted run the way the serial loop does: a captured exception is
/// `CaughtVmException`, a bare fault is `VmFault`.
fn fault_category(has_exception: bool) -> FaultCategory {
    if has_exception {
        FaultCategory::CaughtVmException
    } else {
        FaultCategory::VmFault
    }
}

/// Stages one fault observation, counting overflow instead of allocating.
fn stage_fault(
    faults: &mut Vec<FaultObservation>,
    dropped: &mut u64,
    limit: usize,
    observation: FaultObservation,
) {
    if faults.len() < limit {
        faults.push(observation);
    } else {
        *dropped = dropped.saturating_add(1);
    }
}

/// Executes one block's transactions speculatively and commits the results in order.
///
/// This is the entry point the persistence layer uses when speculative execution is
/// enabled: it is a drop-in alternative to
/// [`neo_ledger::persistence::execute_serially`] with the same inputs and,
/// by construction, the same output.
pub fn parallel_execute_block(
    engines: &dyn EngineFactory,
    context: &TransactionLoopContext<'_>,
    transaction_states: LedgerTransactionStates,
    stats: &Arc<ExecutionStats>,
) -> LedgerResult<TransactionLoopOutcome> {
    ParallelTransactionLoop::new(Arc::clone(stats)).execute(engines, context, transaction_states)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neo_system::persistence::ParallelExecutionSnapshot;
    use crate::neo_system::{ExecutionMode, NeoSystem};
    use neo_native_contracts::GasToken;
    use neo_native_contracts::native::composition::default_native_services;
    use neo_primitives::{CallFlags, TriggerType, UInt160, WitnessScope};
    use neo_protocol::ProtocolSettings;
    use neo_protocol::execution::{ApplicationExecuted, Block};
    use neo_protocol::payloads::{Block as PayloadBlock, Header, Signer};
    use neo_protocol::witness::Witness;
    use neo_smart_contract::application_engine::{
        ApplicationEngine, NativeContract, TEST_MODE_GAS,
    };
    use neo_storage::persistence::SeekDirection;
    use neo_storage::persistence::transaction::apply_tracked_items;
    use neo_vm::{OpCode, ScriptBuilder};
    use num_bigint::BigInt;

    /// GAS minted into every sender a test needs (200 GAS in datoshi).
    const FUNDING: i64 = 200_000_000_000;

    fn account(seed: u8) -> UInt160 {
        UInt160::from_bytes(&[seed; 20]).expect("account")
    }

    /// Mints GAS into `account` on the pre-block state.
    ///
    /// The block tests need funded senders that are *not* the genesis BFT
    /// address, so several transactions can run without sharing a balance key.
    fn mint_gas(system: &NeoSystem, account: UInt160) {
        let mut store = system.context().store_snapshot_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut container = Transaction::new();
        container.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
        container.add_witness(Witness::new());
        let script_container: Arc<dyn Verifiable> = Arc::new(container);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            snapshot,
            None,
            system.settings().clone(),
            TEST_MODE_GAS,
            None,
            default_native_services(),
        )
        .expect("mint engine");
        GasToken::new()
            .mint(&mut engine, &account, &BigInt::from(FUNDING), false)
            .expect("mint GAS");
        apply_tracked_items(&mut store, engine.snapshot_cache().tracked_items());
        store.commit();
    }

    fn transfer_script(from: UInt160, to: UInt160, amount: i64) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::PUSHNULL);
        builder.emit_push_int(amount);
        builder.emit_push(&to.to_bytes());
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
        builder.to_array()
    }

    fn transaction_with_script(nonce: u32, signer: UInt160, script: Vec<u8>) -> Transaction {
        let mut transaction = Transaction::new();
        transaction.set_nonce(nonce);
        transaction.set_system_fee(TEST_MODE_GAS);
        transaction.set_network_fee(0);
        transaction.set_valid_until_block(100);
        transaction.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
        transaction.set_witnesses(vec![Witness::new()]);
        transaction.set_script(script);
        transaction
    }

    fn transfer_transaction(nonce: u32, from: UInt160, to: UInt160, amount: i64) -> Transaction {
        transaction_with_script(nonce, from, transfer_script(from, to, amount))
    }

    /// A script that only computes, touching no storage at all.
    fn pure_script() -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(1);
        builder.emit_push_int(2);
        builder.emit_opcode(OpCode::ADD);
        builder.to_array()
    }

    fn block_with(system: &NeoSystem, transactions: Vec<Transaction>) -> PayloadBlock {
        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(system.genesis_block().hash());
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(1);
        header.witness = Witness::new();
        let mut block = PayloadBlock {
            header,
            transactions,
        };
        block.rebuild_merkle_root();
        block
    }

    /// Every receipt field the two paths must agree on, in block order.
    fn observations(executions: &[ApplicationExecuted]) -> Vec<String> {
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

    /// Every key/value the node's store can see, in key order.
    fn storage_dump(system: &NeoSystem) -> Vec<(Vec<u8>, Vec<u8>)> {
        system
            .store_cache()
            .find(None, SeekDirection::Forward)
            .map(|(key, item)| (key.to_array(), item.value_bytes().to_vec()))
            .collect()
    }

    /// Runs one block on a fresh system and reports everything the two paths must share.
    async fn run(
        parallel: bool,
        mode: ExecutionMode,
        fund: &[UInt160],
        transactions: &impl Fn(&NeoSystem) -> Vec<Transaction>,
    ) -> (
        Vec<String>,
        Vec<(Vec<u8>, Vec<u8>)>,
        ParallelExecutionSnapshot,
    ) {
        let system = NeoSystem::new(ProtocolSettings::testnet(), None, None).expect("system");
        system.set_parallel_execution(parallel);
        for account in fund {
            mint_gas(&system, *account);
        }
        let block = block_with(&system, transactions(&system));
        let executions = system
            .persist_block_with_receipts_and_execution_mode(Block::from(&block), mode)
            .expect("block persists");
        let result = (
            observations(&executions),
            storage_dump(&system),
            system.execution_stats().parallel_snapshot(),
        );
        system.shutdown().await.expect("shutdown");
        result
    }

    /// Runs the same block serially and speculatively, asserting identical output.
    async fn compare_paths(
        mode: ExecutionMode,
        fund: &[UInt160],
        transactions: impl Fn(&NeoSystem) -> Vec<Transaction>,
    ) -> (ParallelExecutionSnapshot, ParallelExecutionSnapshot) {
        let serial = run(false, mode, fund, &transactions).await;
        let parallel = run(true, mode, fund, &transactions).await;
        assert_eq!(serial.0, parallel.0, "execution results differ in {mode:?}");
        assert_eq!(serial.1, parallel.1, "storage state differs in {mode:?}");
        (serial.2, parallel.2)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn independent_transfers_speculate_without_conflicts() {
        let senders = [account(0x11), account(0x22), account(0x33)];
        let recipients = [account(0x41), account(0x42), account(0x43)];

        for mode in [ExecutionMode::Canonical, ExecutionMode::Auto] {
            let (serial, parallel) = compare_paths(mode, &senders, |_| {
                senders
                    .iter()
                    .zip(recipients)
                    .enumerate()
                    .map(|(index, (from, to))| {
                        transfer_transaction(index as u32 + 1, *from, to, 1_000_000)
                    })
                    .collect()
            })
            .await;

            assert_eq!(
                serial.speculative_blocks, 0,
                "the serial loop never speculates"
            );
            assert_eq!(parallel.speculative_blocks, 1, "{mode:?} must speculate");
            assert_eq!(parallel.speculative_transactions, 3);
            assert_eq!(
                parallel.conflicts, 0,
                "disjoint senders share no written key in {mode:?}"
            );
            assert_eq!(parallel.reexecutions, 0);
            assert_eq!(parallel.serial_fallbacks, 0);
            assert!(
                parallel.parallel_nanos > 0,
                "the scheduler must time itself"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn same_sender_transfers_reexecute_after_conflicts() {
        let sender = account(0x11);
        let recipients = [account(0x41), account(0x42), account(0x43)];

        for mode in [ExecutionMode::Canonical, ExecutionMode::Auto] {
            let (_, parallel) = compare_paths(mode, &[sender], |_| {
                recipients
                    .iter()
                    .enumerate()
                    .map(|(index, to)| {
                        transfer_transaction(index as u32 + 1, sender, *to, 1_000_000)
                    })
                    .collect()
            })
            .await;

            assert_eq!(parallel.speculative_blocks, 1);
            assert_eq!(parallel.speculative_transactions, 3);
            // The first run only reads the pre-block balance; the next two read a
            // key a predecessor wrote.
            assert_eq!(parallel.conflicts, 2, "{mode:?} conflicts");
            assert_eq!(parallel.reexecutions, 2, "{mode:?} re-executions");
            assert_eq!(parallel.serial_fallbacks, 0);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn every_transaction_conflicting_still_matches_serial() {
        let sender = account(0x11);
        let recipients = [account(0x41), account(0x42), account(0x43), account(0x44)];

        let (_, parallel) = compare_paths(ExecutionMode::Canonical, &[sender], |_| {
            recipients
                .iter()
                .enumerate()
                .map(|(index, to)| transfer_transaction(index as u32 + 1, sender, *to, 1_000))
                .collect()
        })
        .await;

        assert_eq!(parallel.speculative_transactions, 4);
        assert_eq!(parallel.conflicts, 3, "every run after the first conflicts");
        assert_eq!(parallel.reexecutions, 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mixed_contract_calls_and_plain_scripts_match_serial() {
        let first = account(0x11);
        let second = account(0x22);
        let first_recipient = account(0x41);
        let second_recipient = account(0x42);

        for mode in [ExecutionMode::Canonical, ExecutionMode::Auto] {
            // Disjoint recipients: the plain script must not manufacture a conflict.
            let (_, independent) = compare_paths(mode, &[first, second], |_| {
                vec![
                    transfer_transaction(1, first, first_recipient, 500_000),
                    transaction_with_script(2, second, pure_script()),
                    transfer_transaction(3, second, second_recipient, 700_000),
                ]
            })
            .await;
            assert_eq!(independent.speculative_blocks, 1);
            assert_eq!(independent.speculative_transactions, 3);
            assert_eq!(independent.conflicts, 0, "{mode:?} must not conflict");
            assert_eq!(independent.reexecutions, 0);

            // Shared recipient: the two transfers touch the same balance key.
            let (_, shared) = compare_paths(mode, &[first, second], |_| {
                vec![
                    transfer_transaction(1, first, first_recipient, 500_000),
                    transaction_with_script(2, second, pure_script()),
                    transfer_transaction(3, second, first_recipient, 700_000),
                ]
            })
            .await;
            assert_eq!(shared.speculative_transactions, 3);
            assert_eq!(shared.conflicts, 1, "{mode:?} shared recipient");
            assert_eq!(shared.reexecutions, 1);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn faulting_transaction_rolls_back_identically() {
        let sender = account(0x11);
        let broke = account(0x22);
        let recipient = account(0x41);

        let (_, parallel) = compare_paths(ExecutionMode::Canonical, &[sender, broke], |_| {
            vec![
                transfer_transaction(1, sender, recipient, 1_000),
                // Funded, but the amount exceeds the balance: this run faults and
                // its overlay must be discarded exactly like the serial reset.
                transfer_transaction(2, broke, recipient, i64::MAX),
            ]
        })
        .await;

        assert_eq!(parallel.speculative_blocks, 1);
        assert_eq!(parallel.speculative_transactions, 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_and_single_transaction_blocks_skip_speculation() {
        let sender = account(0x11);
        let recipient = account(0x41);

        let (serial_empty, parallel_empty) =
            compare_paths(ExecutionMode::Canonical, &[], |_| Vec::new()).await;
        assert_eq!(serial_empty.serial_fallbacks, 0);
        assert_eq!(parallel_empty.speculative_blocks, 0);
        assert_eq!(parallel_empty.speculative_transactions, 0);
        assert_eq!(
            parallel_empty.serial_fallbacks, 1,
            "empty block stays serial"
        );
        assert!(
            parallel_empty.serial_nanos > 0,
            "the serial loop must be timed"
        );

        let (_, parallel_single) = compare_paths(ExecutionMode::Canonical, &[sender], |_| {
            vec![transfer_transaction(1, sender, recipient, 1_000)]
        })
        .await;
        assert_eq!(parallel_single.speculative_blocks, 0);
        assert_eq!(
            parallel_single.serial_fallbacks, 1,
            "one transaction stays serial"
        );
    }
}
