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

use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_manifest::CallFlags;
use neo_payloads::ApplicationExecuted;
use neo_payloads::{Block, Header, Witness};
use neo_primitives::{TriggerType, UInt160, UInt256, Verifiable};
use neo_storage::DataCache;
use neo_storage::StorageKey;
use neo_storage::persistence::SeekDirection;
use neo_vm_rs::VmState as VMState;

mod artifacts;
mod metrics;
mod trace;

pub use artifacts::NativePersistNotification;
use artifacts::{application_executed, collect_notifications};
use metrics::record_tx_stage;
use trace::{TraceTxFilter, trace_tx_frames, trace_tx_notifications};

#[cfg(test)]
use neo_vm::StackItem;
#[cfg(test)]
use neo_vm_rs::StackValue;

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

/// Outcome of [`persist_block_natives_with_resources`] for one block.
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

/// Controls which non-consensus replay artifacts native persistence materializes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePersistOptions {
    /// Capture `ApplicationExecuted` records and cloned native notifications for
    /// plugin/indexer replay. Disable only for explicit bulk-sync imports whose
    /// consumers intentionally skip those replay hooks.
    pub capture_replay_artifacts: bool,
}

impl Default for NativePersistOptions {
    fn default() -> Self {
        Self {
            capture_replay_artifacts: true,
        }
    }
}

/// Reusable native-persistence resources for a sequence of blocks that share
/// the same explicit native-contract provider and protocol settings.
#[derive(Clone)]
pub struct NativePersistResources {
    provider: Arc<dyn NativeContractProvider>,
    contracts: Arc<[Arc<dyn neo_execution::NativeContract>]>,
}

impl NativePersistResources {
    /// Captures the canonical native-contract list once from an explicit
    /// provider. The list order is the C# native registration order used by
    /// both OnPersist and PostPersist hooks.
    pub fn from_provider(provider: Arc<dyn NativeContractProvider>) -> Self {
        let contracts = provider.all_native_contracts().into();
        Self {
            provider,
            contracts,
        }
    }

    /// Returns the canonical native contracts captured for this persistence
    /// batch, in C# registration order.
    pub fn contracts(&self) -> &[Arc<dyn neo_execution::NativeContract>] {
        self.contracts.as_ref()
    }

    /// Returns the native-contract provider captured for this persistence batch.
    pub fn provider(&self) -> Arc<dyn NativeContractProvider> {
        Arc::clone(&self.provider)
    }
}

/// A block persistence result whose storage writes are still staged in a child
/// cache. The caller must run committing hooks against [`Self::snapshot`] and
/// call [`Self::commit`] only after every pre-commit gate succeeds.
pub struct StagedNativePersist {
    /// The staged block writes, isolated from the canonical snapshot until
    /// [`Self::commit`] is called.
    snapshot: Arc<DataCache>,
    /// Native persistence metadata and ApplicationExecuted records.
    pub outcome: NativePersistOutcome,
    block_index: u32,
    n_tx: usize,
    onpersist_us: u64,
    tx_us: u64,
    postpersist_us: u64,
    total_start: std::time::Instant,
}

impl StagedNativePersist {
    /// Returns the staged snapshot that committing hooks should inspect.
    pub fn snapshot(&self) -> &DataCache {
        self.snapshot.as_ref()
    }

    /// Publishes the staged writes into the canonical parent snapshot.
    pub fn commit(&self) {
        let cache_commit_start = std::time::Instant::now();
        self.snapshot.commit();
        let cache_commit_us = neo_runtime::time::elapsed_us(cache_commit_start.elapsed());
        let total_us = neo_runtime::time::elapsed_us(self.total_start.elapsed());
        neo_runtime::sync_metrics::record_native_persist(
            self.block_index as u64,
            self.n_tx as u64,
            self.onpersist_us,
            self.tx_us,
            self.postpersist_us,
            cache_commit_us,
            total_us,
        );
        debug!(
            target: "neo::sync",
            index = self.block_index,
            txs = self.n_tx,
            onpersist_us = self.onpersist_us,
            tx_stage_us = self.tx_us,
            postpersist_us = self.postpersist_us,
            cache_commit_us,
            total_us,
            "persist_block_natives timing"
        );
    }
}

/// C# `NativeContract.Ledger.Initialized(snapshot)` (LedgerContract.cs:91):
/// whether the chain state has been bootstrapped, i.e. the genesis block
/// has been persisted. The first probe is the literal C# check (any
/// `LedgerContract` `Prefix_Block` record, written by the persist
/// pipeline via `crate::ledger_records`); the second probes the
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
    neo_vm::script_builder::RedeemScript::bft_address(pubkeys)
        .ok_or_else(|| CoreError::invalid_operation("BFT address requires at least one validator"))
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
    let metric_hook = match trigger {
        TriggerType::OnPersist => neo_runtime::sync_metrics::NativePersistHook::OnPersist,
        TriggerType::PostPersist => neo_runtime::sync_metrics::NativePersistHook::PostPersist,
        other => {
            return Err(CoreError::invalid_operation(format!(
                "native persist hooks require an OnPersist/PostPersist engine, got {other:?}"
            )));
        }
    };
    for contract in contracts {
        if !contract.is_active(settings, block_index) {
            continue;
        }
        let hook_start = std::time::Instant::now();
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
        let result = match metric_hook {
            neo_runtime::sync_metrics::NativePersistHook::OnPersist => contract.on_persist(engine),
            neo_runtime::sync_metrics::NativePersistHook::PostPersist => {
                contract.post_persist(engine)
            }
        };
        neo_runtime::sync_metrics::record_native_contract_hook(
            metric_hook,
            contract.id(),
            neo_runtime::time::elapsed_us(hook_start.elapsed()),
        );
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
/// Runs the C# `Blockchain.Persist` sequence with caller-provided reusable
/// native resources and commits the staged writes on success.
pub fn persist_block_natives_with_resources(
    snapshot: Arc<DataCache>,
    block: Arc<Block>,
    settings: &ProtocolSettings,
    options: NativePersistOptions,
    resources: &NativePersistResources,
) -> CoreResult<NativePersistOutcome> {
    let staged = stage_block_natives_with_resources(snapshot, block, settings, options, resources)?;
    let outcome = staged.outcome.clone();
    staged.commit();
    Ok(outcome)
}

/// Runs native block persistence with caller-provided reusable resources.
pub fn stage_block_natives_with_resources(
    snapshot: Arc<DataCache>,
    block: Arc<Block>,
    settings: &ProtocolSettings,
    options: NativePersistOptions,
    resources: &NativePersistResources,
) -> CoreResult<StagedNativePersist> {
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
        settings.clone(),
        0,
        None,
        Some(Arc::clone(&resources.provider)),
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
        settings,
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
    let tx_start = std::time::Instant::now();
    let trace_tx_filter = TraceTxFilter::from_env();
    for tx in &block.transactions {
        let stage_start = std::time::Instant::now();
        let tx_hash = tx
            .try_hash()
            .map_err(|e| CoreError::invalid_operation(format!("persist: tx hash: {e}")))?;
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::Hash,
            stage_start,
        );

        let stage_start = std::time::Instant::now();
        let tx_cache = Arc::new(block_cache.clone_cache());
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::CloneCache,
            stage_start,
        );

        let stage_start = std::time::Instant::now();
        let container: Arc<dyn Verifiable> = Arc::new(tx.clone());
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::ContainerClone,
            stage_start,
        );

        let stage_start = std::time::Instant::now();
        let mut tx_engine =
            ApplicationEngine::new_with_preloaded_native_and_native_contract_provider(
                TriggerType::Application,
                Some(container),
                Arc::clone(&tx_cache),
                Some(Arc::clone(&block)),
                settings.clone(),
                tx.system_fee(),
                HashMap::new(),
                Arc::clone(&native_contract_cache),
                None,
                Some(Arc::clone(&resources.provider)),
            )?;
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::EngineCreate,
            stage_start,
        );

        // C# loads the script unchecked and lets execution FAULT on a
        // bad instruction; a Rust load error therefore faults the
        // transaction, never the block.
        let load_execute_start = std::time::Instant::now();
        let stage_start = std::time::Instant::now();
        let load_result = tx_engine.load_script(tx.script().to_vec(), CallFlags::ALL, None);
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::LoadScript,
            stage_start,
        );
        let (vm_state, load_error) = match load_result {
            Ok(()) => {
                let stage_start = std::time::Instant::now();
                let vm_state = tx_engine.execute_allow_fault();
                record_tx_stage(
                    neo_runtime::sync_metrics::NativePersistTxStage::Execute,
                    stage_start,
                );
                (vm_state, None)
            }
            Err(error) => (VMState::FAULT, Some(error.to_string())),
        };
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::LoadExecute,
            load_execute_start,
        );

        let should_trace_tx = trace_tx_filter.matches(&tx_hash);
        let stage_start = std::time::Instant::now();
        let mut executed = if options.capture_replay_artifacts || should_trace_tx {
            let mut executed = application_executed(&tx_engine, Some(tx.clone()), vm_state);
            if executed.exception.is_none() {
                executed.exception = load_error.clone();
            }
            Some(executed)
        } else {
            None
        };
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::ApplicationExecuted,
            stage_start,
        );

        if should_trace_tx {
            let exception = executed
                .as_ref()
                .and_then(|executed| executed.exception.as_deref())
                .unwrap_or("");
            eprintln!(
                "trace tx block={} hash={} vm_state={:?} fee_consumed={} fee_consumed_pico={} fee_amount_pico={} current={} calling={} entry={} depth={} frames={} exception={} notifications={} notification_details={}",
                block_index,
                tx_hash,
                vm_state,
                tx_engine.fee_consumed(),
                tx_engine.fee_consumed_pico(),
                tx_engine.fee_amount_pico(),
                tx_engine
                    .current_script_hash()
                    .map(|hash| hash.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                tx_engine
                    .get_calling_script_hash()
                    .map(|hash| hash.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                tx_engine
                    .entry_script_hash()
                    .map(|hash| hash.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                tx_engine.invocation_stack().len(),
                trace_tx_frames(&tx_engine),
                exception,
                tx_engine.notifications().len(),
                trace_tx_notifications(&tx_engine)
            );
        }
        if options.capture_replay_artifacts {
            if let Some(executed) = executed.take() {
                outcome.application_executed.push(executed);
            }
        }
        drop(tx_engine);

        if vm_state == VMState::HALT {
            let stage_start = std::time::Instant::now();
            tx_cache.commit();
            record_tx_stage(
                neo_runtime::sync_metrics::NativePersistTxStage::TxCacheCommit,
                stage_start,
            );
        }
        let stage_start = std::time::Instant::now();
        crate::ledger_records::LedgerRecords::update_transaction_vm_state(
            &block_cache,
            block_index,
            tx,
            &tx_hash,
            vm_state,
        )?;
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::LedgerVmState,
            stage_start,
        );
    }
    let tx_us = neo_runtime::time::elapsed_us(tx_start.elapsed());

    // --- PostPersist stage (C# TriggerType.PostPersist engine, gas 0) ---
    let postpersist_start = std::time::Instant::now();
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::PostPersist,
        None,
        Arc::clone(&block_cache),
        Some(Arc::clone(&block)),
        settings.clone(),
        0,
        None,
        Some(Arc::clone(&resources.provider)),
    )?;
    run_native_persist_hooks(
        contracts,
        &mut engine,
        settings,
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
