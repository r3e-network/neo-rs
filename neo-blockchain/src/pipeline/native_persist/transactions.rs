use std::collections::HashMap;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_execution::{ApplicationEngine, NativeContractsCache, NoDiagnostic};
use neo_manifest::CallFlags;
use neo_payloads::{Block, VerifiableContainer};
use neo_primitives::TriggerType;
use neo_storage::{CacheRead, DataCache};
use neo_vm::VmState as VMState;
use parking_lot::Mutex;

use super::artifacts::application_executed;
use super::metrics::{record_tx_stage, record_tx_stage_elapsed};
use super::trace::{
    SlowTxFilter, TraceTxFilter, VmProfileFilter, format_vm_hottest_opcodes,
    format_vm_opcode_classes, trace_tx_artifact, trace_tx_frames, trace_tx_notifications,
};
use super::{NativePersistOptions, NativePersistOutcome, NativePersistResources};

/// Runs the C# `Blockchain.Persist` transaction stage.
///
/// Each transaction executes in its own Application-trigger engine with gas
/// limit equal to `tx.SystemFee` over a child cache of the block cache. HALT
/// commits the child cache into the block cache, FAULT discards it, and the
/// Ledger transaction record is rewritten with the final VM state either way.
pub(super) fn run_transaction_stage<P, B>(
    block_cache: &Arc<DataCache<B>>,
    block: &Arc<Block>,
    settings: &Arc<ProtocolSettings>,
    options: NativePersistOptions,
    resources: &NativePersistResources<P>,
    native_contract_cache: Arc<Mutex<NativeContractsCache>>,
    outcome: &mut NativePersistOutcome,
) -> CoreResult<u64>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
    let block_index = block.index();
    let tx_start = std::time::Instant::now();
    let trace_tx_filter = TraceTxFilter::from_env();
    let slow_tx_filter = SlowTxFilter::from_env();
    let vm_profile_filter = VmProfileFilter::from_env();
    let native_contract_provider = resources.provider();
    let mut reusable_tx_cache: Option<Arc<DataCache<B>>> = None;

    for (transaction_index, tx) in block.transactions.iter().enumerate() {
        let stage_start = std::time::Instant::now();
        let tx_hash = tx
            .try_hash()
            .map_err(|e| CoreError::invalid_operation(format!("persist: tx hash: {e}")))?;
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::Hash,
            stage_start,
        );

        let stage_start = std::time::Instant::now();
        if let Some(tx_cache) = reusable_tx_cache.as_ref() {
            tx_cache.reset();
        } else {
            reusable_tx_cache = Some(Arc::new(block_cache.clone_cache()));
        }
        let Some(tx_cache) = reusable_tx_cache.as_ref() else {
            return Err(CoreError::invalid_operation(
                "persist: failed to initialize transaction cache",
            ));
        };
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::CachePrepare,
            stage_start,
        );

        let stage_start = std::time::Instant::now();
        let container = Arc::new(
            VerifiableContainer::transaction_in_block(Arc::clone(block), transaction_index)
                .ok_or_else(|| {
                    CoreError::invalid_operation(
                        "persist: transaction position disappeared from immutable block",
                    )
                })?,
        );
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::ContainerPrepare,
            stage_start,
        );

        let stage_start = std::time::Instant::now();
        let mut tx_engine =
            ApplicationEngine::new_with_preloaded_native_and_native_contract_provider(
                TriggerType::Application,
                Some(container),
                Arc::clone(tx_cache),
                Some(Arc::clone(block)),
                Arc::clone(settings),
                tx.system_fee(),
                HashMap::new(),
                Arc::clone(&native_contract_cache),
                NoDiagnostic,
                Arc::clone(&native_contract_provider),
            )?;
        let should_profile_vm = vm_profile_filter.matches(block_index, &tx_hash);
        if should_profile_vm {
            tx_engine.enable_vm_execution_profiling();
        }
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::EngineCreate,
            stage_start,
        );

        // C# loads the script unchecked and lets execution FAULT on a bad
        // instruction; a Rust load error therefore faults the transaction,
        // never the block.
        let load_execute_start = std::time::Instant::now();
        let stage_start = std::time::Instant::now();
        let load_result = tx_engine.load_script_bytes(tx.script(), CallFlags::ALL, None);
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::LoadScript,
            stage_start,
        );
        let (vm_state, load_error, execute_us) = match load_result {
            Ok(()) => {
                let stage_start = std::time::Instant::now();
                let vm_state = tx_engine.execute_allow_fault();
                let execute_us = record_tx_stage_elapsed(
                    neo_runtime::sync_metrics::NativePersistTxStage::Execute,
                    stage_start.elapsed(),
                );
                (vm_state, None, execute_us)
            }
            Err(error) => (VMState::FAULT, Some(error.to_string()), 0),
        };
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::LoadExecute,
            load_execute_start,
        );

        let should_trace_tx = trace_tx_filter.matches(block_index, &tx_hash);
        if slow_tx_filter.matches(block_index, execute_us) {
            let instructions = tx_engine.instructions_executed();
            tracing::info!(
                target: "neo::profile",
                block_index,
                transaction_index,
                tx_hash = %tx_hash,
                execute_us,
                instructions,
                ns_per_instruction = execute_us.saturating_mul(1_000) / instructions.max(1),
                script_bytes = tx.script().len(),
                system_fee = tx.system_fee(),
                network_fee = tx.network_fee(),
                fee_consumed = tx_engine.fee_consumed(),
                vm_state = ?vm_state,
                invocation_depth = tx_engine.invocation_stack().len(),
                notification_count = tx_engine.notifications().len(),
                "slow transaction execution profile"
            );
        }
        if should_profile_vm && let Some(profile) = tx_engine.vm_execution_profile() {
            let stack = profile.stack_operations();
            tracing::info!(
                target: "neo::profile",
                block_index,
                transaction_index,
                tx_hash = %tx_hash,
                execute_us,
                instructions = tx_engine.instructions_executed(),
                profiled_instructions = profile.total_instructions(),
                opcode_classes = %format_vm_opcode_classes(&profile),
                hottest_opcodes = %format_vm_hottest_opcodes(&profile, 16),
                stack_pushes = stack.pushes,
                stack_pops = stack.pops,
                stack_peeks = stack.peeks,
                stack_mutable_peeks = stack.mutable_peeks,
                stack_inserts = stack.inserts,
                stack_removes = stack.removes,
                stack_swaps = stack.swaps,
                stack_reverses = stack.reverses,
                stack_clears = stack.clears,
                stack_cleared_items = stack.cleared_items,
                stack_copies = stack.copies,
                stack_copied_items = stack.copied_items,
                stack_moves = stack.moves,
                stack_moved_items = stack.moved_items,
                stack_max_depth = stack.max_depth,
                vm_state = ?vm_state,
                "targeted VM execution profile"
            );
        }
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
            if let Some(executed) = executed.as_ref() {
                match trace_tx_artifact(block_index, &tx_hash, executed) {
                    Ok(artifact) => eprintln!("NEO_TX_ARTIFACT {artifact}"),
                    Err(error) => eprintln!(
                        "NEO_TX_ARTIFACT_ERROR block={} hash={} error={}",
                        block_index, tx_hash, error
                    ),
                }
            }
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
        if options.capture_replay_artifacts
            && let Some(executed) = executed.take()
        {
            outcome.application_executed.push(executed);
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
            block_cache,
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

    Ok(neo_runtime::time::elapsed_us(tx_start.elapsed()))
}
