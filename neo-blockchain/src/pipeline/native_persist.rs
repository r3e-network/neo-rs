//! Native-contract block-persistence pipeline.
//!
//! Replicates C# `Blockchain.Persist`
//! (`neo_csharp/src/Neo/Ledger/Blockchain.cs:410`): every persisted
//! block runs an `OnPersist`-trigger [`ApplicationEngine`] over the
//! block snapshot executing `System.Contract.NativeOnPersist`, then
//! executes each block transaction in its own `Application`-trigger
//! engine (gas limit = the transaction's system fee) over a per-tx
//! child cache that commits on HALT and is discarded on FAULT, then a
//! `PostPersist`-trigger engine executing
//! `System.Contract.NativePostPersist`. The native scripts must HALT
//! (an error there aborts the block).
//!
//! ## Per-block atomicity
//!
//! C# stages every write of the sequence in one `snapshot =
//! system.GetSnapshotCache()` and calls `snapshot.Commit()` only after
//! the whole sequence succeeds; a throw disposes the snapshot and
//! nothing lands in the store. [`persist_block_natives_with_resources`]
//! mirrors that with a child [`DataCache`] (`snapshot.clone_cache()`): all
//! three stages write into the child, and only a fully successful sequence
//! commits it into the caller's snapshot. On any error the child is dropped,
//! so observers of the caller's snapshot (e.g. the genesis re-init guard
//! [`chain_state_initialized`]) can never see partial block state. The
//! per-transaction engines add the inner C# layer: `clonedSnapshot =
//! snapshot.CloneCache()` per transaction, committed into the block cache only
//! when the script HALTs.
//!
//! The per-stage native hooks are driven from a [`NativePersistResources`]
//! value that captures one explicit native-contract provider plus the canonical
//! native contract list for the whole block batch. This keeps bulk sync,
//! genesis initialization, and live block import stable even if a compatibility
//! caller replaces the process-global provider later. The hook loop deliberately
//! does not call `ApplicationEngine::native_on_persist`/`native_post_persist`:
//! those engine functions iterate the engine's local `NativeRegistry`, which is
//! still empty for these constructors. The direct loop uses the same contracts
//! in the same canonical order with the same `is_active` filter against the
//! same engine, so storage writes, notifications, and ordering stay aligned
//! with C#'s `NativeContract.OnPersistAsync(engine)` dispatch. When
//! `neo-execution` grows a registry-population seam, this can switch to the
//! engine functions without reintroducing ambient provider lookup.
//!
//! In C# the native *deployment + initialization* (committee cache,
//! genesis NEO/GAS mints, Oracle price, …) happens inside
//! `ContractManagement.OnPersistAsync`, which calls
//! `contract.InitializeAsync(engine, hardfork)` for every native whose
//! activation block is being persisted. Rust keeps the same observable
//! ordering in `ContractManagement::on_persist`.
//!
//! The Rust `LedgerContract::on_persist`/`post_persist` hooks are
//! read-only no-ops to avoid a crate cycle, so the block/transaction
//! records C# writes there come from `crate::ledger_records` when the
//! direct native hook loop reaches the Ledger contract's canonical slot.
//!
//! `GasToken::on_persist` lives in `neo-native-contracts` and is invoked by
//! this pipeline through the shared native-contract provider. It burns each
//! transaction's system+network fee from the sender and mints the block's
//! network-fee total to the primary validator, matching C# `GasToken.OnPersist`.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_execution::{ApplicationEngine, NativeContract, NoDiagnostic};
use neo_payloads::Block;
use neo_primitives::TriggerType;
use neo_storage::{CacheRead, DataCache};
use neo_vm::VmState as VMState;

mod artifacts;
mod genesis;
mod hooks;
mod metrics;
mod trace;
mod transactions;
mod types;

pub use artifacts::NativePersistNotification;
use artifacts::{application_executed, collect_notifications};
pub(crate) use genesis::{LEDGER_CONTRACT_ID, bft_address};
#[cfg(test)]
pub(crate) use genesis::{LEDGER_PREFIX_BLOCK, NEO_PREFIX_COMMITTEE_KEY, NEO_TOKEN_ID};
pub use genesis::{chain_state_initialized, genesis_block};
use hooks::run_native_persist_hooks;
use transactions::run_transaction_stage;
pub use types::{
    NativePersistOptions, NativePersistOutcome, NativePersistResources, StagedNativePersist,
};

#[cfg(test)]
use neo_vm::StackItem;
#[cfg(test)]
use neo_vm::StackValue;

/// Runs the C# `Blockchain.Persist` sequence for `block` against
/// `snapshot`: native `OnPersist` (with activation-block
/// initialization and the LedgerContract block/transaction records),
/// per-transaction `Application` execution (gas = the transaction's
/// system fee, per-tx child cache committed on HALT), and native
/// `PostPersist` (with the LedgerContract current-block pointer).
///
/// Runs the C# `Blockchain.Persist` sequence with caller-provided reusable
/// native resources and commits the staged writes on success.
pub fn persist_block_natives_with_resources<P, B>(
    snapshot: Arc<DataCache<B>>,
    block: Arc<Block>,
    settings: Arc<ProtocolSettings>,
    options: NativePersistOptions,
    resources: &NativePersistResources<P>,
) -> CoreResult<NativePersistOutcome>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
    let staged = stage_block_natives_with_resources(snapshot, block, settings, options, resources)?;
    let outcome = staged.outcome.clone();
    staged.commit();
    Ok(outcome)
}

/// Runs native block persistence with caller-provided reusable resources and
/// composition-shared protocol settings.
pub fn stage_block_natives_with_resources<P, B>(
    snapshot: Arc<DataCache<B>>,
    block: Arc<Block>,
    settings: Arc<ProtocolSettings>,
    options: NativePersistOptions,
    resources: &NativePersistResources<P>,
) -> CoreResult<StagedNativePersist<B>>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
    let total_start = std::time::Instant::now();
    let block_index = block.index();
    let block_hash = block
        .header
        .try_hash()
        .map_err(|e| CoreError::invalid_operation(format!("persist: block hash: {e}")))?;
    let contracts = resources.contracts.as_ref();
    let mut outcome = NativePersistOutcome::default();

    // Per-block atomicity: stage the whole sequence in a child cache
    // over the caller's snapshot (C# `using var snapshot = …` with the
    // final `snapshot.Commit()`); only a fully successful sequence is
    // merged back, a fault drops every staged write.
    let block_cache = Arc::new(snapshot.clone_cache());

    // --- OnPersist stage (C# TriggerType.OnPersist engine, gas 0) ---
    let onpersist_start = std::time::Instant::now();
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::OnPersist,
        None,
        Arc::clone(&block_cache),
        Some(Arc::clone(&block)),
        Arc::clone(&settings),
        0,
        NoDiagnostic,
        resources.provider(),
    )?;

    // Record which activation initializers will run inside
    // ContractManagement.OnPersist. The execution itself must stay there so
    // deploy records, InitializeAsync side-effects, and Deploy notifications
    // retain the exact C# order.
    for contract in contracts {
        let activation_height = match contract.active_in() {
            None => 0,
            Some(hardfork) => match settings.hardforks.get(&hardfork) {
                Some(&height) => height,
                None => continue,
            },
        };
        if activation_height == block_index {
            outcome.initialized.push(contract.name().to_string());
        }
    }

    run_native_persist_hooks(
        contracts,
        &mut engine,
        settings.as_ref(),
        &block,
        &block_hash,
        block_index,
    )?;
    if options.capture_replay_artifacts {
        outcome.on_persist_notifications = collect_notifications(&engine);
        outcome
            .application_executed
            .push(application_executed(&engine, None, VMState::HALT));
    }
    let native_contract_cache = engine.native_contract_cache_handle();
    drop(engine);
    let onpersist_us = neo_runtime::time::elapsed_us(onpersist_start.elapsed());

    // --- Transaction stage (C# Blockchain.Persist:433-453) ---
    // Each transaction runs in its own Application-trigger engine with
    // gas limit = tx.SystemFee over a child cache of the block cache
    // (C# `clonedSnapshot = snapshot.CloneCache()`): HALT commits the
    // child into the block cache, FAULT discards it. Either way the
    // transaction's ledger record is rewritten with the final VM state
    // (C# mutates the TransactionState stored by Ledger.OnPersist).
    let tx_us = run_transaction_stage(
        &block_cache,
        &block,
        &settings,
        options,
        resources,
        native_contract_cache,
        &mut outcome,
    )?;

    // --- PostPersist stage (C# TriggerType.PostPersist engine, gas 0) ---
    let postpersist_start = std::time::Instant::now();
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::PostPersist,
        None,
        Arc::clone(&block_cache),
        Some(Arc::clone(&block)),
        Arc::clone(&settings),
        0,
        NoDiagnostic,
        resources.provider(),
    )?;
    run_native_persist_hooks(
        contracts,
        &mut engine,
        settings.as_ref(),
        &block,
        &block_hash,
        block_index,
    )?;
    if options.capture_replay_artifacts {
        outcome.post_persist_notifications = collect_notifications(&engine);
        outcome
            .application_executed
            .push(application_executed(&engine, None, VMState::HALT));
    }
    drop(engine);
    let postpersist_us = neo_runtime::time::elapsed_us(postpersist_start.elapsed());

    Ok(StagedNativePersist {
        snapshot: block_cache,
        outcome,
        block_index,
        n_tx: block.transactions.len(),
        onpersist_us,
        tx_us,
        postpersist_us,
        total_start,
    })
}

#[cfg(test)]
#[path = "../tests/pipeline/native_persist.rs"]
mod tests;
