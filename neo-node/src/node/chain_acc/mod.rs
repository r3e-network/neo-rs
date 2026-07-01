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
//! - `format`: chain.acc file format readers and validation helpers.
//! - `metrics`: Metrics collection and progress-reporting helpers.

use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
use neo_blockchain::command::ImportBlocksReply;
use neo_blockchain::command::ImportBlocksStats;
use neo_blockchain::handle::BlockchainHandle;
use neo_payloads::block::Block;
use neo_storage::persistence::store::Store;
use tracing::info;

mod format;
mod metrics;
use format::{read_chain_acc_header, read_next_chain_acc_block, skip_chain_acc_records};
use metrics::{
    ChainAccImportProgress, RocksDbBatchImportMetrics, StateServiceMptImportMetrics,
    should_log_import_progress,
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
        unclassified_import_seconds,
        hot_metrics,
    })
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct PendingChainAccBatch {
    len: usize,
    composition: ChainAccImportComposition,
    tip: Option<LocalLedgerTip>,
}

impl PendingChainAccBatch {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn record_pushed(&mut self, block: &Block) {
        self.len += 1;
        self.tip = Some(LocalLedgerTip {
            height: block.index(),
            hash: block.hash(),
        });
        let tx_count = block.transactions.len() as u64;
        if tx_count == 0 {
            self.composition.empty_blocks += 1;
        } else {
            self.composition.transaction_blocks += 1;
            self.composition.transactions += tx_count;
        }
    }

    #[cfg(test)]
    fn is_empty_only(&self) -> bool {
        self.len > 0 && self.composition.is_empty_only()
    }

    fn should_flush(&self, batch_len: usize) -> bool {
        debug_assert_eq!(self.len, batch_len);
        batch_len >= IMPORT_BATCH_SIZE
    }
}

struct ChainAccBatchImportResult {
    len: usize,
    imported: usize,
    elapsed: std::time::Duration,
    composition: ChainAccImportComposition,
    stats: ImportBlocksStats,
    tip: Option<LocalLedgerTip>,
}

impl ChainAccBatchImportResult {
    fn fully_imported(&self) -> bool {
        self.imported == self.len
    }
}

async fn import_chain_acc_batch(
    handle: &BlockchainHandle,
    batch_blocks: Vec<Block>,
    composition: ChainAccImportComposition,
    tip: Option<LocalLedgerTip>,
    verify: bool,
) -> anyhow::Result<ChainAccBatchImportResult> {
    let len = batch_blocks.len();
    let start = Instant::now();
    let reply = handle
        .import_blocks_bulk_detailed(batch_blocks, verify)
        .await?;
    let elapsed = start.elapsed();
    if let Some(error) = reply.error {
        anyhow::bail!(
            "block import finalization failed after importing {} blocks: {error}",
            reply.imported
        );
    }
    Ok(ChainAccBatchImportResult {
        len,
        imported: reply.imported,
        elapsed,
        composition,
        stats: reply.stats,
        tip,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ChainAccImportComposition {
    empty_blocks: u64,
    empty_only_blocks: u64,
    transaction_blocks: u64,
    transactions: u64,
    empty_elapsed: std::time::Duration,
    transaction_elapsed: std::time::Duration,
    transaction_block_clone_elapsed: std::time::Duration,
    transaction_ledger_insert_elapsed: std::time::Duration,
    transaction_committed_hook_elapsed: std::time::Duration,
    finalization_elapsed: std::time::Duration,
}

impl ChainAccImportComposition {
    #[cfg(test)]
    fn has_transaction_blocks(&self) -> bool {
        self.transaction_blocks > 0
    }

    #[cfg(test)]
    fn is_empty_only(&self) -> bool {
        self.empty_blocks > 0 && !self.has_transaction_blocks()
    }

    fn record_imported(
        &mut self,
        batch: Self,
        imported: usize,
        elapsed: std::time::Duration,
        stats: ImportBlocksStats,
    ) {
        if imported == 0 {
            return;
        }
        let imported = imported as u64;
        let batch_blocks = batch.empty_blocks + batch.transaction_blocks;
        if imported >= batch_blocks {
            self.empty_blocks += batch.empty_blocks;
            self.transaction_blocks += batch.transaction_blocks;
            self.transactions += batch.transactions;
            if stats.has_composition() {
                if stats.empty_blocks > 0 {
                    self.empty_only_blocks += stats.empty_blocks as u64;
                    self.empty_elapsed += stats.empty_elapsed;
                }
                if stats.transaction_blocks > 0 {
                    self.transaction_elapsed += stats.transaction_elapsed;
                    self.transaction_block_clone_elapsed += stats.transaction_block_clone_elapsed;
                    self.transaction_ledger_insert_elapsed +=
                        stats.transaction_ledger_insert_elapsed;
                    self.transaction_committed_hook_elapsed +=
                        stats.transaction_committed_hook_elapsed;
                }
                self.finalization_elapsed += stats.finalization_elapsed;
            } else if batch.transaction_blocks > 0 {
                self.transaction_elapsed += elapsed;
            } else if batch.empty_blocks > 0 {
                self.empty_only_blocks += batch.empty_blocks;
                self.empty_elapsed += elapsed;
            }
        }
    }

    fn empty_block_import_seconds(&self) -> f64 {
        self.empty_elapsed.as_secs_f64()
    }

    fn empty_blocks_per_second(&self) -> f64 {
        let elapsed = self.empty_block_import_seconds();
        if elapsed > 0.0 {
            self.empty_only_blocks as f64 / elapsed
        } else {
            0.0
        }
    }

    fn transaction_block_import_seconds(&self) -> f64 {
        self.transaction_elapsed.as_secs_f64()
    }

    fn transaction_block_clone_seconds(&self) -> f64 {
        self.transaction_block_clone_elapsed.as_secs_f64()
    }

    fn transaction_ledger_insert_seconds(&self) -> f64 {
        self.transaction_ledger_insert_elapsed.as_secs_f64()
    }

    fn transaction_committed_hook_seconds(&self) -> f64 {
        self.transaction_committed_hook_elapsed.as_secs_f64()
    }

    fn transaction_blocks_per_second(&self) -> f64 {
        let elapsed = self.transaction_block_import_seconds();
        if elapsed > 0.0 {
            self.transaction_blocks as f64 / elapsed
        } else {
            0.0
        }
    }

    fn finalization_seconds(&self) -> f64 {
        self.finalization_elapsed.as_secs_f64()
    }

    fn accounted_elapsed(&self) -> std::time::Duration {
        self.empty_elapsed
            + self.transaction_elapsed
            + self.transaction_block_clone_elapsed
            + self.transaction_ledger_insert_elapsed
            + self.transaction_committed_hook_elapsed
            + self.finalization_elapsed
    }

    fn unclassified_import_seconds(&self, total: std::time::Duration) -> f64 {
        total
            .checked_sub(self.accounted_elapsed())
            .unwrap_or_default()
            .as_secs_f64()
    }
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

fn take_import_batch(batch: &mut Vec<Block>, more_blocks_remain: bool) -> Vec<Block> {
    if more_blocks_remain {
        let next_batch = Vec::with_capacity(batch.capacity().max(IMPORT_BATCH_SIZE));
        std::mem::replace(batch, next_batch)
    } else {
        std::mem::take(batch)
    }
}

fn validate_chain_acc_count(count: usize, range: ChainAccExpectedRange) -> anyhow::Result<()> {
    let expected_count = expected_chain_acc_count(range)?;
    if count != expected_count {
        anyhow::bail!(
            "chain.acc count mismatch for expected range {}..={}: expected {expected_count} blocks, file has {count}",
            range.start_height,
            range.end_height
        );
    }
    Ok(())
}

fn bounded_chain_acc_import_range(
    expected_range: Option<ChainAccExpectedRange>,
    header_start_height: Option<u32>,
    stop_at_height: Option<u32>,
) -> Option<ChainAccExpectedRange> {
    if let Some(range) = expected_range {
        let Some(stop_at_height) = stop_at_height else {
            return Some(range);
        };
        if stop_at_height < range.start_height {
            return None;
        }
        return Some(ChainAccExpectedRange {
            start_height: range.start_height,
            end_height: range.end_height.min(stop_at_height),
        });
    }

    let start_height = header_start_height?;
    let stop_at_height = stop_at_height?;
    if stop_at_height < start_height {
        return None;
    }
    Some(ChainAccExpectedRange {
        start_height,
        end_height: stop_at_height,
    })
}

fn resume_chain_acc_import_range(
    import_range: Option<ChainAccExpectedRange>,
    local_tip: Option<&LocalLedgerTip>,
) -> anyhow::Result<Option<ChainAccExpectedRange>> {
    let Some(range) = import_range else {
        return Ok(None);
    };
    let Some(local_tip) = local_tip else {
        return Ok(Some(range));
    };

    if local_tip.height >= range.end_height {
        return Ok(None);
    }
    if local_tip.height < range.start_height {
        let Some(expected_previous_height) = range.start_height.checked_sub(1) else {
            return Ok(Some(range));
        };
        if local_tip.height != expected_previous_height {
            anyhow::bail!(
                "chain.acc expected range {}..={} requires local ledger tip at height {expected_previous_height} or inside the range, got {}",
                range.start_height,
                range.end_height,
                local_tip.height
            );
        }
        return Ok(Some(range));
    }

    let start_height = local_tip.height.checked_add(1).ok_or_else(|| {
        anyhow::anyhow!(
            "local ledger tip height {} cannot be advanced for chain.acc resume",
            local_tip.height
        )
    })?;
    Ok(Some(ChainAccExpectedRange {
        start_height,
        end_height: range.end_height,
    }))
}

fn chain_acc_import_record_count(
    file_count: usize,
    expected_range: Option<ChainAccExpectedRange>,
    import_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
) -> anyhow::Result<usize> {
    match (expected_range, import_range) {
        (Some(_), Some(range)) => expected_chain_acc_count(range),
        (Some(_), None) => Ok(0),
        (None, Some(range)) => expected_chain_acc_count(range).map(|count| count.min(file_count)),
        (None, None) if stop_at_height.is_some() => Ok(file_count),
        (None, _) => Ok(file_count),
    }
}

fn chain_acc_records_to_skip(
    file_count: usize,
    expected_range: Option<ChainAccExpectedRange>,
    header_start_height: Option<u32>,
    import_range: Option<ChainAccExpectedRange>,
) -> anyhow::Result<usize> {
    let Some(import_range) = import_range else {
        return Ok(0);
    };
    let Some(file_start_height) = expected_range
        .map(|range| range.start_height)
        .or(header_start_height)
    else {
        return Ok(0);
    };
    let skip = import_range
        .start_height
        .checked_sub(file_start_height)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chain.acc import start {} is before file start {file_start_height}",
                import_range.start_height
            )
        })? as usize;
    if skip > file_count {
        anyhow::bail!(
            "chain.acc import start {} skips {skip} records, but file has only {file_count} records",
            import_range.start_height
        );
    }
    Ok(skip)
}

fn count_only_stop_height_reached(
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    block_height: u32,
) -> bool {
    expected_range.is_none() && stop_at_height.is_some_and(|target| block_height >= target)
}

fn count_only_stop_height_exceeded(
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    block_height: u32,
) -> bool {
    expected_range.is_none() && stop_at_height.is_some_and(|target| block_height > target)
}

fn validate_chain_acc_block_height(
    record: usize,
    height: u32,
    header_start_height: Option<u32>,
    expected_range: Option<ChainAccExpectedRange>,
    expected_count: Option<usize>,
    previous_height: &mut Option<u32>,
) -> anyhow::Result<()> {
    if record == 0 {
        if let Some(expected_first_height) = expected_range
            .map(|range| range.start_height)
            .or(header_start_height)
        {
            if height != expected_first_height {
                anyhow::bail!(
                    "chain.acc first block height mismatch: expected {expected_first_height}, got {height}"
                );
            }
        }
    } else if let Some(previous) = previous_height {
        if height != previous.saturating_add(1) {
            anyhow::bail!(
                "chain.acc block heights are not contiguous at record {record}: expected {}, got {height}",
                previous.saturating_add(1)
            );
        }
    }

    if let (Some(range), Some(expected_count)) = (expected_range, expected_count) {
        if record + 1 == expected_count && height != range.end_height {
            anyhow::bail!(
                "chain.acc last block height mismatch: expected {}, got {height}",
                range.end_height
            );
        }
    }

    *previous_height = Some(height);
    Ok(())
}

fn expected_chain_acc_first_prev_hash(
    expected_range: Option<ChainAccExpectedRange>,
    local_tip: Option<&LocalLedgerTip>,
) -> anyhow::Result<Option<neo_primitives::UInt256>> {
    let Some(range) = expected_range else {
        return Ok(None);
    };
    if range.start_height == 0 {
        return Ok(None);
    }
    let Some(local_tip) = local_tip else {
        anyhow::bail!(
            "chain.acc partial expected range {}..={} requires local storage for previous hash validation",
            range.start_height,
            range.end_height
        );
    };
    let expected_previous_height = range.start_height.checked_sub(1).ok_or_else(|| {
        anyhow::anyhow!(
            "chain.acc expected range is invalid: {}..={}",
            range.start_height,
            range.end_height
        )
    })?;
    if local_tip.height != expected_previous_height {
        anyhow::bail!(
            "chain.acc partial expected range {}..={} requires local ledger tip at height {expected_previous_height}, got {}",
            range.start_height,
            range.end_height,
            local_tip.height
        );
    }
    Ok(Some(local_tip.hash))
}

fn validate_chain_acc_first_prev_hash(
    record: usize,
    block: &Block,
    expected_prev_hash: Option<&neo_primitives::UInt256>,
) -> anyhow::Result<()> {
    let Some(expected_prev_hash) = expected_prev_hash else {
        return Ok(());
    };
    if record != 0 {
        return Ok(());
    }
    if block.prev_hash() != expected_prev_hash {
        anyhow::bail!(
            "chain.acc previous hash mismatch at first imported block {}: expected local tip hash {}, got {}",
            block.index(),
            expected_prev_hash,
            block.prev_hash()
        );
    }
    Ok(())
}

fn validate_chain_acc_internal_prev_hash(
    record: usize,
    block: &Block,
    previous_hash: Option<&neo_primitives::UInt256>,
) -> anyhow::Result<()> {
    let Some(previous_hash) = previous_hash else {
        return Ok(());
    };
    if block.prev_hash() != previous_hash {
        anyhow::bail!(
            "chain.acc previous hash mismatch at record {record}, block {}: expected previous block hash {}, got {}",
            block.index(),
            previous_hash,
            block.prev_hash()
        );
    }
    Ok(())
}

fn expected_chain_acc_count(range: ChainAccExpectedRange) -> anyhow::Result<usize> {
    Ok(range
        .end_height
        .checked_sub(range.start_height)
        .and_then(|span| span.checked_add(1))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chain.acc expected range is invalid: {}..={}",
                range.start_height,
                range.end_height
            )
        })? as usize)
}

#[cfg(test)]
mod tests {
    use super::format::tests::{
        empty_block, empty_block_with_prev_hash, encode_chain_acc, linked_empty_blocks,
    };
    use super::*;
    use neo_blockchain::BlockchainCommand;
    use neo_payloads::{Signer, Transaction, Witness};
    use neo_primitives::{UInt160, WitnessScope};

    fn signed_test_transaction(nonce: u32) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_script(vec![neo_vm_rs::OpCode::RET.byte()]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_witnesses(vec![Witness::new_with_scripts(
            Vec::new(),
            vec![neo_vm_rs::OpCode::PUSH1.byte()],
        )]);
        tx
    }

    fn non_empty_block_with_prev_hash(
        index: u32,
        prev_hash: neo_primitives::UInt256,
        transactions: Vec<Transaction>,
    ) -> Block {
        let mut header = neo_payloads::Header::new();
        header.set_index(index);
        header.set_prev_hash(prev_hash);
        let mut block = Block::from_parts(header, transactions);
        block.try_rebuild_merkle_root().expect("merkle root");
        block
    }

    fn memory_store_with_ledger_tip(tip: u32, hash: neo_primitives::UInt256) -> Arc<dyn Store> {
        use neo_storage::persistence::providers::memory_store::MemoryStore;
        use neo_storage::{StorageItem, StorageKey};

        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let mut cache =
            neo_storage::persistence::StoreCache::new_from_store(Arc::clone(&store), false);
        let current = neo_native_contracts::LedgerContract::new()
            .serialize_hash_index_state(&hash, tip)
            .expect("serialize current ledger pointer");
        cache.data_cache().add(
            StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
            StorageItem::from_bytes(current),
        );
        cache.commit();
        store
    }

    #[test]
    fn take_import_batch_preserves_preallocated_capacity_when_more_blocks_remain() {
        let mut batch = Vec::with_capacity(IMPORT_BATCH_SIZE);
        batch.push(empty_block(1));

        let imported = take_import_batch(&mut batch, true);

        assert_eq!(imported.len(), 1);
        assert_eq!(batch.len(), 0);
        assert!(
            batch.capacity() >= IMPORT_BATCH_SIZE,
            "expected to preserve batch capacity, got {}",
            batch.capacity()
        );
    }

    #[test]
    fn take_import_batch_avoids_reallocating_after_final_flush() {
        let mut batch = Vec::with_capacity(IMPORT_BATCH_SIZE);
        batch.push(empty_block(1));

        let imported = take_import_batch(&mut batch, false);

        assert_eq!(imported.len(), 1);
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.capacity(), 0);
    }

    #[test]
    fn bounded_chain_acc_import_range_caps_only_within_expected_range() {
        let full = ChainAccExpectedRange {
            start_height: 10,
            end_height: 20,
        };

        assert_eq!(
            bounded_chain_acc_import_range(Some(full), None, None),
            Some(full)
        );
        assert_eq!(
            bounded_chain_acc_import_range(Some(full), None, Some(15)),
            Some(ChainAccExpectedRange {
                start_height: 10,
                end_height: 15,
            })
        );
        assert_eq!(
            bounded_chain_acc_import_range(Some(full), None, Some(25)),
            Some(full)
        );
        assert_eq!(
            bounded_chain_acc_import_range(Some(full), None, Some(9)),
            None
        );
        assert_eq!(bounded_chain_acc_import_range(None, None, Some(15)), None);
        assert_eq!(
            bounded_chain_acc_import_range(None, Some(0), Some(15)),
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 15,
            })
        );
    }

    #[test]
    fn chain_acc_import_record_count_uses_bounded_expected_range() {
        let full = ChainAccExpectedRange {
            start_height: 10,
            end_height: 20,
        };
        let bounded = ChainAccExpectedRange {
            start_height: 10,
            end_height: 15,
        };

        assert_eq!(
            chain_acc_import_record_count(11, Some(full), Some(bounded), Some(15))
                .expect("bounded count"),
            6
        );
        assert_eq!(
            chain_acc_import_record_count(11, Some(full), None, Some(9)).expect("below-range stop"),
            0
        );
        assert_eq!(
            chain_acc_import_record_count(11, None, None, None).expect("unbounded count"),
            11
        );
        assert_eq!(
            chain_acc_import_record_count(11, None, Some(bounded), Some(15))
                .expect("prefixed count-only bound"),
            6
        );
        assert_eq!(
            chain_acc_import_record_count(11, None, None, Some(15))
                .expect("unprefixed count-only bound"),
            11
        );
    }

    #[test]
    fn count_only_stop_height_reached_only_applies_without_expected_range() {
        let full = ChainAccExpectedRange {
            start_height: 10,
            end_height: 20,
        };

        assert!(count_only_stop_height_reached(None, Some(2), 2));
        assert!(count_only_stop_height_reached(None, Some(2), 3));
        assert!(!count_only_stop_height_reached(None, Some(2), 1));
        assert!(!count_only_stop_height_reached(None, None, 2));
        assert!(!count_only_stop_height_reached(Some(full), Some(15), 15));
        assert!(count_only_stop_height_exceeded(None, Some(2), 3));
        assert!(!count_only_stop_height_exceeded(None, Some(2), 2));
        assert!(!count_only_stop_height_exceeded(Some(full), Some(15), 16));
    }

    #[tokio::test]
    async fn import_chain_acc_can_stop_count_only_file_before_full_end() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&linked_empty_blocks(0, 5));
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            let heights = import
                .blocks
                .iter()
                .map(|block| block.index())
                .collect::<Vec<_>>();
            assert_eq!(heights, vec![0, 1, 2]);
            reply
                .send(ImportBlocksReply::ok(import.blocks.len()))
                .expect("reply import");
            assert!(
                commands.try_recv().is_err(),
                "stop height should prevent importing blocks beyond the bound"
            );
        });

        let imported = import_chain_acc_from_reader_until_height(
            &handle,
            &mut cursor,
            None,
            false,
            None,
            Some(2),
            None,
        )
        .await
        .expect("count-only import should stop at requested height");

        service.await.expect("service task");
        assert_eq!(imported, 3);
    }

    #[tokio::test]
    async fn import_chain_acc_until_height_public_wrapper_bounds_count_only_file() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let temp = tempfile::NamedTempFile::new().expect("temp chain.acc");
        std::fs::write(temp.path(), encode_chain_acc(&linked_empty_blocks(0, 5)))
            .expect("write chain.acc");
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            let heights = import
                .blocks
                .iter()
                .map(|block| block.index())
                .collect::<Vec<_>>();
            assert_eq!(heights, vec![0, 1, 2]);
            reply
                .send(ImportBlocksReply::ok(import.blocks.len()))
                .expect("reply import");
        });

        let imported = import_chain_acc_until_height(&handle, temp.path(), false, Some(2), None)
            .await
            .expect("bounded public import should stop at requested height");

        service.await.expect("service task");
        assert_eq!(imported, 3);
    }

    #[tokio::test]
    async fn import_chain_acc_errors_when_blockchain_accepts_partial_batch() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&linked_empty_blocks(0, 2));
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 2);
            reply
                .send(ImportBlocksReply::ok(1))
                .expect("reply partial import");
        });

        let err = import_chain_acc_from_reader(&handle, &mut cursor, None, false, None, None)
            .await
            .expect_err("partial import must be an error");

        service.await.expect("service task");
        assert!(
            err.to_string().contains("partial chain.acc import"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("imported 1 of 2"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_errors_when_blockchain_finalization_fails() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&linked_empty_blocks(0, 2));
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 2);
            reply
                .send(ImportBlocksReply::failed(
                    2,
                    "state-root worker reported a failed operation",
                ))
                .expect("reply failed finalization");
        });

        let err = import_chain_acc_from_reader(&handle, &mut cursor, None, false, None, None)
            .await
            .expect_err("finalization failure must be an error");

        service.await.expect("service task");
        assert!(
            err.to_string().contains("finalization failed"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("state-root worker"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_rejects_expected_range_count_mismatch_before_import() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&[empty_block(0)]);
        let mut cursor = std::io::Cursor::new(bytes);

        let err = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 1,
            }),
            None,
        )
        .await
        .expect_err("count mismatch must be an error");

        assert!(
            commands.try_recv().is_err(),
            "range validation must fail before import"
        );
        assert!(
            err.to_string().contains("count mismatch"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_rejects_expected_range_first_height_mismatch_before_import() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&[empty_block(1)]);
        let mut cursor = std::io::Cursor::new(bytes);

        let err = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 0,
            }),
            None,
        )
        .await
        .expect_err("first height mismatch must be an error");

        assert!(
            commands.try_recv().is_err(),
            "range validation must fail before import"
        );
        assert!(
            err.to_string().contains("first block height mismatch"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_rejects_non_contiguous_heights_before_import() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&[empty_block(0), empty_block(2)]);
        let mut cursor = std::io::Cursor::new(bytes);

        let err = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 1,
            }),
            None,
        )
        .await
        .expect_err("height gap must be an error");

        assert!(
            commands.try_recv().is_err(),
            "range validation must fail before import"
        );
        assert!(
            err.to_string().contains("not contiguous"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_rejects_partial_range_prev_hash_mismatch_before_import() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let local_tip_hash = neo_primitives::UInt256::from([0xAA; 32]);
        let store = memory_store_with_ledger_tip(9, local_tip_hash);
        let wrong_prev_hash = neo_primitives::UInt256::from([0xBB; 32]);
        let bytes = encode_chain_acc(&[empty_block_with_prev_hash(10, wrong_prev_hash)]);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                return false;
            };
            let imported = import.blocks.len();
            reply
                .send(ImportBlocksReply::ok(imported))
                .expect("reply import");
            true
        });

        let result = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 10,
                end_height: 10,
            }),
            Some(store),
        )
        .await;
        service.abort();

        assert!(
            result.is_err(),
            "partial package with mismatched previous hash must fail, got {result:?}"
        );
        let err = result.expect_err("partial package with mismatched previous hash must fail");
        assert!(
            err.to_string().contains("previous hash mismatch"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_rejects_internal_prev_hash_mismatch_before_import() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let genesis = empty_block(0);
        let wrong_prev_hash = neo_primitives::UInt256::from([0xCC; 32]);
        let bytes = encode_chain_acc(&[genesis, empty_block_with_prev_hash(1, wrong_prev_hash)]);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Ok(Some(BlockchainCommand::ImportBlocks { import, reply })) =
                tokio::time::timeout(std::time::Duration::from_millis(50), commands.recv()).await
            else {
                return false;
            };
            let imported = import.blocks.len();
            reply
                .send(ImportBlocksReply::ok(imported))
                .expect("reply import");
            true
        });

        let result = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 1,
            }),
            None,
        )
        .await;
        let import_reached_service = service.await.expect("service task");

        assert!(
            result.is_err(),
            "internal previous-hash mismatch must fail before import, got {result:?}"
        );
        assert!(
            !import_reached_service,
            "internal previous-hash validation must fail before sending an import command"
        );
        let err = result.expect_err("internal previous-hash mismatch must fail");
        assert!(
            err.to_string().contains("previous hash mismatch"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("record 1"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_allows_partial_range_prev_hash_match() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let local_tip_hash = neo_primitives::UInt256::from([0xAA; 32]);
        let store = memory_store_with_ledger_tip(9, local_tip_hash);
        let bytes = encode_chain_acc(&[empty_block_with_prev_hash(10, local_tip_hash)]);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 1);
            assert_eq!(import.blocks[0].index(), 10);
            reply.send(ImportBlocksReply::ok(1)).expect("reply import");
        });

        let imported = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 10,
                end_height: 10,
            }),
            Some(store),
        )
        .await
        .expect("matching previous hash can import");

        service.await.expect("service task");
        assert_eq!(imported, 1);
    }

    #[tokio::test]
    async fn import_chain_acc_can_stop_before_full_expected_range_end() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&linked_empty_blocks(0, 5));
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            let heights = import
                .blocks
                .iter()
                .map(|block| block.index())
                .collect::<Vec<_>>();
            assert_eq!(heights, vec![0, 1, 2]);
            reply
                .send(ImportBlocksReply::ok(import.blocks.len()))
                .expect("reply import");
            assert!(
                commands.try_recv().is_err(),
                "stop height should prevent importing blocks beyond the bound"
            );
        });

        let imported = import_chain_acc_from_reader_until_height(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 4,
            }),
            Some(2),
            None,
        )
        .await
        .expect("bounded expected-range import should stop at requested height");

        service.await.expect("service task");
        assert_eq!(imported, 3);
    }

    #[tokio::test]
    async fn import_chain_acc_resumes_full_expected_range_after_local_tip() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let blocks = linked_empty_blocks(0, 5);
        let local_tip_hash = blocks[2].hash();
        let store = memory_store_with_ledger_tip(2, local_tip_hash);
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            let heights = import
                .blocks
                .iter()
                .map(|block| block.index())
                .collect::<Vec<_>>();
            assert_eq!(heights, vec![3, 4]);
            reply
                .send(ImportBlocksReply::ok(import.blocks.len()))
                .expect("reply import");
            assert!(
                commands.try_recv().is_err(),
                "resuming after local tip should not import earlier package blocks"
            );
        });

        let imported = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 4,
            }),
            Some(store),
        )
        .await
        .expect("full package import should resume after local tip");

        service.await.expect("service task");
        assert_eq!(imported, 2);
    }

    #[tokio::test]
    async fn import_chain_acc_report_tracks_last_imported_tip() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let blocks = linked_empty_blocks(0, 3);
        let expected_tip = LocalLedgerTip {
            height: blocks[2].index(),
            hash: blocks[2].hash(),
        };
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 3);
            reply
                .send(ImportBlocksReply::ok(import.blocks.len()))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 2,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, 3);
        assert_eq!(report.last_imported_tip, Some(expected_tip));
    }

    #[tokio::test]
    async fn import_chain_acc_report_tracks_final_average_bps() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&linked_empty_blocks(0, 3));
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 3);
            reply
                .send(ImportBlocksReply::ok(import.blocks.len()))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 2,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, 3);
        assert!(report.elapsed_seconds >= 0.0);
        assert!(
            report.average_blocks_per_second > 0.0,
            "importing blocks should report a positive final BPS, got {report:?}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_report_tracks_empty_and_transaction_bearing_blocks() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let genesis = empty_block(0);
        let block1 =
            non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
        let block2 = empty_block_with_prev_hash(2, block1.hash());
        let blocks = vec![genesis, block1, block2];
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 3);
            reply
                .send(ImportBlocksReply::ok_with_stats(
                    import.blocks.len(),
                    neo_blockchain::command::ImportBlocksStats {
                        empty_blocks: 2,
                        empty_elapsed: std::time::Duration::from_millis(2),
                        transaction_blocks: 1,
                        transaction_elapsed: std::time::Duration::from_millis(1),
                        transaction_block_clone_elapsed: std::time::Duration::from_millis(3),
                        transaction_ledger_insert_elapsed: std::time::Duration::from_millis(4),
                        transaction_committed_hook_elapsed: std::time::Duration::from_millis(5),
                        finalization_elapsed: std::time::Duration::from_millis(1),
                    },
                ))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 2,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, 3);
        assert_eq!(report.empty_blocks, 2);
        assert_eq!(report.empty_only_blocks, 2);
        assert!(report.empty_blocks_per_second > 0.0);
        assert_eq!(report.transaction_blocks, 1);
        assert_eq!(report.transactions, 1);
        assert_eq!(report.transaction_block_clone_seconds, 0.003);
        assert_eq!(report.transaction_ledger_insert_seconds, 0.004);
        assert_eq!(report.transaction_committed_hook_seconds, 0.005);
        assert_eq!(report.finalization_seconds, 0.001);
        assert!(
            report.transaction_blocks_per_second > 0.0,
            "transaction-bearing BPS must be reported independently from empty-block throughput"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_report_times_only_transaction_bearing_batches_for_transaction_bps() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let empty_run =
            neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
        let mut blocks = linked_empty_blocks(0, empty_run);
        let prev = blocks.last().expect("previous block");
        blocks.push(non_empty_block_with_prev_hash(
            empty_run as u32,
            prev.hash(),
            vec![signed_test_transaction(1)],
        ));
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), empty_run + 1);
            assert!(
                import.blocks[..empty_run]
                    .iter()
                    .all(|block| block.transactions.is_empty())
            );
            assert_eq!(import.blocks[empty_run].transactions.len(), 1);
            reply
                .send(ImportBlocksReply::ok_with_stats(
                    import.blocks.len(),
                    neo_blockchain::command::ImportBlocksStats {
                        empty_blocks: empty_run,
                        empty_elapsed: std::time::Duration::from_millis(20),
                        transaction_blocks: 1,
                        transaction_elapsed: std::time::Duration::from_millis(1),
                        transaction_block_clone_elapsed: std::time::Duration::ZERO,
                        transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                        transaction_committed_hook_elapsed: std::time::Duration::ZERO,
                        finalization_elapsed: std::time::Duration::from_millis(1),
                    },
                ))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: empty_run as u32,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, (empty_run + 1) as u64);
        assert_eq!(report.empty_blocks, empty_run as u64);
        assert_eq!(report.empty_only_blocks, empty_run as u64);
        assert!(
            report.empty_block_import_seconds >= 0.02,
            "empty-block elapsed should include the empty-only batch time: {report:?}"
        );
        assert!(
            report.empty_blocks_per_second > 0.0,
            "empty-block BPS should be reported independently from transaction-bearing throughput"
        );
        assert_eq!(report.transaction_blocks, 1);
        assert_eq!(report.transactions, 1);
        assert_eq!(report.transaction_block_import_seconds, 0.001);
        assert!(
            report.transaction_block_import_seconds < report.empty_block_import_seconds,
            "transaction elapsed must exclude empty fast-forward service time: {report:?}"
        );
        assert!(
            (report.transaction_blocks_per_second - 1000.0).abs() < f64::EPSILON,
            "transaction BPS should use transaction-bearing service time: {report:?}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_uses_fast_forward_sized_batches_for_empty_runs() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let empty_run =
            neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
        let mut blocks = linked_empty_blocks(0, empty_run);
        let prev = blocks.last().expect("previous block");
        blocks.push(non_empty_block_with_prev_hash(
            empty_run as u32,
            prev.hash(),
            vec![signed_test_transaction(1)],
        ));
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), empty_run + 1);
            assert!(
                import.blocks[..empty_run]
                    .iter()
                    .all(|block| block.transactions.is_empty())
            );
            assert_eq!(import.blocks[empty_run].transactions.len(), 1);
            reply
                .send(ImportBlocksReply::ok_with_stats(
                    import.blocks.len(),
                    neo_blockchain::command::ImportBlocksStats {
                        empty_blocks: empty_run,
                        empty_elapsed: std::time::Duration::from_millis(20),
                        transaction_blocks: 1,
                        transaction_elapsed: std::time::Duration::from_millis(1),
                        transaction_block_clone_elapsed: std::time::Duration::ZERO,
                        transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                        transaction_committed_hook_elapsed: std::time::Duration::ZERO,
                        finalization_elapsed: std::time::Duration::from_millis(1),
                    },
                ))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: empty_run as u32,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, (empty_run + 1) as u64);
        assert_eq!(report.empty_only_blocks, empty_run as u64);
        assert_eq!(report.transaction_blocks, 1);
    }

    #[tokio::test]
    async fn import_chain_acc_keeps_short_empty_prefix_with_transaction_block() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let empty_run = 24;
        let mut blocks = linked_empty_blocks(0, empty_run);
        let prev = blocks.last().expect("previous block");
        blocks.push(non_empty_block_with_prev_hash(
            empty_run as u32,
            prev.hash(),
            vec![signed_test_transaction(1)],
        ));
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), empty_run + 1);
            assert_eq!(import.blocks[empty_run].transactions.len(), 1);
            reply
                .send(ImportBlocksReply::ok_with_stats(
                    import.blocks.len(),
                    neo_blockchain::command::ImportBlocksStats {
                        empty_blocks: empty_run,
                        empty_elapsed: std::time::Duration::from_millis(20),
                        transaction_blocks: 1,
                        transaction_elapsed: std::time::Duration::from_millis(1),
                        transaction_block_clone_elapsed: std::time::Duration::ZERO,
                        transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                        transaction_committed_hook_elapsed: std::time::Duration::ZERO,
                        finalization_elapsed: std::time::Duration::from_millis(1),
                    },
                ))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: empty_run as u32,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, (empty_run + 1) as u64);
        assert_eq!(report.empty_blocks, empty_run as u64);
        assert_eq!(report.empty_only_blocks, empty_run as u64);
        assert!(
            report.empty_block_import_seconds >= 0.02,
            "short empty-prefix elapsed should come from service-side empty timing: {report:?}"
        );
        assert_eq!(report.transaction_blocks, 1);
        assert_eq!(report.transactions, 1);
        assert!(
            report.transaction_block_import_seconds < report.empty_block_import_seconds,
            "transaction elapsed must exclude short empty-prefix service time: {report:?}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_keeps_short_empty_suffix_after_transaction_block() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let genesis = empty_block(0);
        let tx =
            non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
        let mut blocks = vec![genesis, tx];
        for index in 2..26 {
            let prev = blocks.last().expect("previous block");
            blocks.push(empty_block_with_prev_hash(index, prev.hash()));
        }
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(import.blocks.len(), 26);
            assert_eq!(import.blocks[1].transactions.len(), 1);
            reply
                .send(ImportBlocksReply::ok_with_stats(
                    import.blocks.len(),
                    neo_blockchain::command::ImportBlocksStats {
                        empty_blocks: 25,
                        empty_elapsed: std::time::Duration::from_millis(20),
                        transaction_blocks: 1,
                        transaction_elapsed: std::time::Duration::from_millis(1),
                        transaction_block_clone_elapsed: std::time::Duration::ZERO,
                        transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                        transaction_committed_hook_elapsed: std::time::Duration::ZERO,
                        finalization_elapsed: std::time::Duration::from_millis(1),
                    },
                ))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 25,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, 26);
        assert_eq!(report.empty_blocks, 25);
        assert_eq!(report.empty_only_blocks, 25);
        assert!(
            report.empty_block_import_seconds >= 0.02,
            "short empty suffix elapsed should come from service-side empty timing: {report:?}"
        );
        assert_eq!(report.transaction_blocks, 1);
        assert_eq!(report.transactions, 1);
        assert!(
            report.transaction_block_import_seconds < report.empty_block_import_seconds,
            "transaction elapsed must exclude short empty-suffix service time: {report:?}"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_uses_service_timing_without_splitting_mixed_batches() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let genesis = empty_block(0);
        let tx =
            non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
        let mut blocks = vec![genesis, tx];
        for index in 2..26 {
            let prev = blocks.last().expect("previous block");
            blocks.push(empty_block_with_prev_hash(index, prev.hash()));
        }
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                panic!("expected import blocks command");
            };
            assert_eq!(
                import.blocks.len(),
                26,
                "short mixed runs should stay in one bulk import command"
            );
            reply
                .send(ImportBlocksReply::ok_with_stats(
                    import.blocks.len(),
                    neo_blockchain::command::ImportBlocksStats {
                        empty_blocks: 25,
                        empty_elapsed: std::time::Duration::from_millis(20),
                        transaction_blocks: 1,
                        transaction_elapsed: std::time::Duration::from_millis(5),
                        transaction_block_clone_elapsed: std::time::Duration::ZERO,
                        transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                        transaction_committed_hook_elapsed: std::time::Duration::ZERO,
                        finalization_elapsed: std::time::Duration::from_millis(2),
                    },
                ))
                .expect("reply import");
        });

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 25,
            }),
            None,
        )
        .await
        .expect("import report");

        service.await.expect("service task");
        assert_eq!(report.imported, 26);
        assert_eq!(report.empty_blocks, 25);
        assert_eq!(report.empty_only_blocks, 25);
        assert_eq!(report.transaction_blocks, 1);
        assert_eq!(report.transactions, 1);
        assert!(
            report.empty_block_import_seconds >= 0.02,
            "empty elapsed should come from service-side fast-forward timing: {report:?}"
        );
        assert!(
            report.transaction_block_import_seconds >= 0.005,
            "transaction elapsed should come from service-side transaction timing: {report:?}"
        );
        assert!(
            report.transaction_block_import_seconds < report.empty_block_import_seconds,
            "service timing must let transaction proof exclude empty fast-forward time: {report:?}"
        );
    }

    #[test]
    fn chain_acc_batch_keeps_short_mixed_runs_until_normal_boundary() {
        let empty_limit =
            neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
        let mut small_empty_prefix = linked_empty_blocks(0, empty_limit - 1);
        let mut pending = PendingChainAccBatch::default();
        for block in &small_empty_prefix {
            pending.record_pushed(block);
        }
        let prev = small_empty_prefix.last().expect("previous block");
        let next = non_empty_block_with_prev_hash(
            (empty_limit - 1) as u32,
            prev.hash(),
            vec![signed_test_transaction(1)],
        );
        assert!(
            !pending.should_flush(small_empty_prefix.len()),
            "short empty prefixes should stay in the mixed bulk import; service-side stats separate their timing"
        );
        pending.record_pushed(&next);
        small_empty_prefix.push(next);
        assert!(
            !pending.should_flush(small_empty_prefix.len()),
            "transaction runs still wait for the normal import boundary until the next empty block"
        );
        let following_empty = empty_block_with_prev_hash(
            small_empty_prefix.len() as u32,
            small_empty_prefix.last().expect("previous block").hash(),
        );
        pending.record_pushed(&following_empty);
        small_empty_prefix.push(following_empty);
        assert!(
            !pending.should_flush(small_empty_prefix.len()),
            "short empty suffixes should not force an extra bulk finalization boundary"
        );

        let empty_run = empty_limit;
        let mut blocks = linked_empty_blocks(0, empty_run);
        let mut pending = PendingChainAccBatch::default();
        for block in &blocks {
            pending.record_pushed(block);
        }
        assert!(
            !pending.should_flush(blocks.len()),
            "empty-only outer chain.acc batches do not flush at the service-internal fast-forward chunk size"
        );
        let prev = blocks.last().expect("previous block");
        blocks.push(non_empty_block_with_prev_hash(
            empty_run as u32,
            prev.hash(),
            vec![signed_test_transaction(1)],
        ));
        pending.record_pushed(blocks.last().expect("transaction block"));

        assert!(
            !pending.should_flush(blocks.len()),
            "transaction blocks can share the outer batch with a fast-forwardable empty prefix"
        );
    }

    #[test]
    fn pending_chain_acc_batch_tracks_transaction_presence_without_scanning_blocks() {
        let mut pending = PendingChainAccBatch::default();
        let empty = empty_block(0);
        pending.record_pushed(&empty);

        assert!(pending.is_empty_only());
        assert!(!pending.should_flush(1));

        let tx = non_empty_block_with_prev_hash(1, empty.hash(), vec![signed_test_transaction(1)]);
        pending.record_pushed(&tx);

        assert!(!pending.is_empty_only());
        assert!(!pending.should_flush(2));
        for index in 2..IMPORT_BATCH_SIZE {
            pending.record_pushed(&empty_block(index as u32));
        }
        assert!(!pending.is_empty_only());
        assert!(pending.should_flush(IMPORT_BATCH_SIZE));
    }

    #[test]
    fn empty_only_chain_acc_batches_flush_at_outer_import_boundary() {
        let mut pending = PendingChainAccBatch::default();
        let empty = empty_block(0);

        let max = neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
        for _ in 0..max {
            pending.record_pushed(&empty);
        }

        assert!(
            !pending.should_flush(max),
            "chain.acc owns only the outer import boundary; the blockchain service chunks empty runs internally"
        );
        for _ in max..IMPORT_BATCH_SIZE {
            pending.record_pushed(&empty);
        }
        assert!(pending.should_flush(IMPORT_BATCH_SIZE));
    }

    #[test]
    fn chain_acc_batch_import_uses_tracked_composition_without_rescanning_blocks() {
        let source = include_str!("mod.rs");
        let batch_import = source
            .split("async fn import_chain_acc_batch")
            .nth(1)
            .and_then(|tail| {
                tail.split("#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]")
                    .next()
            })
            .expect("import_chain_acc_batch source");

        assert!(
            !batch_import.contains("ChainAccImportComposition::from_blocks(&batch_blocks)"),
            "chain.acc import should reuse composition tracked while reading, not rescan every batch before dispatch"
        );
    }

    #[test]
    fn pending_chain_acc_batch_derives_transaction_presence_from_composition() {
        let source = include_str!("mod.rs");
        let pending_batch = source
            .split("struct PendingChainAccBatch")
            .nth(1)
            .and_then(|tail| tail.split("struct ChainAccBatchImportResult").next())
            .expect("PendingChainAccBatch source");

        assert!(
            !pending_batch.contains("has_transactions"),
            "pending chain.acc batch should not duplicate transaction-presence state once composition is tracked"
        );
    }

    #[test]
    fn chain_acc_batch_import_uses_tracked_tip_without_rehashing_last_block() {
        let source = include_str!("mod.rs");
        let batch_import = source
            .split("async fn import_chain_acc_batch")
            .nth(1)
            .and_then(|tail| {
                tail.split("#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]")
                    .next()
            })
            .expect("import_chain_acc_batch source");

        assert!(
            !batch_import.contains("batch_blocks.last().map"),
            "chain.acc import should reuse the tip tracked while reading, not rehash the last block before dispatch"
        );
    }

    #[tokio::test]
    async fn import_chain_acc_report_uses_zero_bps_for_noop_resume() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let blocks = linked_empty_blocks(0, 3);
        let local_tip_hash = blocks[2].hash();
        let store = memory_store_with_ledger_tip(2, local_tip_hash);
        let bytes = encode_chain_acc(&blocks);
        let mut cursor = std::io::Cursor::new(bytes);

        let report = import_chain_acc_from_reader_report(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 0,
                end_height: 2,
            }),
            Some(store),
        )
        .await
        .expect("noop report");

        assert!(
            commands.try_recv().is_err(),
            "noop resume should not import"
        );
        assert_eq!(report.imported, 0);
        assert_eq!(report.average_blocks_per_second, 0.0);
    }

    #[tokio::test]
    async fn import_chain_acc_rejects_partial_range_without_storage_before_import() {
        let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
        let bytes = encode_chain_acc(&[empty_block(10)]);
        let mut cursor = std::io::Cursor::new(bytes);
        let service = tokio::spawn(async move {
            let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await
            else {
                return false;
            };
            let imported = import.blocks.len();
            reply
                .send(ImportBlocksReply::ok(imported))
                .expect("reply import");
            true
        });

        let result = import_chain_acc_from_reader(
            &handle,
            &mut cursor,
            None,
            false,
            Some(ChainAccExpectedRange {
                start_height: 10,
                end_height: 10,
            }),
            None,
        )
        .await;
        service.abort();

        assert!(
            result.is_err(),
            "partial expected-range import without storage must fail, got {result:?}"
        );
        let err = result
            .expect_err("partial expected-range import needs storage for continuity validation");
        assert!(
            err.to_string().contains("requires local storage"),
            "unexpected error: {err}"
        );
    }
}
