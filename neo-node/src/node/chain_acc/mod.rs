//! # neo-node::node::chain_acc
//!
//! chain.acc import, reporting, and throughput accounting helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `batch`: batch accounting and dispatch helpers.
//! - `format`: chain.acc file format readers and validation helpers.
//! - `metrics`: Metrics collection and progress-reporting helpers.
//! - `range`: Expected-range, resume, and continuity validation helpers.

use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
use neo_blockchain::command::ImportBlocksReply;
use neo_blockchain::handle::BlockchainHandle;
use neo_payloads::block::Block;
use neo_storage::persistence::store::Store;
use tracing::info;

mod batch;
mod format;
mod metrics;
mod range;
use batch::{
    ChainAccImportComposition, PendingChainAccBatch, import_chain_acc_batch, take_import_batch,
};
use format::{read_chain_acc_header, read_next_chain_acc_block, skip_chain_acc_records};
use metrics::{
    ChainAccImportProgress, NativePersistTxStageImportMetrics, RocksDbBatchImportMetrics,
    StateServiceMptImportMetrics, should_log_import_progress,
};
use range::{
    bounded_chain_acc_import_range, chain_acc_import_record_count, chain_acc_records_to_skip,
    count_only_stop_height_exceeded, count_only_stop_height_reached, expected_chain_acc_count,
    expected_chain_acc_first_prev_hash, resume_chain_acc_import_range,
    validate_chain_acc_block_height, validate_chain_acc_count, validate_chain_acc_first_prev_hash,
    validate_chain_acc_internal_prev_hash,
};

/// The mixed-block batch size for trusted `chain.acc` Import commands.
///
/// C# Neo uses 10 because it prioritizes simple live-import parity. This path is
/// a trusted local fast-sync import: larger mixed batches reduce expensive
/// StateService/durable-store finalization fences while preserving per-block
/// native/state transitions. Empty-only runs use the same outer command
/// boundary: the blockchain service owns the smaller internal empty
/// fast-forward chunks while keeping one outer batch snapshot/finalization.
const IMPORT_BATCH_SIZE: usize = 10_000;

/// Import blocks from a `chain.acc` file and stop once `stop_at_height` is imported.
pub async fn import_chain_acc_until_height(
    handle: &BlockchainHandle,
    path: &Path,
    verify: bool,
    stop_at_height: Option<u32>,
    storage: Option<Arc<dyn Store>>,
) -> anyhow::Result<u64> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("opening chain.acc {}: {e}", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    import_chain_acc_from_reader_until_height(
        handle,
        &mut reader,
        Some(path),
        verify,
        None,
        stop_at_height,
        storage,
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChainAccExpectedRange {
    pub(super) start_height: u32,
    pub(super) end_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LocalLedgerTip {
    pub(super) height: u32,
    pub(super) hash: neo_primitives::UInt256,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ChainAccImportReport {
    pub(super) imported: u64,
    pub(super) last_imported_tip: Option<LocalLedgerTip>,
    pub(super) elapsed_seconds: f64,
    pub(super) driver_elapsed_seconds: f64,
    pub(super) chain_acc_read_seconds: f64,
    pub(super) chain_acc_validate_seconds: f64,
    pub(super) average_blocks_per_second: f64,
    pub(super) empty_blocks: u64,
    pub(super) empty_only_blocks: u64,
    pub(super) empty_block_import_seconds: f64,
    pub(super) empty_blocks_per_second: f64,
    pub(super) transaction_blocks: u64,
    pub(super) transactions: u64,
    pub(super) transaction_block_import_seconds: f64,
    pub(super) transaction_block_clone_seconds: f64,
    pub(super) transaction_ledger_insert_seconds: f64,
    pub(super) transaction_committed_hook_seconds: f64,
    pub(super) transaction_blocks_per_second: f64,
    pub(super) finalization_seconds: f64,
    pub(super) finalization_commit_handlers_seconds: f64,
    pub(super) finalization_store_commit_seconds: f64,
    pub(super) unclassified_import_seconds: f64,
    pub(super) hot_metrics: ImportHotMetrics,
}

impl ChainAccImportReport {
    #[cfg(test)]
    pub(super) fn with_hot_metrics(mut self, hot_metrics: ImportHotMetrics) -> Self {
        self.hot_metrics = hot_metrics;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ImportHotMetrics {
    pub(super) state_service_mpt_apply_attempts: u64,
    pub(super) state_service_mpt_apply_failures: u64,
    pub(super) state_service_mpt_apply_height: u64,
    pub(super) state_service_mpt_avg_total_us: u64,
    pub(super) state_service_mpt_avg_project_us: u64,
    pub(super) state_service_mpt_avg_trie_us: u64,
    pub(super) state_service_mpt_avg_changes: u64,
    pub(super) state_service_mpt_enqueue_blocking_avg_us: u64,
    pub(super) state_service_mpt_queue_wait_avg_us: u64,
    pub(super) state_service_mpt_mutate_changes_avg_us: u64,
    pub(super) state_service_mpt_root_hash_avg_us: u64,
    pub(super) state_service_mpt_trie_commit_avg_us: u64,
    pub(super) state_service_mpt_backing_commit_avg_us: u64,
    pub(super) state_service_mpt_publish_generation_avg_us: u64,
    pub(super) state_service_mpt_overlay_entries_avg: u64,
    pub(super) state_service_mpt_batch_blocks_avg: u64,
    pub(super) native_persist_avg_total_us: u64,
    pub(super) native_persist_tx_hot_stage: &'static str,
    pub(super) native_persist_tx_hot_stage_avg_us: u64,
    pub(super) rocksdb_batch_avg_flush_duration_ms: u64,
    pub(super) rocksdb_batch_pending_operations: u64,
}

pub(super) fn local_ledger_tip(
    store: Option<&Arc<dyn Store>>,
) -> anyhow::Result<Option<LocalLedgerTip>> {
    let Some(store) = store else {
        return Ok(None);
    };
    let cache = neo_storage::persistence::StoreCache::new_from_store(Arc::clone(store), true);
    let ledger = neo_native_contracts::LedgerContract::new();
    let Ok(height) = ledger.current_index(cache.data_cache()) else {
        return Ok(None);
    };
    let hash = ledger.current_hash(cache.data_cache()).map_err(|err| {
        anyhow::anyhow!("reading local ledger tip hash before chain.acc import: {err}")
    })?;
    Ok(Some(LocalLedgerTip { height, hash }))
}

pub(super) async fn import_chain_acc_report_with_expected_range(
    handle: &BlockchainHandle,
    path: &Path,
    verify: bool,
    expected_range: ChainAccExpectedRange,
    stop_at_height: Option<u32>,
    storage: Option<Arc<dyn Store>>,
) -> anyhow::Result<ChainAccImportReport> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("opening chain.acc {}: {e}", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    import_chain_acc_report_from_reader_until_height(
        handle,
        &mut reader,
        Some(path),
        verify,
        Some(expected_range),
        stop_at_height,
        storage,
    )
    .await
}

#[cfg(test)]
async fn import_chain_acc_from_reader<R>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    storage: Option<Arc<dyn Store>>,
) -> anyhow::Result<u64>
where
    R: Read + Seek,
{
    Ok(
        import_chain_acc_from_reader_report(handle, reader, path, verify, expected_range, storage)
            .await?
            .imported,
    )
}

#[cfg(test)]
async fn import_chain_acc_from_reader_report<R>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    storage: Option<Arc<dyn Store>>,
) -> anyhow::Result<ChainAccImportReport>
where
    R: Read + Seek,
{
    import_chain_acc_report_from_reader_until_height(
        handle,
        reader,
        path,
        verify,
        expected_range,
        None,
        storage,
    )
    .await
}

async fn import_chain_acc_from_reader_until_height<R>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    storage: Option<Arc<dyn Store>>,
) -> anyhow::Result<u64>
where
    R: Read + Seek,
{
    Ok(import_chain_acc_report_from_reader_until_height(
        handle,
        reader,
        path,
        verify,
        expected_range,
        stop_at_height,
        storage,
    )
    .await?
    .imported)
}

async fn import_chain_acc_report_from_reader_until_height<R>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    storage: Option<Arc<dyn Store>>,
) -> anyhow::Result<ChainAccImportReport>
where
    R: Read + Seek,
{
    let driver_start = Instant::now();
    let mut chain_acc_read_elapsed = std::time::Duration::ZERO;
    let mut chain_acc_validate_elapsed = std::time::Duration::ZERO;

    let read_start = Instant::now();
    let header = read_chain_acc_header(reader)?;
    chain_acc_read_elapsed += read_start.elapsed();
    let count = header.count;
    let validate_start = Instant::now();
    if let Some(range) = expected_range {
        validate_chain_acc_count(count, range)?;
    }
    let bounded_import_range =
        bounded_chain_acc_import_range(expected_range, header.start_height, stop_at_height);
    let local_tip = local_ledger_tip(storage.as_ref())?;
    let import_range = resume_chain_acc_import_range(bounded_import_range, local_tip.as_ref())?;
    let import_count =
        chain_acc_import_record_count(count, expected_range, import_range, stop_at_height)?;
    let import_expected_count = expected_range
        .and(import_range)
        .map(expected_chain_acc_count)
        .transpose()?;
    let records_to_skip =
        chain_acc_records_to_skip(count, expected_range, header.start_height, import_range)?;

    if let Some(path) = path {
        info!(target: "neo::import", file = %path.display(), count, import_count, verify, "importing blocks from chain.acc");
    } else {
        info!(target: "neo::import", count, import_count, verify, "importing blocks from chain.acc stream");
    }

    let mut batch: Vec<Block> = Vec::with_capacity(IMPORT_BATCH_SIZE);
    let mut pending_batch = PendingChainAccBatch::default();
    let mut block_bytes = Vec::new();
    let mut imported = 0u64;
    let mut progress = ChainAccImportProgress::new(import_count);
    let mut composition = ChainAccImportComposition::default();
    let mut previous_height = None;
    let mut previous_hash = None;
    let mut last_imported_tip = None;
    let mut hot_metrics = ImportHotMetrics::default();
    let expected_first_prev_hash =
        expected_chain_acc_first_prev_hash(import_range, local_tip.as_ref())?;
    chain_acc_validate_elapsed += validate_start.elapsed();

    let read_start = Instant::now();
    skip_chain_acc_records(reader, records_to_skip)?;
    chain_acc_read_elapsed += read_start.elapsed();

    for i in 0..import_count {
        let record = records_to_skip + i;
        let read_start = Instant::now();
        let block = read_next_chain_acc_block(reader, record, &mut block_bytes)?;
        chain_acc_read_elapsed += read_start.elapsed();
        let validate_start = Instant::now();
        validate_chain_acc_block_height(
            i,
            block.index(),
            header.start_height,
            import_range,
            import_expected_count,
            &mut previous_height,
        )?;
        validate_chain_acc_first_prev_hash(i, &block, expected_first_prev_hash.as_ref())?;
        validate_chain_acc_internal_prev_hash(i, &block, previous_hash.as_ref())?;
        if count_only_stop_height_exceeded(expected_range, stop_at_height, block.index()) {
            chain_acc_validate_elapsed += validate_start.elapsed();
            break;
        }
        let reached_count_only_stop_height =
            count_only_stop_height_reached(expected_range, stop_at_height, block.index());
        chain_acc_validate_elapsed += validate_start.elapsed();
        previous_hash = Some(block.hash());
        pending_batch.record_pushed(&block);
        batch.push(block);

        if pending_batch.should_flush(batch.len())
            || i + 1 == import_count
            || reached_count_only_stop_height
        {
            let batch_blocks = take_import_batch(
                &mut batch,
                i + 1 < import_count && !reached_count_only_stop_height,
            );
            let batch_composition = pending_batch.composition;
            let batch_tip = pending_batch.tip;
            pending_batch.clear();
            let batch_result =
                import_chain_acc_batch(handle, batch_blocks, batch_composition, batch_tip, verify)
                    .await
                    .map_err(|e| anyhow::anyhow!("import command failed: {e}"))?;
            progress.record_batch(batch_result.imported, batch_result.elapsed);
            imported += batch_result.imported as u64;
            composition.record_imported(
                batch_result.composition,
                batch_result.imported,
                batch_result.elapsed,
                batch_result.stats,
            );
            if batch_result.fully_imported() {
                last_imported_tip = batch_result.tip;
            }
            let state_service_metrics = StateServiceMptImportMetrics::current();
            let native_tx_stage_metrics = NativePersistTxStageImportMetrics::current();
            let rocksdb_batch_metrics = storage
                .as_deref()
                .and_then(RocksDbBatchImportMetrics::from_store);
            hot_metrics =
                ImportHotMetrics::from_snapshots(&state_service_metrics, rocksdb_batch_metrics);
            if should_log_import_progress(
                progress.imported(),
                batch_result.imported,
                batch_result.len,
                import_count,
            ) && tracing::enabled!(target: "neo::import", tracing::Level::INFO)
            {
                let progress_sample = progress.sample(batch_result.imported, batch_result.elapsed);
                info!(
                    target: "neo::import",
                    imported = progress_sample.imported,
                    total = progress_sample.total,
                    batch_imported = progress_sample.batch_imported,
                    batch_blocks_per_second = progress_sample.batch_blocks_per_second,
                    average_blocks_per_second = progress_sample.average_blocks_per_second,
                    empty_blocks = composition.empty_blocks,
                    empty_only_blocks = composition.empty_only_blocks,
                    empty_block_import_seconds = composition.empty_block_import_seconds(),
                    empty_blocks_per_second = composition.empty_blocks_per_second(),
                    transaction_blocks = composition.transaction_blocks,
                    transactions = composition.transactions,
                    transaction_block_import_seconds =
                        composition.transaction_block_import_seconds(),
                    transaction_blocks_per_second = composition.transaction_blocks_per_second(),
                    elapsed_seconds = progress_sample.elapsed_seconds,
                    sync_blocks_persisted = state_service_metrics.sync_blocks_persisted,
                    sync_avg_total_us = state_service_metrics.sync_avg_total_us,
                    sync_avg_verify_us = state_service_metrics.sync_avg_verify_us,
                    sync_avg_persist_us = state_service_metrics.sync_avg_persist_us,
                    sync_avg_commit_us = state_service_metrics.sync_avg_commit_us,
                    native_persist_avg_total_us = state_service_metrics.native_persist_avg_total_us,
                    native_persist_avg_onpersist_us = state_service_metrics.native_persist_avg_onpersist_us,
                    native_persist_avg_tx_us = state_service_metrics.native_persist_avg_tx_us,
                    native_persist_avg_postpersist_us = state_service_metrics.native_persist_avg_postpersist_us,
                    native_persist_avg_cache_commit_us = state_service_metrics.native_persist_avg_cache_commit_us,
                    native_persist_avg_tx_count = state_service_metrics.native_persist_avg_tx_count,
                    native_persist_tx_hot_stage = state_service_metrics.native_persist_tx_hot_stage,
                    native_persist_tx_hot_stage_avg_us = state_service_metrics.native_persist_tx_hot_stage_avg_us,
                    native_persist_tx_load_execute_avg_us = native_tx_stage_metrics.load_execute_avg_us,
                    native_persist_tx_load_script_avg_us = native_tx_stage_metrics.load_script_avg_us,
                    native_persist_tx_execute_avg_us = native_tx_stage_metrics.execute_avg_us,
                    native_contract_hook_hot_trigger = state_service_metrics.native_contract_hook_hot_trigger,
                    native_contract_hook_hot_contract = state_service_metrics.native_contract_hook_hot_contract,
                    native_contract_hook_hot_contract_id = state_service_metrics.native_contract_hook_hot_contract_id,
                    native_contract_hook_hot_avg_us = state_service_metrics.native_contract_hook_hot_avg_us,
                    neotoken_onpersist_hot_stage = state_service_metrics.neotoken_onpersist_hot_stage,
                    neotoken_onpersist_hot_stage_avg_us = state_service_metrics.neotoken_onpersist_hot_stage_avg_us,
                    neotoken_committee_compute_hot_stage = state_service_metrics.neotoken_committee_compute_hot_stage,
                    neotoken_committee_compute_hot_stage_avg_us = state_service_metrics.neotoken_committee_compute_hot_stage_avg_us,
                    neotoken_committee_candidate_hot_kind = state_service_metrics.neotoken_committee_candidate_hot_kind,
                    neotoken_committee_candidate_hot_avg = state_service_metrics.neotoken_committee_candidate_hot_avg,
                    state_service_mpt_apply_attempts = state_service_metrics.apply_attempts,
                    state_service_mpt_apply_failures = state_service_metrics.apply_failures,
                    state_service_mpt_apply_height = state_service_metrics.apply_height,
                    state_service_mpt_avg_total_us = state_service_metrics.avg_total_us,
                    state_service_mpt_avg_project_us = state_service_metrics.avg_project_us,
                    state_service_mpt_avg_trie_us = state_service_metrics.avg_trie_us,
                    state_service_mpt_avg_changes = state_service_metrics.avg_changes,
                    state_service_mpt_enqueue_blocking_avg_us = state_service_metrics.enqueue_blocking_avg_us,
                    state_service_mpt_queue_wait_avg_us = state_service_metrics.queue_wait_avg_us,
                    state_service_mpt_mutate_changes_avg_us = state_service_metrics.mutate_changes_avg_us,
                    state_service_mpt_root_hash_avg_us = state_service_metrics.root_hash_avg_us,
                    state_service_mpt_trie_commit_avg_us = state_service_metrics.trie_commit_avg_us,
                    state_service_mpt_backing_commit_avg_us = state_service_metrics.backing_commit_avg_us,
                    state_service_mpt_publish_generation_avg_us = state_service_metrics.publish_generation_avg_us,
                    state_service_mpt_overlay_entries_avg = state_service_metrics.overlay_entries_avg,
                    state_service_mpt_batch_blocks_avg = state_service_metrics.batch_blocks_avg,
                    rocksdb_batch_pending_operations = rocksdb_batch_metrics.map_or(0, |metrics| metrics.pending_operations),
                    rocksdb_batch_batches_flushed = rocksdb_batch_metrics.map_or(0, |metrics| metrics.batches_flushed),
                    rocksdb_batch_operations_written = rocksdb_batch_metrics.map_or(0, |metrics| metrics.operations_written),
                    rocksdb_batch_bytes_written = rocksdb_batch_metrics.map_or(0, |metrics| metrics.bytes_written),
                    rocksdb_batch_flush_timeouts = rocksdb_batch_metrics.map_or(0, |metrics| metrics.flush_timeouts),
                    rocksdb_batch_avg_ops_per_flush = rocksdb_batch_metrics.map_or(0, |metrics| metrics.avg_ops_per_flush),
                    rocksdb_batch_avg_bytes_per_flush = rocksdb_batch_metrics.map_or(0, |metrics| metrics.avg_bytes_per_flush),
                    rocksdb_batch_avg_flush_duration_ms = rocksdb_batch_metrics.map_or(0, |metrics| metrics.avg_flush_duration_ms),
                    rocksdb_batch_max_batch_size = rocksdb_batch_metrics.map_or(0, |metrics| metrics.max_batch_size),
                    rocksdb_batch_max_batch_bytes = rocksdb_batch_metrics.map_or(0, |metrics| metrics.max_batch_bytes),
                    rocksdb_batch_disable_wal = rocksdb_batch_metrics.is_some_and(|metrics| metrics.disable_wal),
                    "chain.acc import progress"
                );
            }
            if !batch_result.fully_imported() {
                let batch_start_record = (i + 1).saturating_sub(batch_result.len);
                let failed_record = batch_start_record + batch_result.imported;
                anyhow::bail!(
                    "partial chain.acc import at record {failed_record}: imported {} of {} blocks in batch, {imported} of {import_count} requested blocks imported",
                    batch_result.imported,
                    batch_result.len
                );
            }
            if reached_count_only_stop_height {
                break;
            }
        }
    }

    let elapsed_seconds = progress.elapsed_seconds();
    let driver_elapsed_seconds = driver_start.elapsed().as_secs_f64();
    let chain_acc_read_seconds = chain_acc_read_elapsed.as_secs_f64();
    let chain_acc_validate_seconds = chain_acc_validate_elapsed.as_secs_f64();
    let average_blocks_per_second = progress.average_blocks_per_second();
    let empty_block_import_seconds = composition.empty_block_import_seconds();
    let empty_blocks_per_second = composition.empty_blocks_per_second();
    let transaction_block_import_seconds = composition.transaction_block_import_seconds();
    let transaction_block_clone_seconds = composition.transaction_block_clone_seconds();
    let transaction_ledger_insert_seconds = composition.transaction_ledger_insert_seconds();
    let transaction_committed_hook_seconds = composition.transaction_committed_hook_seconds();
    let transaction_blocks_per_second = composition.transaction_blocks_per_second();
    let finalization_seconds = composition.finalization_seconds();
    let finalization_commit_handlers_seconds = composition.finalization_commit_handlers_seconds();
    let finalization_store_commit_seconds = composition.finalization_store_commit_seconds();
    let unclassified_import_seconds = composition.unclassified_import_seconds(progress.elapsed());
    info!(
        target: "neo::import",
        imported,
        elapsed_seconds,
        driver_elapsed_seconds,
        chain_acc_read_seconds,
        chain_acc_validate_seconds,
        average_blocks_per_second,
        empty_blocks = composition.empty_blocks,
        empty_only_blocks = composition.empty_only_blocks,
        empty_block_import_seconds,
        empty_blocks_per_second,
        transaction_blocks = composition.transaction_blocks,
        transactions = composition.transactions,
        transaction_block_import_seconds,
        transaction_block_clone_seconds,
        transaction_ledger_insert_seconds,
        transaction_committed_hook_seconds,
        transaction_blocks_per_second,
        finalization_seconds,
        finalization_commit_handlers_seconds,
        finalization_store_commit_seconds,
        unclassified_import_seconds,
        "chain.acc import complete"
    );
    Ok(ChainAccImportReport {
        imported,
        last_imported_tip,
        elapsed_seconds,
        driver_elapsed_seconds,
        chain_acc_read_seconds,
        chain_acc_validate_seconds,
        average_blocks_per_second,
        empty_blocks: composition.empty_blocks,
        empty_only_blocks: composition.empty_only_blocks,
        empty_block_import_seconds,
        empty_blocks_per_second,
        transaction_blocks: composition.transaction_blocks,
        transactions: composition.transactions,
        transaction_block_import_seconds,
        transaction_block_clone_seconds,
        transaction_ledger_insert_seconds,
        transaction_committed_hook_seconds,
        transaction_blocks_per_second,
        finalization_seconds,
        finalization_commit_handlers_seconds,
        finalization_store_commit_seconds,
        unclassified_import_seconds,
        hot_metrics,
    })
}

impl ImportHotMetrics {
    fn from_snapshots(
        state_service: &StateServiceMptImportMetrics,
        rocksdb: Option<RocksDbBatchImportMetrics>,
    ) -> Self {
        Self {
            state_service_mpt_apply_attempts: state_service.apply_attempts,
            state_service_mpt_apply_failures: state_service.apply_failures,
            state_service_mpt_apply_height: state_service.apply_height,
            state_service_mpt_avg_total_us: state_service.avg_total_us,
            state_service_mpt_avg_project_us: state_service.avg_project_us,
            state_service_mpt_avg_trie_us: state_service.avg_trie_us,
            state_service_mpt_avg_changes: state_service.avg_changes,
            state_service_mpt_enqueue_blocking_avg_us: state_service.enqueue_blocking_avg_us,
            state_service_mpt_queue_wait_avg_us: state_service.queue_wait_avg_us,
            state_service_mpt_mutate_changes_avg_us: state_service.mutate_changes_avg_us,
            state_service_mpt_root_hash_avg_us: state_service.root_hash_avg_us,
            state_service_mpt_trie_commit_avg_us: state_service.trie_commit_avg_us,
            state_service_mpt_backing_commit_avg_us: state_service.backing_commit_avg_us,
            state_service_mpt_publish_generation_avg_us: state_service.publish_generation_avg_us,
            state_service_mpt_overlay_entries_avg: state_service.overlay_entries_avg,
            state_service_mpt_batch_blocks_avg: state_service.batch_blocks_avg,
            native_persist_avg_total_us: state_service.native_persist_avg_total_us,
            native_persist_tx_hot_stage: state_service.native_persist_tx_hot_stage,
            native_persist_tx_hot_stage_avg_us: state_service.native_persist_tx_hot_stage_avg_us,
            rocksdb_batch_avg_flush_duration_ms: rocksdb
                .map_or(0, |metrics| metrics.avg_flush_duration_ms),
            rocksdb_batch_pending_operations: rocksdb
                .map_or(0, |metrics| metrics.pending_operations),
        }
    }
}

#[cfg(test)]
mod tests;
