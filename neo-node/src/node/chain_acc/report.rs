//! Import report DTOs for `chain.acc` ingestion.
//!
//! The stream driver owns reading and dispatch. This module owns the
//! machine-readable report shape and the projection from hot-path metrics into
//! stable report fields consumed by fast-sync reporting.

use super::metrics::StateServiceMptImportMetrics;

use crate::node::ledger_source::LocalLedgerTip;

#[derive(Debug, Clone, Copy, PartialEq)]
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
