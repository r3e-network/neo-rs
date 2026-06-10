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
//! activation block is being persisted. The Rust
//! `ContractManagement::on_persist` hook does not perform that yet, so
//! [`persist_block_natives`] runs the equivalent
//! [`NativeContract::initialize`] pass itself, immediately before the
//! `OnPersist` hooks — the same observable order as C#, where
//! ContractManagement is the first native in the OnPersist sequence.
//! Similarly, the Rust `LedgerContract::on_persist`/`post_persist`
//! hooks are read-only no-ops, so the block/transaction records C#
//! writes there come from [`crate::ledger_records`] at the same stage
//! boundaries.
//!
//! ## Remaining gaps (documented)
//!
//! - The native deploy *records* (`ContractManagement` `Prefix_Contract`
//!   entries + `Deploy` notifications) are ContractManagement's job and
//!   are not written here.
//! - C# `GasToken.OnPersist` burns each transaction's system+network
//!   fee from its sender and mints the network fees to the primary;
//!   the Rust `GasToken` does not override `on_persist` yet, so fee
//!   burn/mint records are absent until `neo-native-contracts` grows
//!   that hook.

use std::sync::Arc;

use neo_block::ApplicationExecuted;
use neo_config::ProtocolSettings;
use neo_data_cache::DataCache;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::native_contract_provider;
use neo_execution::ApplicationEngine;
use neo_manifest::CallFlags;
use neo_payloads::{Block, Header, Witness};
use neo_primitives::{TriggerType, UInt160, UInt256, Verifiable};
use neo_storage::persistence::SeekDirection;
use neo_storage::StorageKey;
use neo_vm::StackItem;
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
        .get(&StorageKey::new(NEO_TOKEN_ID, vec![NEO_PREFIX_COMMITTEE_KEY]))
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
    header.witness =
        Witness::new_with_scripts(Vec::new(), vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    Ok(Block::from_parts(header, Vec::new()))
}

/// C# `Contract.GetBFTAddress(pubkeys)`: the script hash of the
/// `m`-of-`n` multisig over `pubkeys` with `m = n - (n - 1) / 3`.
fn bft_address(pubkeys: &[neo_crypto::ECPoint]) -> CoreResult<UInt160> {
    if pubkeys.is_empty() {
        return Err(CoreError::invalid_operation(
            "BFT address requires at least one validator",
        ));
    }
    let m = pubkeys.len() - (pubkeys.len() - 1) / 3;
    let script = neo_redeem_script::multi_sig_redeem_script_from_points(m, pubkeys)
        .map_err(|e| CoreError::invalid_operation(format!("BFT multisig script: {e}")))?;
    Ok(UInt160::from_script(&script))
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
    block_index: u32,
) -> CoreResult<()> {
    let trigger = engine.trigger_type();
    for contract in contracts {
        if !contract.is_active(settings, block_index) {
            continue;
        }
        let result = match trigger {
            TriggerType::OnPersist => contract.on_persist(engine),
            TriggerType::PostPersist => contract.post_persist(engine),
            other => {
                return Err(CoreError::invalid_operation(format!(
                    "native persist hooks require an OnPersist/PostPersist engine, got {other:?}"
                )))
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
    let provider = native_contract_provider().ok_or_else(|| {
        CoreError::invalid_operation(
            "persist_block_natives requires the native-contract provider \
             (call neo_native_contracts::install() at startup)",
        )
    })?;
    let block_index = block.index();
    let block_hash = block.header.try_hash().map_err(|e| {
        CoreError::invalid_operation(format!("persist: block hash: {e}"))
    })?;
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

    // Native deployment-block initialization (C# ContractManagement
    // .OnPersistAsync calling `InitializeAsync(engine, hardfork)` with
    // `hardfork == ActiveIn`): run `initialize()` for every native
    // whose activation block is this block. Genesis-active natives
    // (ActiveIn == None) initialize at block 0; hardfork-activated
    // natives initialize at their scheduled activation height. An
    // UNCONFIGURED ActiveIn hardfork never initializes: C#
    // IsInitializeBlock skips unconfigured hardforks ("treated as
    // disabled"), even though C# IsActive treats the same contract as
    // genesis-active — such a native runs its (no-op) hooks but is
    // never deployed/initialized, and the Rust pipeline matches that.
    //
    // `NativeContract::is_initialize_block` is deliberately NOT used
    // here: it also fires on later method-hardfork blocks (C# passes
    // those hardforks to `InitializeAsync(engine, hf)`, which is a
    // no-op unless `hf == ActiveIn`), but the parameterless Rust
    // `initialize()` hook only models the `hf == ActiveIn` seeding —
    // re-running it on such blocks would wrongly re-seed genesis state.
    for contract in &contracts {
        let activation_height = match contract.active_in() {
            None => 0,
            Some(hardfork) => match settings.hardforks.get(&hardfork) {
                Some(&height) => height,
                None => continue,
            },
        };
        if activation_height == block_index {
            contract.initialize(&mut engine).map_err(|e| {
                CoreError::invalid_operation(format!(
                    "native initialize for {} at block {block_index} failed: {e}",
                    contract.name()
                ))
            })?;
            outcome.initialized.push(contract.name().to_string());
        }
    }

    // LedgerContract.OnPersistAsync record writes (the Rust ledger
    // hook is a read-only no-op; see `ledger_records`): the block-hash
    // index entry, the trimmed block, the per-transaction records
    // (VMState::NONE until executed), and the conflict stubs.
    crate::ledger_records::write_on_persist_records(&block_cache, &block, &block_hash)?;

    run_native_persist_hooks(&contracts, &mut engine, settings, block_index)?;
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
        let tx_hash = tx.try_hash().map_err(|e| {
            CoreError::invalid_operation(format!("persist: tx hash: {e}"))
        })?;
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
        crate::ledger_records::update_transaction_vm_state(
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
        Some(block),
        settings.clone(),
        0,
        None,
    )?;
    run_native_persist_hooks(&contracts, &mut engine, settings, block_index)?;
    // LedgerContract.PostPersistAsync: the current-block pointer.
    crate::ledger_records::write_post_persist_record(&block_cache, &block_hash, block_index)?;
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
        engine.result_stack().iter().cloned().collect(),
    );
    executed.notifications = engine.notifications().to_vec();
    executed.logs = engine.logs().to_vec();
    executed
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
mod tests {
    use super::*;
    use neo_serialization::BinarySerializer;
    use neo_storage::StorageKey;
    use neo_vm_rs::ExecutionEngineLimits;
    use num_bigint::BigInt;

    /// NEO `Prefix_Committee` (C# NeoToken).
    const NEO_PREFIX_COMMITTEE: u8 = 14;
    /// NEO `Prefix_VotersCount`.
    const NEO_PREFIX_VOTERS_COUNT: u8 = 1;
    /// NEO `Prefix_GasPerBlock`.
    const NEO_PREFIX_GAS_PER_BLOCK: u8 = 29;
    /// NEO `Prefix_RegisterPrice`.
    const NEO_PREFIX_REGISTER_PRICE: u8 = 13;
    /// Shared NEP-17 `Prefix_Account` / `Prefix_TotalSupply`.
    const NEP17_PREFIX_ACCOUNT: u8 = 20;
    const NEP17_PREFIX_TOTAL_SUPPLY: u8 = 11;
    /// Oracle `Prefix_Price` / `Prefix_RequestId`.
    const ORACLE_PREFIX_PRICE: u8 = 5;
    const ORACLE_PREFIX_REQUEST_ID: u8 = 9;

    fn neo_id() -> i32 {
        neo_native_contracts::NeoToken::ID
    }

    fn get(snapshot: &DataCache, id: i32, key: Vec<u8>) -> Option<Vec<u8>> {
        snapshot
            .get(&StorageKey::new(id, key))
            .map(|item| item.value_bytes().into_owned())
    }

    #[test]
    fn genesis_block_matches_csharp_create_genesis_block() {
        let settings = ProtocolSettings::default();
        let block = genesis_block(&settings).expect("genesis block");
        assert_eq!(block.index(), 0);
        assert_eq!(block.header.version(), 0);
        assert_eq!(*block.header.prev_hash(), UInt256::zero());
        assert_eq!(*block.header.merkle_root(), UInt256::zero());
        assert_eq!(block.header.timestamp(), 1_468_595_301_000);
        assert_eq!(block.header.nonce(), 2_083_236_893);
        assert_eq!(block.header.primary_index(), 0);
        assert!(block.transactions.is_empty());
        // NextConsensus = BFT address (m = n - (n-1)/3) of the standby validators.
        let validators = settings.standby_validators();
        let m = validators.len() - (validators.len() - 1) / 3;
        let script =
            neo_redeem_script::multi_sig_redeem_script_from_points(m, &validators).unwrap();
        assert_eq!(*block.header.next_consensus(), UInt160::from_script(&script));
        // Witness: empty invocation, PUSH1 verification.
        assert!(block.header.witness.invocation_script().is_empty());
        assert_eq!(
            block.header.witness.verification_script(),
            &[neo_vm_rs::OpCode::PUSH1.byte()]
        );
    }

    #[test]
    fn genesis_persist_seeds_native_state_and_mints() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        let block = Arc::new(genesis_block(&settings).expect("genesis block"));

        let outcome =
            persist_block_natives(Arc::clone(&snapshot), block, &settings).expect("genesis persist");

        // Genesis-active natives initialized (NeoToken + OracleContract among them).
        assert!(outcome.initialized.iter().any(|n| n == "NeoToken"));
        assert!(outcome.initialized.iter().any(|n| n == "OracleContract"));
        // C# allApplicationExecuted for an empty block: the OnPersist
        // engine and the PostPersist engine.
        assert_eq!(outcome.application_executed.len(), 2);
        assert_eq!(
            outcome.application_executed[0].trigger,
            neo_primitives::TriggerType::OnPersist
        );
        assert_eq!(
            outcome.application_executed[1].trigger,
            neo_primitives::TriggerType::PostPersist
        );

        // --- NeoToken.Initialize seeds (byte-exact) ---
        // Committee cache: Array of Struct[pubkey, 0] in standby order.
        let expected_committee = StackItem::from_array(
            settings
                .standby_committee
                .iter()
                .map(|p| {
                    StackItem::from_struct(vec![
                        StackItem::from_byte_string(p.to_bytes()),
                        StackItem::from_int(BigInt::from(0)),
                    ])
                })
                .collect::<Vec<_>>(),
        );
        let expected_committee_bytes =
            BinarySerializer::serialize(&expected_committee, &ExecutionEngineLimits::default())
                .unwrap();
        assert_eq!(
            get(&snapshot, neo_id(), vec![NEO_PREFIX_COMMITTEE]),
            Some(expected_committee_bytes)
        );
        // Voters count: BigInteger zero = empty bytes.
        assert_eq!(get(&snapshot, neo_id(), vec![NEO_PREFIX_VOTERS_COUNT]), Some(Vec::new()));
        // gasPerBlock record at big-endian index 0 = 5 GAS.
        let mut gpb_key = vec![NEO_PREFIX_GAS_PER_BLOCK];
        gpb_key.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            get(&snapshot, neo_id(), gpb_key),
            Some(BigInt::from(500_000_000i64).to_signed_bytes_le())
        );
        // registerPrice = 1000 GAS.
        assert_eq!(
            get(&snapshot, neo_id(), vec![NEO_PREFIX_REGISTER_PRICE]),
            Some(BigInt::from(100_000_000_000i64).to_signed_bytes_le())
        );

        // --- The genesis NEO mint: 100M NEO to the standby-validator BFT address ---
        let bft = bft_address(&settings.standby_validators()).unwrap();
        let mut account_key = vec![NEP17_PREFIX_ACCOUNT];
        account_key.extend_from_slice(&bft.to_bytes());
        let expected_account = StackItem::from_struct(vec![
            StackItem::from_int(BigInt::from(100_000_000)),
            StackItem::from_int(BigInt::from(0)),
            StackItem::null(),
            StackItem::from_int(BigInt::from(0)),
        ]);
        let expected_account_bytes =
            BinarySerializer::serialize(&expected_account, &ExecutionEngineLimits::default())
                .unwrap();
        assert_eq!(get(&snapshot, neo_id(), account_key), Some(expected_account_bytes));
        assert_eq!(
            get(&snapshot, neo_id(), vec![NEP17_PREFIX_TOTAL_SUPPLY]),
            Some(BigInt::from(100_000_000).to_signed_bytes_le())
        );
        // The mint's Transfer(null, bft, 100M) notification was emitted by NEO.
        let transfer = outcome
            .on_persist_notifications
            .iter()
            .find(|n| n.event_name == "Transfer")
            .expect("genesis NEO Transfer notification");
        assert_eq!(
            transfer.script_hash,
            neo_native_contracts::NeoToken::script_hash(),
            "the genesis mint Transfer is emitted by the NEO contract"
        );
        assert!(matches!(transfer.state[0], StackItem::Null), "from = null (mint)");
        assert_eq!(
            transfer.state[1].as_bytes().expect("to address bytes"),
            bft.to_bytes(),
            "to = the standby-validator BFT address"
        );
        assert_eq!(
            transfer.state[2].as_int().expect("amount"),
            BigInt::from(100_000_000),
            "amount = the full NEO TotalAmount"
        );
        // No CommitteeChanged at genesis: the recomputed committee equals the
        // seeded standby committee.
        assert!(
            !outcome
                .on_persist_notifications
                .iter()
                .any(|n| n.event_name == "CommitteeChanged"),
            "genesis recompute must not change the committee"
        );

        // --- OracleContract.Initialize seeds ---
        let oracle_id = neo_native_contracts::OracleContract::ID;
        assert_eq!(
            get(&snapshot, oracle_id, vec![ORACLE_PREFIX_REQUEST_ID]),
            Some(Vec::new()),
            "RequestId seeds as BigInteger.Zero (empty bytes)"
        );
        assert_eq!(
            get(&snapshot, oracle_id, vec![ORACLE_PREFIX_PRICE]),
            Some(BigInt::from(50_000_000i64).to_signed_bytes_le()),
            "oracle price seeds as 0.5 GAS"
        );

        // --- NeoToken.PostPersist: committee reward minted at genesis ---
        // gasPerBlock(5 GAS) * CommitteeRewardRatio(10) / 100 = 0.5 GAS to the
        // signature address of committee[0 % m] = standby_committee[0].
        let member = &settings.standby_committee[0];
        let script = neo_redeem_script::signature_redeem_script(&member.to_bytes());
        let reward_account = UInt160::from_script(&script);
        let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
        gas_key.extend_from_slice(&reward_account.to_bytes());
        let gas_account = get(&snapshot, neo_native_contracts::GasToken::ID, gas_key)
            .expect("committee reward GAS account");
        let decoded = BinarySerializer::deserialize(
            &gas_account,
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account is not a struct");
        };
        assert_eq!(
            fields.items().first().unwrap().as_int().unwrap(),
            BigInt::from(50_000_000i64),
            "committee member 0 earns 0.5 GAS at genesis"
        );
        let gas_transfer_minted = outcome
            .post_persist_notifications
            .iter()
            .any(|n| n.event_name == "Transfer");
        assert!(gas_transfer_minted, "PostPersist GAS mint emits Transfer");
    }

    #[test]
    fn non_refresh_block_mints_to_rotating_member_without_recompute() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        // Persist genesis first so the committee cache + gas records exist.
        let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
        persist_block_natives(Arc::clone(&snapshot), genesis, &settings).expect("genesis persist");

        // Block 1: not a refresh block for the 21-member committee.
        let mut header = Header::new();
        header.set_index(1);
        let block = Arc::new(Block::from_parts(header, Vec::new()));
        let outcome = persist_block_natives(Arc::clone(&snapshot), block, &settings)
            .expect("block 1 persist");
        assert!(outcome.initialized.is_empty(), "no native initializes after genesis");

        // committee[1 % 21] = standby_committee[1] earns 0.5 GAS.
        let member = &settings.standby_committee[1];
        let script = neo_redeem_script::signature_redeem_script(&member.to_bytes());
        let reward_account = UInt160::from_script(&script);
        let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
        gas_key.extend_from_slice(&reward_account.to_bytes());
        let gas_account = get(&snapshot, neo_native_contracts::GasToken::ID, gas_key)
            .expect("committee reward GAS account for member 1");
        let decoded = BinarySerializer::deserialize(
            &gas_account,
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account is not a struct");
        };
        assert_eq!(fields.items().first().unwrap().as_int().unwrap(), BigInt::from(50_000_000i64));
    }

    #[test]
    fn probe_constants_pin_the_real_native_ids() {
        // The probe hardcodes protocol constants because the blockchain
        // crate only reaches natives through the type-erased provider;
        // pin them against the canonical definitions.
        assert_eq!(LEDGER_CONTRACT_ID, neo_native_contracts::LedgerContract::ID);
        assert_eq!(NEO_TOKEN_ID, neo_native_contracts::NeoToken::ID);
        assert_eq!(NEO_PREFIX_COMMITTEE_KEY, NEO_PREFIX_COMMITTEE);
    }

    #[test]
    fn chain_state_initialized_flips_after_genesis_persist() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        assert!(!chain_state_initialized(&snapshot), "fresh store is uninitialized");

        let block = Arc::new(genesis_block(&settings).expect("genesis block"));
        persist_block_natives(Arc::clone(&snapshot), block, &settings).expect("genesis persist");
        assert!(chain_state_initialized(&snapshot), "genesis persist initializes the chain");

        // The C#-faithful leg of the probe: a LedgerContract Prefix_Block
        // record alone also reports initialized.
        let ledger_only = DataCache::new(false);
        let mut key = vec![LEDGER_PREFIX_BLOCK];
        key.extend_from_slice(&[0u8; 32]);
        ledger_only.add(
            StorageKey::new(LEDGER_CONTRACT_ID, key),
            neo_data_cache::StorageItem::from_bytes(vec![1]),
        );
        assert!(chain_state_initialized(&ledger_only));
    }

    /// Mainnet genesis-hash pin. Oracle:
    /// `neo_csharp/tests/Neo.UnitTests/SmartContract/UT_InteropService.cs:872`
    /// (`TestGetBlockHash`) asserts block 0's hash under
    /// `TestProtocolSettings.Default`, whose `StandbyCommittee` /
    /// `ValidatorsCount` are byte-identical to
    /// `neo_csharp/src/Neo.CLI/config.mainnet.json` (verified 2026-06-10).
    /// The header hash covers only the serialized unsigned header
    /// (`Neo.Network.P2P.Helper.CalculateHash` — single SHA-256, no
    /// network magic), so the test-chain genesis hash IS the mainnet
    /// genesis hash. This transitively pins `NextConsensus`, the
    /// standby-validator multisig redeem script, and hash160.
    #[test]
    fn mainnet_genesis_hash_matches_csharp() {
        let settings = ProtocolSettings::default();
        let block = genesis_block(&settings).expect("genesis block");
        let hash = block.header.try_hash().expect("genesis hash");
        assert_eq!(
            hash.to_string(),
            "0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15",
            "mainnet genesis hash must match the C# oracle \
             (UT_InteropService.TestGetBlockHash)"
        );
    }

    /// The transaction stage of `Blockchain.Persist`: a HALTing and a
    /// FAULTing transaction in one block both execute and get ledger
    /// records carrying their final VM state, and the
    /// `ApplicationExecuted` list has the C# shape (OnPersist, one per
    /// tx, PostPersist).
    #[test]
    fn persist_executes_transactions_and_records_vm_states() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
        persist_block_natives(Arc::clone(&snapshot), genesis, &settings).expect("genesis persist");

        // tx1 faults (ABORT), tx2 halts (PUSH1).
        let signer = neo_payloads::Signer::new(
            neo_primitives::UInt160::from_bytes(&[0x33; 20]).unwrap(),
            neo_primitives::WitnessScope::NONE,
        );
        let mut tx1 = neo_payloads::Transaction::new();
        tx1.set_nonce(1);
        tx1.set_script(vec![neo_vm_rs::OpCode::ABORT.byte()]);
        tx1.set_system_fee(1_0000_0000);
        tx1.set_signers(vec![signer.clone()]);
        tx1.set_witnesses(vec![neo_payloads::Witness::empty()]);
        let mut tx2 = neo_payloads::Transaction::new();
        tx2.set_nonce(2);
        tx2.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
        tx2.set_system_fee(1_0000_0000);
        tx2.set_signers(vec![signer]);
        tx2.set_witnesses(vec![neo_payloads::Witness::empty()]);
        let tx1_hash = tx1.try_hash().unwrap();
        let tx2_hash = tx2.try_hash().unwrap();

        let mut header = Header::new();
        header.set_index(1);
        let block = Arc::new(Block::from_parts(header, vec![tx1, tx2]));
        let block_hash = block.header.try_hash().unwrap();
        let outcome = persist_block_natives(Arc::clone(&snapshot), block, &settings)
            .expect("block 1 persist");

        // C# allApplicationExecuted: OnPersist, tx1, tx2, PostPersist.
        assert_eq!(outcome.application_executed.len(), 4);
        let tx1_exec = &outcome.application_executed[1];
        assert_eq!(tx1_exec.trigger, neo_primitives::TriggerType::Application);
        assert_eq!(tx1_exec.vm_state, neo_vm_rs::VmState::FAULT);
        assert!(tx1_exec.transaction.is_some());
        let tx2_exec = &outcome.application_executed[2];
        assert_eq!(tx2_exec.vm_state, neo_vm_rs::VmState::HALT);
        // PUSH1 leaves the integer 1 on the result stack.
        assert_eq!(tx2_exec.stack.len(), 1);
        assert_eq!(
            tx2_exec.stack[0].as_int().expect("stack int"),
            BigInt::from(1)
        );

        // Ledger records carry the final VM states (C# mutates the
        // TransactionState stored by Ledger.OnPersist) and the block
        // records exist.
        let ledger = neo_native_contracts::LedgerContract::new();
        let s1 = ledger
            .get_transaction_state(&snapshot, &tx1_hash)
            .unwrap()
            .expect("tx1 record");
        assert_eq!(s1.state, neo_vm_rs::VmState::FAULT);
        assert_eq!(s1.block_index, 1);
        let s2 = ledger
            .get_transaction_state(&snapshot, &tx2_hash)
            .unwrap()
            .expect("tx2 record");
        assert_eq!(s2.state, neo_vm_rs::VmState::HALT);
        assert_eq!(ledger.get_block_hash(&snapshot, 1).unwrap(), Some(block_hash));
        let trimmed = ledger
            .get_trimmed_block(&snapshot, &block_hash)
            .unwrap()
            .expect("trimmed block");
        assert_eq!(trimmed.hashes, vec![tx1_hash, tx2_hash]);
        // PostPersist current-block pointer.
        assert_eq!(ledger.current_index(&snapshot).unwrap(), 1);
        assert_eq!(ledger.current_hash(&snapshot).unwrap(), block_hash);
    }

    /// Genesis persist now writes the C#-faithful Ledger records: the
    /// `Prefix_Block` probe of [`chain_state_initialized`] (the literal
    /// C# `Ledger.Initialized` check) and the current-block pointer.
    #[test]
    fn genesis_persist_writes_ledger_records() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
        let genesis_hash = genesis.header.try_hash().unwrap();
        persist_block_natives(Arc::clone(&snapshot), genesis, &settings).expect("genesis persist");

        let ledger = neo_native_contracts::LedgerContract::new();
        assert_eq!(ledger.get_block_hash(&snapshot, 0).unwrap(), Some(genesis_hash));
        assert_eq!(ledger.current_index(&snapshot).unwrap(), 0);
        assert_eq!(ledger.current_hash(&snapshot).unwrap(), genesis_hash);
        let block_prefix = StorageKey::new(LEDGER_CONTRACT_ID, vec![LEDGER_PREFIX_BLOCK]);
        assert!(
            snapshot
                .find(Some(&block_prefix), neo_storage::persistence::SeekDirection::Forward)
                .next()
                .is_some(),
            "the C# Ledger.Initialized probe (any Prefix_Block record) must hit"
        );
    }

    /// The staging contract the per-block atomicity rests on: writes
    /// into a `clone_cache()` child are invisible to the parent until
    /// `commit()`, and dropping the child discards them. The persist
    /// pipeline stages every block write in such a child, so a
    /// mid-sequence error can never leave partial block state in the
    /// caller's snapshot.
    #[test]
    fn block_staging_cache_isolates_until_commit() {
        let parent = DataCache::new(false);
        let key = StorageKey::new(-4, vec![5, 0xAA]);

        // Discard leg: child writes never reach the parent.
        {
            let child = parent.clone_cache();
            child.add(key.clone(), neo_data_cache::StorageItem::from_bytes(vec![1]));
            assert!(child.get(&key).is_some());
            assert!(parent.get(&key).is_none(), "uncommitted child write leaked");
        }
        assert!(parent.get(&key).is_none(), "dropped child write leaked");

        // Commit leg: the child write lands atomically on commit.
        let child = parent.clone_cache();
        child.add(key.clone(), neo_data_cache::StorageItem::from_bytes(vec![2]));
        assert!(parent.get(&key).is_none());
        child.commit();
        assert_eq!(
            parent.get(&key).map(|i| i.value_bytes().into_owned()),
            Some(vec![2])
        );
    }
}
