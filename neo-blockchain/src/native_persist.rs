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
//! nothing lands in the store. [`persist_block_natives`] mirrors that
//! with a child [`DataCache`] (`snapshot.clone_cache()`): all three
//! stages write into the child, and only a fully successful sequence
//! commits it into the caller's snapshot. On any error the child is
//! dropped, so observers of the caller's snapshot (e.g. the genesis
//! re-init guard [`chain_state_initialized`]) can never see partial
//! block state. The per-transaction engines add the inner C# layer:
//! `clonedSnapshot = snapshot.CloneCache()` per transaction,
//! committed into the block cache only when the script HALTs.
//!
//! The per-stage native hooks are driven directly off the installed
//! global provider (see [`run_native_persist_hooks`]) rather than via
//! `ApplicationEngine::native_on_persist`/`native_post_persist`: those
//! engine functions run the identical loop, but over the engine's
//! *local* `NativeRegistry`, which every constructor builds empty and
//! which has no population API — they would silently no-op. The direct
//! loop uses the same contracts in the same canonical order with the
//! same `is_active` filter against the same engine, so the observable
//! behavior (storage writes, notifications, ordering) is identical to
//! C#'s `NativeContract.OnPersistAsync(engine)` dispatch. When
//! neo-execution grows a registry-population seam, this should switch
//! to the engine functions.
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
//! records C# writes there come from [`crate::ledger_records`] when the
//! direct native hook loop reaches the Ledger contract's canonical slot.
//!
//! ## Remaining gaps (documented)
//!
//! - C# `GasToken.OnPersist` burns each transaction's system+network
//!   fee from its sender and mints the network fees to the primary;
//!   the Rust `GasToken` does not override `on_persist` yet, so fee
//!   burn/mint records are absent until `neo-native-contracts` grows
//!   that hook.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_manifest::CallFlags;
use neo_payloads::ApplicationExecuted;
use neo_payloads::{Block, Header, Witness};
use neo_primitives::{TriggerType, UInt160, UInt256, Verifiable};
use neo_storage::DataCache;
use neo_storage::StorageKey;
use neo_storage::persistence::SeekDirection;
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use neo_vm_rs::VmState as VMState;

/// C# genesis timestamp: `2016-07-15T15:08:21Z` in Unix milliseconds.
const GENESIS_TIMESTAMP_MS: u64 = 1_468_595_301_000;
/// C# genesis nonce — the nonce of the Bitcoin genesis block.
const GENESIS_NONCE: u64 = 2_083_236_893;
/// `LedgerContract` native id (a fixed protocol constant, C# id -4).
/// Hardcoded because the blockchain crate reaches natives only through
/// the type-erased provider seam; pinned against the real constant by
/// a dev-dependency test.
const LEDGER_CONTRACT_ID: i32 = -4;
/// C# `LedgerContract.Prefix_Block` (5): trimmed-block records by hash.
const LEDGER_PREFIX_BLOCK: u8 = 5;
/// `NeoToken` native id (a fixed protocol constant, C# id -5).
const NEO_TOKEN_ID: i32 = -5;
/// C# `NeoToken.Prefix_Committee` (14): the cached-committee record —
/// the first key genesis initialization writes.
const NEO_PREFIX_COMMITTEE_KEY: u8 = 14;

/// A notification emitted by a native persistence engine, captured for
/// the caller (C# wraps these in `ApplicationExecuted` events).
#[derive(Debug, Clone)]
pub struct NativePersistNotification {
    /// The contract that emitted the notification.
    pub script_hash: UInt160,
    /// The event name (e.g. `Transfer`, `CommitteeChanged`).
    pub event_name: String,
    /// The event arguments.
    pub state: Vec<StackItem>,
}

/// Outcome of [`persist_block_natives`] for one block.
#[derive(Debug, Clone, Default)]
pub struct NativePersistOutcome {
    /// Names of the native contracts whose `initialize()` ran at this
    /// block (their activation block is this block).
    pub initialized: Vec<String>,
    /// Per-engine execution records, in C# `Blockchain.Persist` order:
    /// the `OnPersist` engine, one entry per block transaction, then
    /// the `PostPersist` engine (C# `allApplicationExecuted`).
    pub application_executed: Vec<ApplicationExecuted>,
    /// Notifications emitted by the `OnPersist` native hooks.
    pub on_persist_notifications: Vec<NativePersistNotification>,
    /// Notifications emitted by the `PostPersist` native hooks.
    pub post_persist_notifications: Vec<NativePersistNotification>,
}

/// C# `NativeContract.Ledger.Initialized(snapshot)` (LedgerContract.cs:91):
/// whether the chain state has been bootstrapped, i.e. the genesis block
/// has been persisted. The first probe is the literal C# check (any
/// `LedgerContract` `Prefix_Block` record, written by the persist
/// pipeline via [`crate::ledger_records`]); the second probes the
/// `NeoToken` committee cache — a key genesis initialization always
/// seeds and that can never be deleted afterwards — which keeps stores
/// persisted before the ledger records landed reporting initialized.
pub fn chain_state_initialized(snapshot: &DataCache) -> bool {
    let block_prefix = StorageKey::new(LEDGER_CONTRACT_ID, vec![LEDGER_PREFIX_BLOCK]);
    if snapshot
        .find(Some(&block_prefix), SeekDirection::Forward)
        .next()
        .is_some()
    {
        return true;
    }
    snapshot
        .get(&StorageKey::new(
            NEO_TOKEN_ID,
            vec![NEO_PREFIX_COMMITTEE_KEY],
        ))
        .is_some()
}

/// C# `NeoSystem.CreateGenesisBlock(settings)`: index 0, zero
/// previous/merkle hashes, the 2016-07-15T15:08:21Z timestamp, the
/// Bitcoin-genesis nonce, primary index 0, `NextConsensus` set to the
/// BFT address of the standby validators, and an empty-invocation
/// `PUSH1` witness. The genesis block carries no transactions.
pub fn genesis_block(settings: &ProtocolSettings) -> CoreResult<Block> {
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_timestamp(GENESIS_TIMESTAMP_MS);
    header.set_nonce(GENESIS_NONCE);
    header.set_index(0);
    header.set_primary_index(0);
    header.set_next_consensus(bft_address(&settings.standby_validators())?);
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    Ok(Block::from_parts(header, Vec::new()))
}

/// C# `Contract.GetBFTAddress(pubkeys)`: the script hash of the
/// C# `Contract.GetBFTAddress(pubkeys)`: the `m = n - (n - 1) / 3` multisig
/// script hash. Delegates to the single workspace implementation.
pub(crate) fn bft_address(pubkeys: &[neo_crypto::ECPoint]) -> CoreResult<UInt160> {
    neo_vm::script_builder::RedeemScript::bft_address(pubkeys).ok_or_else(|| {
        CoreError::invalid_operation("BFT address requires at least one validator")
    })
}

/// Runs the per-block native hook matching `engine`'s trigger
/// (`on_persist` for [`TriggerType::OnPersist`], `post_persist` for
/// [`TriggerType::PostPersist`]) for every contract in `contracts` that
/// is active at `block_index`, in the given (canonical registration)
/// order — the exact body of C#'s `System.Contract.NativeOnPersist` /
/// `NativePostPersist` syscalls (`NativeContract.OnPersistAsync` /
/// `PostPersistAsync` over `Contracts.Where(IsActive)`). A hook error
/// aborts the block, like the C# native script faulting.
///
/// See the module docs for why this loop runs here instead of through
/// `ApplicationEngine::native_on_persist`/`native_post_persist`.
fn run_native_persist_hooks(
    contracts: &[Arc<dyn neo_execution::NativeContract>],
    engine: &mut ApplicationEngine,
    settings: &ProtocolSettings,
    block: &Block,
    block_hash: &UInt256,
    block_index: u32,
) -> CoreResult<()> {
    let trigger = engine.trigger_type();
    for contract in contracts {
        if !contract.is_active(settings, block_index) {
            continue;
        }
        if contract.id() == LEDGER_CONTRACT_ID {
            let snapshot = engine.snapshot_cache();
            match trigger {
                TriggerType::OnPersist => {
                    crate::ledger_records::LedgerRecords::write_on_persist_records(
                        &snapshot, block, block_hash,
                    )?;
                }
                TriggerType::PostPersist => {
                    crate::ledger_records::LedgerRecords::write_post_persist_record(
                        &snapshot,
                        block_hash,
                        block_index,
                    )?;
                }
                _ => {}
            }
        }
        let result = match trigger {
            TriggerType::OnPersist => contract.on_persist(engine),
            TriggerType::PostPersist => contract.post_persist(engine),
            other => {
                return Err(CoreError::invalid_operation(format!(
                    "native persist hooks require an OnPersist/PostPersist engine, got {other:?}"
                )));
            }
        };
        result.map_err(|e| {
            CoreError::invalid_operation(format!(
                "native {} {trigger:?} hook failed at block {block_index}: {e}",
                contract.name()
            ))
        })?;
    }
    Ok(())
}

/// Runs the C# `Blockchain.Persist` sequence for `block` against
/// `snapshot`: native `OnPersist` (with activation-block
/// initialization and the LedgerContract block/transaction records),
/// per-transaction `Application` execution (gas = the transaction's
/// system fee, per-tx child cache committed on HALT), and native
/// `PostPersist` (with the LedgerContract current-block pointer).
///
/// The whole sequence is staged in a child cache over `snapshot` and
/// committed into it only when every stage succeeds, mirroring C#'s
/// single `snapshot.Commit()` at the end of `Persist` (see the module
/// docs). Committing `snapshot` itself to the backing store remains
/// the caller's responsibility.
///
/// Requires the global native-contract provider to be installed
/// (`neo_native_contracts::install()`), like every engine-based
/// execution path.
pub fn persist_block_natives(
    snapshot: Arc<DataCache>,
    block: Arc<Block>,
    settings: &ProtocolSettings,
) -> CoreResult<NativePersistOutcome> {
    let provider = NativeContractLookup::native_contract_provider().ok_or_else(|| {
        CoreError::invalid_operation(
            "persist_block_natives requires the native-contract provider \
             (call neo_native_contracts::install() at startup)",
        )
    })?;
    let block_index = block.index();
    let block_hash = block
        .header
        .try_hash()
        .map_err(|e| CoreError::invalid_operation(format!("persist: block hash: {e}")))?;
    let contracts = provider.all_native_contracts();
    let mut outcome = NativePersistOutcome::default();

    // Per-block atomicity: stage the whole sequence in a child cache
    // over the caller's snapshot (C# `using var snapshot = …` with the
    // final `snapshot.Commit()`); only a fully successful sequence is
    // merged back, a fault drops every staged write.
    let block_cache = Arc::new(snapshot.clone_cache());

    // --- OnPersist stage (C# TriggerType.OnPersist engine, gas 0) ---
    let mut engine = ApplicationEngine::new_with_shared_block(
        TriggerType::OnPersist,
        None,
        Arc::clone(&block_cache),
        Some(Arc::clone(&block)),
        settings.clone(),
        0,
        None,
    )?;

    // Record which activation initializers will run inside
    // ContractManagement.OnPersist. The execution itself must stay there so
    // deploy records, InitializeAsync side-effects, and Deploy notifications
    // retain the exact C# order.
    for contract in &contracts {
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
        &contracts,
        &mut engine,
        settings,
        &block,
        &block_hash,
        block_index,
    )?;
    outcome.on_persist_notifications = collect_notifications(&engine);
    outcome
        .application_executed
        .push(application_executed(&engine, None, VMState::HALT));
    drop(engine);

    // --- Transaction stage (C# Blockchain.Persist:433-453) ---
    // Each transaction runs in its own Application-trigger engine with
    // gas limit = tx.SystemFee over a child cache of the block cache
    // (C# `clonedSnapshot = snapshot.CloneCache()`): HALT commits the
    // child into the block cache, FAULT discards it. Either way the
    // transaction's ledger record is rewritten with the final VM state
    // (C# mutates the TransactionState stored by Ledger.OnPersist).
    for tx in &block.transactions {
        let tx_hash = tx
            .try_hash()
            .map_err(|e| CoreError::invalid_operation(format!("persist: tx hash: {e}")))?;
        let tx_cache = Arc::new(block_cache.clone_cache());
        let container: Arc<dyn Verifiable> = Arc::new(tx.clone());
        let mut engine = ApplicationEngine::new_with_shared_block(
            TriggerType::Application,
            Some(container),
            Arc::clone(&tx_cache),
            Some(Arc::clone(&block)),
            settings.clone(),
            tx.system_fee(),
            None,
        )?;
        // C# loads the script unchecked and lets execution FAULT on a
        // bad instruction; a Rust load error therefore faults the
        // transaction, never the block.
        let (vm_state, load_error) =
            match engine.load_script(tx.script().to_vec(), CallFlags::ALL, None) {
                Ok(()) => (engine.execute_allow_fault(), None),
                Err(error) => (VMState::FAULT, Some(error.to_string())),
            };
        let mut executed = application_executed(&engine, Some(tx.clone()), vm_state);
        if executed.exception.is_none() {
            executed.exception = load_error;
        }
        outcome.application_executed.push(executed);
        drop(engine);

        if vm_state == VMState::HALT {
            tx_cache.commit();
        }
        crate::ledger_records::LedgerRecords::update_transaction_vm_state(
            &block_cache,
            block_index,
            tx,
            &tx_hash,
            vm_state,
        )?;
    }

    // --- PostPersist stage (C# TriggerType.PostPersist engine, gas 0) ---
    let mut engine = ApplicationEngine::new_with_shared_block(
        TriggerType::PostPersist,
        None,
        Arc::clone(&block_cache),
        Some(Arc::clone(&block)),
        settings.clone(),
        0,
        None,
    )?;
    run_native_persist_hooks(
        &contracts,
        &mut engine,
        settings,
        &block,
        &block_hash,
        block_index,
    )?;
    outcome.post_persist_notifications = collect_notifications(&engine);
    outcome
        .application_executed
        .push(application_executed(&engine, None, VMState::HALT));
    drop(engine);

    // The whole sequence succeeded: merge the staged writes into the
    // caller's snapshot (C# `snapshot.Commit()`).
    block_cache.commit();

    Ok(outcome)
}

/// Builds the C# `ApplicationExecuted` record for a finished engine.
/// `GasConsumed` is the datoshi fee (C# `engine.FeeConsumed`), the
/// stack is the engine's result stack, and the notifications/logs are
/// the engine's captured events.
fn application_executed(
    engine: &ApplicationEngine,
    transaction: Option<neo_payloads::Transaction>,
    vm_state: VMState,
) -> ApplicationExecuted {
    let mut executed = ApplicationExecuted::new(
        transaction,
        engine.trigger_type(),
        vm_state,
        engine.fault_exception().map(str::to_owned),
        engine.fee_consumed(),
        engine
            .result_stack()
            .iter()
            .map(stack_value_snapshot)
            .collect(),
    );
    executed.notifications = engine.notifications().to_vec();
    executed.logs = engine.logs().to_vec();
    executed
}

fn stack_value_snapshot(item: &StackItem) -> StackValue {
    match item {
        StackItem::Null => StackValue::Null,
        StackItem::Boolean(value) => StackValue::Boolean(*value),
        StackItem::Integer(value) => match value.to_i64() {
            Some(value) => StackValue::Integer(value),
            None => StackValue::BigInteger(value.to_signed_bytes_le()),
        },
        StackItem::ByteString(bytes) => StackValue::ByteString(bytes.clone()),
        StackItem::Buffer(buffer) => StackValue::Buffer(0, buffer.data()),
        StackItem::Array(array) => StackValue::Array(
            0,
            array
                .iter()
                .map(|item| stack_value_snapshot(&item))
                .collect(),
        ),
        StackItem::Struct(structure) => StackValue::Struct(
            0,
            structure
                .iter()
                .map(|item| stack_value_snapshot(&item))
                .collect(),
        ),
        StackItem::Map(map) => StackValue::Map(
            0,
            map.iter()
                .map(|(key, value)| (stack_value_snapshot(&key), stack_value_snapshot(&value)))
                .collect(),
        ),
        StackItem::Pointer(pointer) => {
            StackValue::Pointer(i64::try_from(pointer.position()).unwrap_or(i64::MAX))
        }
        StackItem::InteropInterface(_) => StackValue::Interop(0),
    }
}

/// Copies the engine's emitted notifications into the outcome shape.
fn collect_notifications(engine: &ApplicationEngine) -> Vec<NativePersistNotification> {
    engine
        .notifications()
        .iter()
        .map(|event| NativePersistNotification {
            script_hash: event.script_hash,
            event_name: event.event_name.clone(),
            state: event.state.clone(),
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/native_persist.rs"]
mod tests;
