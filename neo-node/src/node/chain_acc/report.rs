//! Import report DTOs for `chain.acc` ingestion.
//!
//! The stream driver owns reading and dispatch. This module owns the
//! machine-readable report shape and the projection from hot-path metrics into
//! stable report fields consumed by fast-sync reporting.

use std::time::Duration;

use serde::Serialize;

use super::metrics::StateServiceMptImportMetrics;
use super::{MdbxCommitWindowMetrics, StateServiceMptWindowMetrics};

use crate::node::ledger_source::LocalLedgerTip;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::node) struct ChainAccImportReport {
    pub(in crate::node) imported: u64,
    pub(in crate::node) last_imported_tip: Option<LocalLedgerTip>,
    pub(in crate::node) elapsed_seconds: f64,
    pub(in crate::node) driver_elapsed_seconds: f64,
    pub(in crate::node) chain_acc_read_seconds: f64,
    pub(in crate::node) chain_acc_validate_seconds: f64,
    pub(in crate::node) average_blocks_per_second: f64,
    pub(in crate::node) empty_blocks: u64,
    pub(in crate::node) empty_only_blocks: u64,
    pub(in crate::node) empty_block_import_seconds: f64,
    pub(in crate::node) empty_blocks_per_second: f64,
    pub(in crate::node) transaction_blocks: u64,
    pub(in crate::node) transactions: u64,
    pub(in crate::node) transaction_block_import_seconds: f64,
    pub(in crate::node) transaction_block_clone_seconds: f64,
    pub(in crate::node) transaction_ledger_insert_seconds: f64,
    pub(in crate::node) transaction_finalized_delivery_seconds: f64,
    pub(in crate::node) transaction_blocks_per_second: f64,
    pub(in crate::node) finalization_seconds: f64,
    pub(in crate::node) finalization_commit_handlers_seconds: f64,
    pub(in crate::node) finalization_store_commit_seconds: f64,
    pub(in crate::node) unclassified_import_seconds: f64,
    pub(in crate::node) hot_metrics: ImportHotMetrics,
    pub(in crate::node) profile_windows: Vec<ChainAccProfileWindow>,
}

/// One durable import-batch profiling window retained in the fast-sync proof.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(in crate::node) struct ChainAccProfileWindow {
    pub(in crate::node) start_height: u32,
    pub(in crate::node) end_height: u32,
    pub(in crate::node) blocks: u64,
    pub(in crate::node) elapsed_seconds: f64,
    pub(in crate::node) blocks_per_second: f64,
    pub(in crate::node) empty_blocks: u64,
    pub(in crate::node) empty_block_import_seconds: f64,
    pub(in crate::node) empty_blocks_per_second: f64,
    pub(in crate::node) transaction_blocks: u64,
    pub(in crate::node) transactions: u64,
    pub(in crate::node) transaction_block_import_seconds: f64,
    pub(in crate::node) transaction_blocks_per_second: f64,
    pub(in crate::node) finalization_seconds: f64,
    pub(in crate::node) finalization_commit_handlers_seconds: f64,
    pub(in crate::node) finalization_canonical_commit_seconds: f64,
    pub(in crate::node) hot_metrics: ImportHotMetrics,
    pub(in crate::node) state_service_mpt: StateServiceMptWindowMetrics,
    pub(in crate::node) mdbx_commit: MdbxCommitWindowMetrics,
}

impl ChainAccProfileWindow {
    pub(super) fn new(
        end_height: u32,
        blocks: usize,
        transactions: u64,
        elapsed: Duration,
        stats: neo_blockchain::ImportBlocksStats,
        hot_metrics: ImportHotMetrics,
        state_service_mpt: StateServiceMptWindowMetrics,
        mdbx_commit: MdbxCommitWindowMetrics,
    ) -> Self {
        let blocks = blocks as u64;
        let start_height = end_height.saturating_sub(blocks.saturating_sub(1) as u32);
        let elapsed_seconds = elapsed.as_secs_f64();
        let empty_blocks = stats.empty_blocks as u64;
        let empty_block_import_seconds = stats.empty_elapsed.as_secs_f64();
        let transaction_blocks = stats.transaction_blocks as u64;
        let transaction_block_import_seconds = stats.transaction_elapsed.as_secs_f64();
        Self {
            start_height,
            end_height,
            blocks,
            elapsed_seconds,
            blocks_per_second: rate(blocks, elapsed_seconds),
            empty_blocks,
            empty_block_import_seconds,
            empty_blocks_per_second: rate(empty_blocks, empty_block_import_seconds),
            transaction_blocks,
            transactions,
            transaction_block_import_seconds,
            transaction_blocks_per_second: rate(
                transaction_blocks,
                transaction_block_import_seconds,
            ),
            finalization_seconds: stats.finalization_elapsed.as_secs_f64(),
            finalization_commit_handlers_seconds: stats
                .finalization_commit_handlers_elapsed
                .as_secs_f64(),
            finalization_canonical_commit_seconds: stats
                .finalization_store_commit_elapsed
                .as_secs_f64(),
            hot_metrics,
            state_service_mpt,
            mdbx_commit,
        }
    }
}

fn rate(count: u64, elapsed_seconds: f64) -> f64 {
    if elapsed_seconds > 0.0 {
        count as f64 / elapsed_seconds
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct ImportHotMetrics {
    pub(in crate::node) state_service_mpt_apply_attempts: u64,
    pub(in crate::node) state_service_mpt_apply_failures: u64,
    pub(in crate::node) state_service_mpt_apply_height: u64,
    pub(in crate::node) state_service_mpt_avg_total_us: u64,
    pub(in crate::node) state_service_mpt_avg_project_us: u64,
    pub(in crate::node) state_service_mpt_avg_trie_us: u64,
    pub(in crate::node) state_service_mpt_avg_changes: u64,
    pub(in crate::node) state_service_mpt_enqueue_blocking_avg_us: u64,
    pub(in crate::node) state_service_mpt_queue_wait_avg_us: u64,
    pub(in crate::node) state_service_mpt_mutate_changes_avg_us: u64,
    pub(in crate::node) state_service_mpt_root_hash_avg_us: u64,
    pub(in crate::node) state_service_mpt_trie_commit_avg_us: u64,
    pub(in crate::node) state_service_mpt_backing_sort_avg_us: u64,
    pub(in crate::node) state_service_mpt_backing_commit_avg_us: u64,
    pub(in crate::node) state_service_mpt_publish_generation_avg_us: u64,
    pub(in crate::node) state_service_mpt_overlay_entries_avg: u64,
    pub(in crate::node) state_service_mpt_batch_blocks_avg: u64,
    pub(in crate::node) native_persist_avg_total_us: u64,
    pub(in crate::node) native_persist_tx_hot_stage: &'static str,
    pub(in crate::node) native_persist_tx_hot_stage_avg_us: u64,
}

impl ImportHotMetrics {
    pub(super) fn from_snapshot(state_service: &StateServiceMptImportMetrics) -> Self {
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
            state_service_mpt_backing_sort_avg_us: state_service.backing_sort_avg_us,
            state_service_mpt_backing_commit_avg_us: state_service.backing_commit_avg_us,
            state_service_mpt_publish_generation_avg_us: state_service.publish_generation_avg_us,
            state_service_mpt_overlay_entries_avg: state_service.overlay_entries_avg,
            state_service_mpt_batch_blocks_avg: state_service.batch_blocks_avg,
            native_persist_avg_total_us: state_service.native_persist_avg_total_us,
            native_persist_tx_hot_stage: state_service.native_persist_tx_hot_stage,
            native_persist_tx_hot_stage_avg_us: state_service.native_persist_tx_hot_stage_avg_us,
        }
    }
}
