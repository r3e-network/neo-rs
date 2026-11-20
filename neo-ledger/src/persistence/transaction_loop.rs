//! Step 10 extension point: the per-transaction application loop.
//!
//! The canonical pipeline always executes transactions **serially** in block order
//! ([`execute_serially`]). This module exposes the loop as an inverted port so a
//! scheduler that reorders *execution* while preserving *commit order* can supply an
//! alternative implementation without the ledger knowing anything about it.
//!
//! # Determinism contract
//!
//! A [`TransactionLoopStrategy`] must produce results that are indistinguishable from
//! [`execute_serially`]:
//!
//! * the merged base snapshot after the loop is byte-identical,
//! * the transaction state token records the same per-transaction VM states,
//! * detailed receipts are emitted in block order,
//! * staged [`FaultObservation`]s carry the same hashes, categories and gas.
//!
//! The ledger does not verify those properties; it only sequences the outcome. Keeping
//! the serial loop here (instead of inside the pipeline body) means the strategy and the
//! fallback share one implementation of the canonical semantics.

use super::engine_port::{EngineFactory, NativeCacheHandle, SeededNativeContracts};
use super::error::{LedgerError, LedgerResult};
use super::evidence::{FaultCategory, FaultObservation, FaultObservationBuffer};
use super::profile::{PersistenceProfiler, PersistenceStage};
use super::receipt::ExecutionReceipt;
use neo_primitives::{UInt256, Verifiable};
use neo_protocol::execution::Block;
use neo_smart_contract::application_engine::LedgerTransactionStates;
use neo_storage::persistence::DataCache;
use neo_vm::VmState;
use std::sync::Arc;

use super::profile::StageTimer;

/// Immutable inputs of step 10.
///
/// Every field is borrowed from the surrounding pipeline body: a strategy may read
/// them freely, including from worker threads, but never owns the block, the overlay
/// or the shared native cache.
pub struct TransactionLoopContext<'a> {
    /// The block being persisted, already projected into an `Arc`.
    pub block: &'a Arc<Block>,
    /// Pre-computed transaction hashes, one per transaction, in block order.
    pub transaction_hashes: &'a [UInt256],
    /// Base overlay that accumulates the block's committed state (step 4).
    pub base_snapshot: &'a Arc<DataCache>,
    /// Native-only contract states seeded by `OnPersist` (step 8).
    pub seeded: &'a SeededNativeContracts,
    /// Shared native-contracts cache threaded from `OnPersist` (step 8).
    pub native_cache: &'a NativeCacheHandle,
    /// Whether the pipeline emits detailed receipts for this block (step 1).
    pub detailed: bool,
    /// Staging capacity available to the fault-evidence sink (step 2).
    pub fault_capacity: usize,
    /// Optional observation-only stage profiler.
    pub profiler: Option<&'a dyn PersistenceProfiler>,
}

/// Everything step 10 hands back to the pipeline.
#[derive(Default)]
pub struct TransactionLoopOutcome {
    /// The transaction state token to install on the `PostPersist` engine (step 11).
    pub transaction_states: LedgerTransactionStates,
    /// Detailed receipts in block order; empty when receipts are suppressed.
    pub receipts: Vec<ExecutionReceipt>,
    /// Fault observations in block order, already bounded by `fault_capacity`.
    pub faults: Vec<FaultObservation>,
    /// Observations the loop had to drop because the staging buffer was full.
    pub dropped_faults: u64,
}

/// Alternative step 10 implementations, e.g. a speculative parallel scheduler.
///
/// Implementations must be usable from the persistence thread only; `Send + Sync`
/// keeps the port usable behind a shared reference.
pub trait TransactionLoopStrategy: Send + Sync {
    /// Executes every transaction of `context.block`.
    ///
    /// `transaction_states` is the token taken from the `OnPersist` engine (step 9).
    /// Any `Err` aborts the whole block persist before the store commit, exactly like
    /// the serial loop.
    fn execute(
        &self,
        engines: &dyn EngineFactory,
        context: &TransactionLoopContext<'_>,
        transaction_states: LedgerTransactionStates,
    ) -> LedgerResult<TransactionLoopOutcome>;
}

/// The canonical serial loop: one shared child overlay, strictly block order.
///
/// This is the reference implementation. It is unchanged from the original inlined
/// pipeline body: `set_transaction_states`, `load_script`, `execute_allow_fault`,
/// `verify_execution`, receipt-or-VM-state, `take_transaction_states`, then dispose by
/// VM state (`HALT` merge, `FAULT` reset + observation, anything else aborts).
pub fn execute_serially(
    engines: &dyn EngineFactory,
    context: &TransactionLoopContext<'_>,
    transaction_states: LedgerTransactionStates,
) -> LedgerResult<TransactionLoopOutcome> {
    let block = context.block;
    let block_hash = block.hash();
    let block_height = block.index();
    let mut states = transaction_states;
    let mut outcome = TransactionLoopOutcome::default();

    if block.transactions.is_empty() {
        outcome.transaction_states = states;
        return Ok(outcome);
    }

    let mut fault_buffer = FaultObservationBuffer::for_available_capacity(context.fault_capacity);
    let tx_overlay = Arc::new(context.base_snapshot.clone_cache());
    for (tx, tx_hash) in block.transactions.iter().zip(context.transaction_hashes) {
        let container: Arc<dyn Verifiable> = Arc::new(tx.clone());
        let mut app = engines.open_application(
            container,
            Arc::clone(&tx_overlay),
            Arc::clone(block),
            tx.system_fee(),
            context.seeded,
            context.native_cache,
        )?;
        app.set_transaction_states(states);
        app.load_script(tx.script().to_vec())?;
        let vm_state = {
            let _stage = StageTimer::new(context.profiler, PersistenceStage::TransactionVm);
            app.execute_allow_fault()
        };
        app.verify_execution()?;

        if context.detailed {
            outcome.receipts.push(app.execution_receipt());
        } else {
            // Record the VM state unconditionally even without a detailed receipt.
            app.record_transaction_vm_state(tx_hash, vm_state);
        }
        states = app.take_transaction_states();

        match vm_state {
            // Step 10a — HALT: merge the child overlay into the base snapshot.
            VmState::HALT => context.base_snapshot.merge_from_child(&tx_overlay),
            // Step 10b — FAULT: reset the overlay and stage a bounded observation.
            VmState::FAULT => {
                tx_overlay.reset();
                let category = if app.fault_exception().is_some() {
                    FaultCategory::CaughtVmException
                } else {
                    FaultCategory::VmFault
                };
                fault_buffer.push(FaultObservation {
                    transaction_hash: *tx_hash,
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
                    transaction_hash: *tx_hash,
                    vm_state: other,
                });
            }
        }
    }

    outcome.transaction_states = states;
    outcome.faults = fault_buffer.observations;
    outcome.dropped_faults = fault_buffer.dropped;
    Ok(outcome)
}
