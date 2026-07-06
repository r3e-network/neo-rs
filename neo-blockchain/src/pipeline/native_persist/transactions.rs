use std::collections::HashMap;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContractsCache};
use neo_manifest::CallFlags;
use neo_payloads::Block;
use neo_primitives::{TriggerType, Verifiable};
use neo_storage::DataCache;
use neo_vm_rs::VmState as VMState;
use parking_lot::Mutex;

use super::artifacts::application_executed;
use super::metrics::record_tx_stage;
use super::trace::{TraceTxFilter, trace_tx_frames, trace_tx_notifications};
use super::{NativePersistOptions, NativePersistOutcome, NativePersistResources};

/// Runs the C# `Blockchain.Persist` transaction stage.
///
/// Each transaction executes in its own Application-trigger engine with gas
/// limit equal to `tx.SystemFee` over a child cache of the block cache. HALT
/// commits the child cache into the block cache, FAULT discards it, and the
/// Ledger transaction record is rewritten with the final VM state either way.
pub(super) fn run_transaction_stage(
    block_cache: &Arc<DataCache>,
    block: &Arc<Block>,
    settings: &ProtocolSettings,
    options: NativePersistOptions,
    resources: &NativePersistResources,
    native_contract_cache: Arc<Mutex<NativeContractsCache>>,
    outcome: &mut NativePersistOutcome,
) -> CoreResult<u64> {
    let block_index = block.index();
    let tx_start = std::time::Instant::now();
    let trace_tx_filter = TraceTxFilter::from_env();
    let native_contract_provider = resources.provider();

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
                Some(Arc::clone(block)),
                settings.clone(),
                tx.system_fee(),
                HashMap::new(),
                Arc::clone(&native_contract_cache),
                None,
                Some(Arc::clone(&native_contract_provider)),
            )?;
        record_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::EngineCreate,
            stage_start,
        );

        // C# loads the script unchecked and lets execution FAULT on a bad
        // instruction; a Rust load error therefore faults the transaction,
        // never the block.
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
