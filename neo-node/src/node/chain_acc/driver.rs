//! Stream driver for `chain.acc` imports.
//!
//! This module owns the end-to-end reader loop: open or consume a chain.acc
//! stream, validate the expected range, batch blocks, dispatch them to the
//! blockchain service, and assemble the final import report.

use std::io::{BufRead, BufReader, Seek};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use neo_blockchain::BlockchainHandle;
use neo_payloads::block::Block;
use neo_storage::persistence::store::Store;
use tracing::info;

use crate::node::ledger_source::local_ledger_tip;

use super::batch::{
    ChainAccImportComposition, PendingChainAccBatch, import_chain_acc_batch, take_import_batch,
};
use super::format::{read_chain_acc_header, read_next_chain_acc_block};
use super::index::{ChainAccResumeValidation, position_chain_acc_reader};
use super::metrics::{
    ChainAccImportProgress, MdbxCommitCumulativeMetrics, NativePersistTxCumulativeMetrics,
    NativePersistTxStageImportMetrics, StateServiceMptCumulativeMetrics,
    StateServiceMptImportMetrics, should_log_import_progress,
};
use super::range::{
    bounded_chain_acc_import_range, chain_acc_import_record_count, chain_acc_records_to_skip,
    count_only_stop_height_exceeded, count_only_stop_height_reached, expected_chain_acc_count,
    expected_chain_acc_first_prev_hash, resume_chain_acc_import_range,
    validate_chain_acc_block_height, validate_chain_acc_count, validate_chain_acc_first_prev_hash,
    validate_chain_acc_internal_prev_hash,
};
use super::{
    ChainAccExpectedRange, ChainAccImportReport, ChainAccProfileWindow, IMPORT_BATCH_SIZE,
    ImportHotMetrics,
};

/// Import blocks from a `chain.acc` file and stop once `stop_at_height` is imported.
pub async fn import_chain_acc_until_height<S>(
    handle: &BlockchainHandle,
    path: &Path,
    verify: bool,
    stop_at_height: Option<u32>,
    storage: Option<Arc<S>>,
) -> anyhow::Result<u64>
where
    S: Store,
{
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

pub(in crate::node) async fn import_chain_acc_report_with_expected_range<S>(
    handle: &BlockchainHandle,
    path: &Path,
    verify: bool,
    expected_range: ChainAccExpectedRange,
    stop_at_height: Option<u32>,
    storage: Option<Arc<S>>,
) -> anyhow::Result<ChainAccImportReport>
where
    S: Store,
{
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

pub(super) async fn import_chain_acc_from_reader_until_height<R, S>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    storage: Option<Arc<S>>,
) -> anyhow::Result<u64>
where
    R: BufRead + Seek,
    S: Store,
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

pub(in crate::node::chain_acc) async fn import_chain_acc_report_from_reader_until_height<R, S>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    storage: Option<Arc<S>>,
) -> anyhow::Result<ChainAccImportReport>
where
    R: BufRead + Seek,
    S: Store,
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
    let local_tip_already_reached_stop = local_tip
        .as_ref()
        .is_some_and(|tip| stop_at_height.is_some_and(|stop| tip.height >= stop));
    let import_count = if local_tip_already_reached_stop {
        0
    } else {
        chain_acc_import_record_count(count, expected_range, import_range, stop_at_height)?
    };
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
    let mut profile_windows = Vec::with_capacity(import_count.div_ceil(IMPORT_BATCH_SIZE));
    let mut previous_state_service_profile = StateServiceMptCumulativeMetrics::current();
    let mut previous_mdbx_commit_profile = MdbxCommitCumulativeMetrics::current();
    let initial_native_tx_profile = NativePersistTxCumulativeMetrics::current();
    let mut previous_native_tx_profile = initial_native_tx_profile.clone();
    let expected_first_prev_hash =
        expected_chain_acc_first_prev_hash(import_range, local_tip.as_ref())?;
    chain_acc_validate_elapsed += validate_start.elapsed();

    let read_start = Instant::now();
    let position = position_chain_acc_reader(
        reader,
        path,
        header,
        records_to_skip,
        ChainAccResumeValidation {
            has_first_record: import_count > 0,
            expected_height: import_range.map(|range| range.start_height),
            expected_prev_hash: expected_first_prev_hash.as_ref(),
        },
    )?;
    tracing::debug!(
        target: "neo::import",
        record = records_to_skip,
        offset = position.offset,
        index_hit = position.index_hit,
        index_rebuilt = position.index_rebuilt,
        "chain.acc reader positioning complete"
    );
    let mut offset_index = position.offset_index;
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
        if let Some(index) = &mut offset_index {
            index.observe_record(record + 1, block_bytes.len());
        }
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
                if let Some(index) = &mut offset_index {
                    index.persist_best_effort();
                }
            }
            let state_service_metrics = StateServiceMptImportMetrics::current();
            let state_service_profile = StateServiceMptCumulativeMetrics::current();
            let state_service_window =
                state_service_profile.window_since(&previous_state_service_profile);
            previous_state_service_profile = state_service_profile;
            let mdbx_commit_profile = MdbxCommitCumulativeMetrics::current();
            let mdbx_commit_window =
                mdbx_commit_profile.window_since(&previous_mdbx_commit_profile);
            previous_mdbx_commit_profile = mdbx_commit_profile;
            let native_tx_profile = NativePersistTxCumulativeMetrics::current();
            let native_tx_stage_metrics =
                NativePersistTxStageImportMetrics::from_stats(native_tx_profile.stages());
            let native_tx_window = native_tx_profile.window_since(&previous_native_tx_profile);
            previous_native_tx_profile = native_tx_profile;
            hot_metrics = ImportHotMetrics::from_snapshot(&state_service_metrics);
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
                    state_service_mpt_backing_sort_avg_us = state_service_metrics.backing_sort_avg_us,
                    state_service_mpt_backing_commit_avg_us = state_service_metrics.backing_commit_avg_us,
                    state_service_mpt_publish_generation_avg_us = state_service_metrics.publish_generation_avg_us,
                    state_service_mpt_overlay_entries_avg = state_service_metrics.overlay_entries_avg,
                    state_service_mpt_batch_blocks_avg = state_service_metrics.batch_blocks_avg,
                    state_service_mpt_window_apply_attempts = state_service_window.apply_attempts,
                    state_service_mpt_window_apply_failures = state_service_window.apply_failures,
                    state_service_mpt_window_end_to_end_total_us = state_service_window.end_to_end_total_us,
                    state_service_mpt_window_avg_end_to_end_us = state_service_window.avg_end_to_end_us,
                    state_service_mpt_window_apply_total_us = state_service_window.apply_total_us,
                    state_service_mpt_window_avg_apply_us = state_service_window.avg_apply_us,
                    state_service_mpt_window_project_total_us = state_service_window.project_total_us,
                    state_service_mpt_window_avg_project_us = state_service_window.avg_project_us,
                    state_service_mpt_window_changes_total = state_service_window.changes_total,
                    state_service_mpt_window_avg_changes = state_service_window.avg_changes,
                    state_service_mpt_window_enqueue_blocking_total_us = state_service_window.stage_total_us("enqueue_blocking"),
                    state_service_mpt_window_enqueue_blocking_avg_us = state_service_window.stage_avg_us("enqueue_blocking"),
                    state_service_mpt_window_queue_wait_total_us = state_service_window.stage_total_us("queue_wait"),
                    state_service_mpt_window_queue_wait_avg_us = state_service_window.stage_avg_us("queue_wait"),
                    state_service_mpt_window_mutate_changes_total_us = state_service_window.stage_total_us("mutate_changes"),
                    state_service_mpt_window_mutate_changes_avg_us = state_service_window.stage_avg_us("mutate_changes"),
                    state_service_mpt_window_root_hash_total_us = state_service_window.stage_total_us("root_hash"),
                    state_service_mpt_window_root_hash_avg_us = state_service_window.stage_avg_us("root_hash"),
                    state_service_mpt_window_trie_commit_total_us = state_service_window.stage_total_us("trie_commit"),
                    state_service_mpt_window_trie_commit_avg_us = state_service_window.stage_avg_us("trie_commit"),
                    state_service_mpt_window_deferred_finalization_prepare_total_us = state_service_window.stage_total_us("deferred_finalization_prepare"),
                    state_service_mpt_window_deferred_finalization_prepare_avg_us = state_service_window.stage_avg_us("deferred_finalization_prepare"),
                    state_service_mpt_window_deferred_finalization_lookup_total_us = state_service_window.stage_total_us("deferred_finalization_lookup"),
                    state_service_mpt_window_deferred_finalization_lookup_avg_us = state_service_window.stage_avg_us("deferred_finalization_lookup"),
                    state_service_mpt_window_deferred_finalization_resolve_total_us = state_service_window.stage_total_us("deferred_finalization_resolve"),
                    state_service_mpt_window_deferred_finalization_resolve_avg_us = state_service_window.stage_avg_us("deferred_finalization_resolve"),
                    state_service_mpt_window_deferred_finalization_parse_total_us = state_service_window.stage_total_us("deferred_finalization_parse"),
                    state_service_mpt_window_deferred_finalization_parse_avg_us = state_service_window.stage_avg_us("deferred_finalization_parse"),
                    state_service_mpt_window_deferred_finalization_replay_total_us = state_service_window.stage_total_us("deferred_finalization_replay"),
                    state_service_mpt_window_deferred_finalization_replay_avg_us = state_service_window.stage_avg_us("deferred_finalization_replay"),
                    state_service_mpt_window_deferred_finalization_encode_total_us = state_service_window.stage_total_us("deferred_finalization_encode"),
                    state_service_mpt_window_deferred_finalization_encode_avg_us = state_service_window.stage_avg_us("deferred_finalization_encode"),
                    state_service_mpt_window_overlay_prepare_total_us = state_service_window.stage_total_us("overlay_prepare"),
                    state_service_mpt_window_overlay_prepare_avg_us = state_service_window.stage_avg_us("overlay_prepare"),
                    state_service_mpt_window_backing_sort_total_us = state_service_window.stage_total_us("backing_sort"),
                    state_service_mpt_window_backing_sort_avg_us = state_service_window.stage_avg_us("backing_sort"),
                    state_service_mpt_window_backing_commit_total_us = state_service_window.stage_total_us("backing_commit"),
                    state_service_mpt_window_backing_commit_avg_us = state_service_window.stage_avg_us("backing_commit"),
                    state_service_mpt_window_publish_generation_total_us = state_service_window.stage_total_us("publish_generation"),
                    state_service_mpt_window_publish_generation_avg_us = state_service_window.stage_avg_us("publish_generation"),
                    state_service_mpt_window_overlay_entries_total = state_service_window.count_total("overlay_entries"),
                    state_service_mpt_window_overlay_entries_avg = state_service_window.count_avg("overlay_entries"),
                    state_service_mpt_window_batch_blocks_total = state_service_window.count_total("batch_blocks"),
                    state_service_mpt_window_batch_blocks_avg = state_service_window.count_avg("batch_blocks"),
                    mdbx_commit_window_attempts = mdbx_commit_window.attempts,
                    mdbx_commit_window_failures = mdbx_commit_window.failures,
                    mdbx_commit_window_committed_transactions = mdbx_commit_window.committed_transactions,
                    mdbx_commit_window_total_us = mdbx_commit_window.stage_total_us("total"),
                    mdbx_commit_window_avg_total_us = mdbx_commit_window.stage_avg_us("total"),
                    mdbx_commit_window_transaction_open_total_us = mdbx_commit_window.stage_total_us("transaction_open"),
                    mdbx_commit_window_transaction_open_avg_us = mdbx_commit_window.stage_avg_us("transaction_open"),
                    mdbx_commit_window_table_open_total_us = mdbx_commit_window.stage_total_us("table_open"),
                    mdbx_commit_window_cursor_open_total_us = mdbx_commit_window.stage_total_us("cursor_open"),
                    mdbx_commit_window_overlay_sort_total_us = mdbx_commit_window.stage_total_us("overlay_sort"),
                    mdbx_commit_window_overlay_visit_total_us = mdbx_commit_window.stage_total_us("overlay_visit"),
                    mdbx_commit_window_cursor_write_total_us = mdbx_commit_window.stage_total_us("cursor_write"),
                    mdbx_commit_window_cursor_resolve_total_us = mdbx_commit_window.stage_total_us("cursor_resolve"),
                    mdbx_commit_window_cursor_resolve_avg_us = mdbx_commit_window.stage_avg_us("cursor_resolve"),
                    mdbx_commit_window_source_overhead_total_us = mdbx_commit_window
                        .stage_total_us("overlay_visit")
                        .saturating_sub(mdbx_commit_window.stage_total_us("cursor_write")),
                    mdbx_commit_window_commit_total_us = mdbx_commit_window.stage_total_us("commit"),
                    mdbx_commit_window_commit_avg_us = mdbx_commit_window.stage_avg_us("commit"),
                    mdbx_commit_window_entries_total = mdbx_commit_window.count_total("entries"),
                    mdbx_commit_window_puts_total = mdbx_commit_window.count_total("puts"),
                    mdbx_commit_window_deletes_total = mdbx_commit_window.count_total("deletes"),
                    mdbx_commit_window_key_bytes_total = mdbx_commit_window.count_total("key_bytes"),
                    mdbx_commit_window_value_bytes_total = mdbx_commit_window.count_total("value_bytes"),
                    "chain.acc import progress"
                );
                info!(
                    target: "neo::import",
                    imported = progress_sample.imported,
                    batch_imported = progress_sample.batch_imported,
                    mdbx_commit_window_cursor_resolve_present_total = mdbx_commit_window.count_total("cursor_resolve_present"),
                    mdbx_commit_window_cursor_resolve_absent_total = mdbx_commit_window.count_total("cursor_resolve_absent"),
                    mdbx_commit_window_cursor_resolve_read_bytes_total = mdbx_commit_window.count_total("cursor_resolve_read_bytes"),
                    mdbx_commit_window_cursor_resolve_write_bytes_total = mdbx_commit_window.count_total("cursor_resolve_write_bytes"),
                    mdbx_commit_window_cursor_resolve_minor_faults_total = mdbx_commit_window.count_total("cursor_resolve_minor_faults"),
                    mdbx_commit_window_cursor_resolve_major_faults_total = mdbx_commit_window.count_total("cursor_resolve_major_faults"),
                    "chain.acc MDBX cursor-resolve profile"
                );
                info!(
                    target: "neo::import",
                    imported = progress_sample.imported,
                    batch_imported = progress_sample.batch_imported,
                    mdbx_commit_window_value_size_0_64_total = mdbx_commit_window.count_total("value_size_0_64"),
                    mdbx_commit_window_value_size_65_128_total = mdbx_commit_window.count_total("value_size_65_128"),
                    mdbx_commit_window_value_size_129_256_total = mdbx_commit_window.count_total("value_size_129_256"),
                    mdbx_commit_window_value_size_257_512_total = mdbx_commit_window.count_total("value_size_257_512"),
                    mdbx_commit_window_value_size_513_1024_total = mdbx_commit_window.count_total("value_size_513_1024"),
                    mdbx_commit_window_value_size_1025_4096_total = mdbx_commit_window.count_total("value_size_1025_4096"),
                    mdbx_commit_window_value_size_4097_16384_total = mdbx_commit_window.count_total("value_size_4097_16384"),
                    mdbx_commit_window_value_size_over_16384_total = mdbx_commit_window.count_total("value_size_over_16384"),
                    "chain.acc MDBX value-size profile"
                );
                info!(
                    target: "neo::import",
                    imported = progress_sample.imported,
                    batch_imported = progress_sample.batch_imported,
                    native_persist_tx_window_hash_calls = native_tx_window.stage_calls("hash"),
                    native_persist_tx_window_hash_total_us = native_tx_window.stage_total_us("hash"),
                    native_persist_tx_window_cache_prepare_calls = native_tx_window.stage_calls("cache_prepare"),
                    native_persist_tx_window_cache_prepare_total_us = native_tx_window.stage_total_us("cache_prepare"),
                    native_persist_tx_window_container_prepare_calls = native_tx_window.stage_calls("container_prepare"),
                    native_persist_tx_window_container_prepare_total_us = native_tx_window.stage_total_us("container_prepare"),
                    native_persist_tx_window_engine_create_calls = native_tx_window.stage_calls("engine_create"),
                    native_persist_tx_window_engine_create_total_us = native_tx_window.stage_total_us("engine_create"),
                    native_persist_tx_window_load_execute_calls = native_tx_window.stage_calls("load_execute"),
                    native_persist_tx_window_load_execute_total_us = native_tx_window.stage_total_us("load_execute"),
                    native_persist_tx_window_load_script_calls = native_tx_window.stage_calls("load_script"),
                    native_persist_tx_window_load_script_total_us = native_tx_window.stage_total_us("load_script"),
                    native_persist_tx_window_execute_calls = native_tx_window.stage_calls("execute"),
                    native_persist_tx_window_execute_total_us = native_tx_window.stage_total_us("execute"),
                    native_persist_tx_window_application_executed_calls = native_tx_window.stage_calls("application_executed"),
                    native_persist_tx_window_application_executed_total_us = native_tx_window.stage_total_us("application_executed"),
                    native_persist_tx_window_tx_cache_commit_calls = native_tx_window.stage_calls("tx_cache_commit"),
                    native_persist_tx_window_tx_cache_commit_total_us = native_tx_window.stage_total_us("tx_cache_commit"),
                    native_persist_tx_window_ledger_vm_state_calls = native_tx_window.stage_calls("ledger_vm_state"),
                    native_persist_tx_window_ledger_vm_state_total_us = native_tx_window.stage_total_us("ledger_vm_state"),
                    native_persist_tx_hash_avg_us = native_tx_stage_metrics.hash_avg_us,
                    native_persist_tx_cache_prepare_avg_us = native_tx_stage_metrics.cache_prepare_avg_us,
                    native_persist_tx_container_prepare_avg_us = native_tx_stage_metrics.container_prepare_avg_us,
                    native_persist_tx_engine_create_avg_us = native_tx_stage_metrics.engine_create_avg_us,
                    native_persist_tx_load_execute_avg_us = native_tx_stage_metrics.load_execute_avg_us,
                    native_persist_tx_load_script_avg_us = native_tx_stage_metrics.load_script_avg_us,
                    native_persist_tx_execute_avg_us = native_tx_stage_metrics.execute_avg_us,
                    native_persist_tx_application_executed_avg_us = native_tx_stage_metrics.application_executed_avg_us,
                    native_persist_tx_cache_commit_avg_us = native_tx_stage_metrics.tx_cache_commit_avg_us,
                    native_persist_tx_ledger_vm_state_avg_us = native_tx_stage_metrics.ledger_vm_state_avg_us,
                    "chain.acc native transaction window profile"
                );
                info!(
                    target: "neo::import",
                    imported = progress_sample.imported,
                    state_service_mpt_window_overlay_puts_total = state_service_window.count_total("overlay_puts"),
                    state_service_mpt_window_overlay_deletes_total = state_service_window.count_total("overlay_deletes"),
                    state_service_mpt_window_node_puts_total = state_service_window.count_total("node_puts"),
                    state_service_mpt_window_node_deletes_total = state_service_window.count_total("node_deletes"),
                    state_service_mpt_window_node_value_size_0_64_total = state_service_window.count_total("node_value_size_0_64"),
                    state_service_mpt_window_node_value_size_65_128_total = state_service_window.count_total("node_value_size_65_128"),
                    state_service_mpt_window_node_value_size_129_256_total = state_service_window.count_total("node_value_size_129_256"),
                    state_service_mpt_window_node_value_size_257_512_total = state_service_window.count_total("node_value_size_257_512"),
                    state_service_mpt_window_node_value_size_513_1024_total = state_service_window.count_total("node_value_size_513_1024"),
                    state_service_mpt_window_node_value_size_1025_4096_total = state_service_window.count_total("node_value_size_1025_4096"),
                    state_service_mpt_window_node_value_size_4097_16384_total = state_service_window.count_total("node_value_size_4097_16384"),
                    state_service_mpt_window_node_value_size_over_16384_total = state_service_window.count_total("node_value_size_over_16384"),
                    state_service_mpt_window_node_value_bytes_0_64_total = state_service_window.count_total("node_value_bytes_0_64"),
                    state_service_mpt_window_node_value_bytes_65_128_total = state_service_window.count_total("node_value_bytes_65_128"),
                    state_service_mpt_window_node_value_bytes_129_256_total = state_service_window.count_total("node_value_bytes_129_256"),
                    state_service_mpt_window_node_value_bytes_257_512_total = state_service_window.count_total("node_value_bytes_257_512"),
                    state_service_mpt_window_node_value_bytes_513_1024_total = state_service_window.count_total("node_value_bytes_513_1024"),
                    state_service_mpt_window_node_value_bytes_1025_4096_total = state_service_window.count_total("node_value_bytes_1025_4096"),
                    state_service_mpt_window_node_value_bytes_4097_16384_total = state_service_window.count_total("node_value_bytes_4097_16384"),
                    state_service_mpt_window_node_value_bytes_over_16384_total = state_service_window.count_total("node_value_bytes_over_16384"),
                    state_service_mpt_window_put_node_cached_calls_total = state_service_window.count_total("put_node_cached_calls"),
                    state_service_mpt_window_serialized_payload_bytes_total = state_service_window.count_total("serialized_payload_bytes"),
                    state_service_mpt_window_hash_computations_total = state_service_window.count_total("hash_computations"),
                    state_service_mpt_window_max_recursion_depth_total = state_service_window.count_total("max_recursion_depth"),
                    state_service_mpt_window_repeated_ancestor_finalizations_total = state_service_window.count_total("repeated_ancestor_finalizations"),
                    state_service_mpt_window_trie_resolve_store_total_us = state_service_window.stage_total_us("trie_resolve_store"),
                    state_service_mpt_window_trie_resolve_cache_hits_total = state_service_window.count_total("trie_resolve_cache_hits"),
                    state_service_mpt_window_trie_resolve_store_hits_total = state_service_window.count_total("trie_resolve_store_hits"),
                    state_service_mpt_window_trie_resolve_store_misses_total = state_service_window.count_total("trie_resolve_store_misses"),
                    state_service_mpt_window_deferred_finalization_read_bytes_total = state_service_window.count_total("deferred_finalization_read_bytes"),
                    state_service_mpt_window_deferred_finalization_minor_faults_total = state_service_window.count_total("deferred_finalization_minor_faults"),
                    state_service_mpt_window_deferred_finalization_major_faults_total = state_service_window.count_total("deferred_finalization_major_faults"),
                    state_service_mpt_window_overlay_working_set_entries_total = state_service_window.count_total("overlay_working_set_entries"),
                    state_service_mpt_window_finalization_cache_hits_total = state_service_window.count_total("finalization_cache_hits"),
                    state_service_mpt_window_finalization_memory_hits_total = state_service_window.count_total("finalization_memory_hits"),
                    state_service_mpt_window_finalization_memory_misses_total = state_service_window.count_total("finalization_memory_misses"),
                    state_service_mpt_window_finalization_backing_hits_total = state_service_window.count_total("finalization_backing_hits"),
                    state_service_mpt_window_finalization_backing_misses_total = state_service_window.count_total("finalization_backing_misses"),
                    state_service_mpt_window_finalization_lookup_errors_total = state_service_window.count_total("finalization_lookup_errors"),
                    state_service_mpt_window_deferred_journal_entries_total = state_service_window.count_total("deferred_journal_entries"),
                    "chain.acc MPT mutation profile"
                );
            }
            if let Some(tip) = batch_result.tip {
                profile_windows.push(ChainAccProfileWindow::new(
                    tip.height,
                    batch_result.imported,
                    batch_result.composition.transactions,
                    batch_result.elapsed,
                    batch_result.stats,
                    hot_metrics,
                    native_tx_window,
                    state_service_window,
                    mdbx_commit_window,
                ));
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
    let transaction_finalized_delivery_seconds =
        composition.transaction_finalized_delivery_seconds();
    let transaction_blocks_per_second = composition.transaction_blocks_per_second();
    let finalization_seconds = composition.finalization_seconds();
    let finalization_commit_handlers_seconds = composition.finalization_commit_handlers_seconds();
    let finalization_store_commit_seconds = composition.finalization_store_commit_seconds();
    let unclassified_import_seconds = composition.unclassified_import_seconds(progress.elapsed());
    let native_persist_tx = previous_native_tx_profile.window_since(&initial_native_tx_profile);
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
        transaction_finalized_delivery_seconds,
        transaction_blocks_per_second,
        finalization_seconds,
        finalization_commit_handlers_seconds,
        finalization_store_commit_seconds,
        unclassified_import_seconds,
        native_persist_tx_import_hash_calls = native_persist_tx.stage_calls("hash"),
        native_persist_tx_import_hash_total_us = native_persist_tx.stage_total_us("hash"),
        native_persist_tx_import_cache_prepare_calls = native_persist_tx.stage_calls("cache_prepare"),
        native_persist_tx_import_cache_prepare_total_us = native_persist_tx.stage_total_us("cache_prepare"),
        native_persist_tx_import_container_prepare_calls = native_persist_tx.stage_calls("container_prepare"),
        native_persist_tx_import_container_prepare_total_us = native_persist_tx.stage_total_us("container_prepare"),
        native_persist_tx_import_engine_create_calls = native_persist_tx.stage_calls("engine_create"),
        native_persist_tx_import_engine_create_total_us = native_persist_tx.stage_total_us("engine_create"),
        native_persist_tx_import_load_execute_calls = native_persist_tx.stage_calls("load_execute"),
        native_persist_tx_import_load_execute_total_us = native_persist_tx.stage_total_us("load_execute"),
        native_persist_tx_import_load_script_calls = native_persist_tx.stage_calls("load_script"),
        native_persist_tx_import_load_script_total_us = native_persist_tx.stage_total_us("load_script"),
        native_persist_tx_import_execute_calls = native_persist_tx.stage_calls("execute"),
        native_persist_tx_import_execute_total_us = native_persist_tx.stage_total_us("execute"),
        native_persist_tx_import_application_executed_calls = native_persist_tx.stage_calls("application_executed"),
        native_persist_tx_import_application_executed_total_us = native_persist_tx.stage_total_us("application_executed"),
        native_persist_tx_import_tx_cache_commit_calls = native_persist_tx.stage_calls("tx_cache_commit"),
        native_persist_tx_import_tx_cache_commit_total_us = native_persist_tx.stage_total_us("tx_cache_commit"),
        native_persist_tx_import_ledger_vm_state_calls = native_persist_tx.stage_calls("ledger_vm_state"),
        native_persist_tx_import_ledger_vm_state_total_us = native_persist_tx.stage_total_us("ledger_vm_state"),
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
        transaction_finalized_delivery_seconds,
        transaction_blocks_per_second,
        finalization_seconds,
        finalization_commit_handlers_seconds,
        finalization_store_commit_seconds,
        unclassified_import_seconds,
        native_persist_tx,
        hot_metrics,
        profile_windows,
    })
}
