//! Machine-readable fast-sync reports and throughput classification.
//!
//! The fast-sync orchestrator imports data, verifies local/reference tips, and
//! hands the raw chain accumulator report here. This module projects that data
//! into the JSON sidecar shape and owns the transaction-bearing BPS floor used
//! as a performance proof.

use std::path::Path;

use anyhow::Context;
use neo_primitives::UInt256;
use serde::Serialize;
use tracing::{info, warn};

use super::package::FastSyncPackage;

const FAST_SYNC_FLOOR_BPS: f64 = 1500.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(in crate::node) struct FastSyncReport {
    pub(in crate::node) package: FastSyncPackageReport,
    pub(in crate::node) import: FastSyncImportReport,
    pub(in crate::node) hot_metrics: FastSyncHotMetricsReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::node) reference: Option<FastSyncReferenceReport>,
}

impl FastSyncReport {
    pub(super) fn from_parts(
        package: &FastSyncPackage,
        zip_path: &Path,
        chain_path: &Path,
        import: super::super::chain_acc::ChainAccImportReport,
        reference: Option<FastSyncReferenceReport>,
    ) -> Self {
        let hot_metrics = import.hot_metrics;
        Self {
            package: FastSyncPackageReport {
                network: package.network_key.to_string(),
                url: package.url.clone(),
                md5: package.md5.clone(),
                start_height: package.start,
                end_height: package.end,
                filename: package.filename.clone(),
                zip_path: zip_path.display().to_string(),
                chain_path: chain_path.display().to_string(),
            },
            import: FastSyncImportReport {
                imported_blocks: import.imported,
                final_height: import.last_imported_tip.map(|tip| tip.height),
                final_hash: import.last_imported_tip.map(|tip| tip.hash.to_string()),
                elapsed_seconds: import.elapsed_seconds,
                driver_elapsed_seconds: import.driver_elapsed_seconds,
                chain_acc_read_seconds: import.chain_acc_read_seconds,
                chain_acc_validate_seconds: import.chain_acc_validate_seconds,
                average_blocks_per_second: import.average_blocks_per_second,
                empty_blocks: import.empty_blocks,
                empty_only_blocks: import.empty_only_blocks,
                empty_block_import_seconds: import.empty_block_import_seconds,
                empty_blocks_per_second: import.empty_blocks_per_second,
                transaction_blocks: import.transaction_blocks,
                transactions: import.transactions,
                transaction_block_import_seconds: import.transaction_block_import_seconds,
                transaction_block_clone_seconds: import.transaction_block_clone_seconds,
                transaction_ledger_insert_seconds: import.transaction_ledger_insert_seconds,
                transaction_finalized_delivery_seconds: import
                    .transaction_finalized_delivery_seconds,
                transaction_blocks_per_second: import.transaction_blocks_per_second,
                finalization_seconds: import.finalization_seconds,
                finalization_commit_handlers_seconds: import.finalization_commit_handlers_seconds,
                finalization_store_commit_seconds: import.finalization_store_commit_seconds,
                unclassified_import_seconds: import.unclassified_import_seconds,
                throughput_status: fast_sync_throughput_status(&import),
            },
            hot_metrics: FastSyncHotMetricsReport {
                state_service_mpt_apply_attempts: hot_metrics.state_service_mpt_apply_attempts,
                state_service_mpt_apply_failures: hot_metrics.state_service_mpt_apply_failures,
                state_service_mpt_apply_height: hot_metrics.state_service_mpt_apply_height,
                state_service_mpt_avg_total_us: hot_metrics.state_service_mpt_avg_total_us,
                state_service_mpt_avg_project_us: hot_metrics.state_service_mpt_avg_project_us,
                state_service_mpt_avg_trie_us: hot_metrics.state_service_mpt_avg_trie_us,
                state_service_mpt_avg_changes: hot_metrics.state_service_mpt_avg_changes,
                state_service_mpt_enqueue_blocking_avg_us: hot_metrics
                    .state_service_mpt_enqueue_blocking_avg_us,
                state_service_mpt_queue_wait_avg_us: hot_metrics
                    .state_service_mpt_queue_wait_avg_us,
                state_service_mpt_mutate_changes_avg_us: hot_metrics
                    .state_service_mpt_mutate_changes_avg_us,
                state_service_mpt_root_hash_avg_us: hot_metrics.state_service_mpt_root_hash_avg_us,
                state_service_mpt_trie_commit_avg_us: hot_metrics
                    .state_service_mpt_trie_commit_avg_us,
                state_service_mpt_backing_commit_avg_us: hot_metrics
                    .state_service_mpt_backing_commit_avg_us,
                state_service_mpt_publish_generation_avg_us: hot_metrics
                    .state_service_mpt_publish_generation_avg_us,
                state_service_mpt_overlay_entries_avg: hot_metrics
                    .state_service_mpt_overlay_entries_avg,
                state_service_mpt_batch_blocks_avg: hot_metrics.state_service_mpt_batch_blocks_avg,
                native_persist_avg_total_us: hot_metrics.native_persist_avg_total_us,
                native_persist_tx_hot_stage: hot_metrics.native_persist_tx_hot_stage.to_string(),
                native_persist_tx_hot_stage_avg_us: hot_metrics.native_persist_tx_hot_stage_avg_us,
                native_persist_tx_stages: neo_runtime::sync_metrics::native_persist_tx_stage_stats(
                )
                .into_iter()
                .map(FastSyncStageMetricReport::from_native_tx_stage)
                .collect(),
                rocksdb_batch_avg_flush_duration_ms: hot_metrics
                    .rocksdb_batch_avg_flush_duration_ms,
                rocksdb_batch_pending_operations: hot_metrics.rocksdb_batch_pending_operations,
            },
            reference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(in crate::node) struct FastSyncPackageReport {
    pub(in crate::node) network: String,
    pub(in crate::node) url: String,
    pub(in crate::node) md5: String,
    pub(in crate::node) start_height: u32,
    pub(in crate::node) end_height: u32,
    pub(in crate::node) filename: String,
    pub(in crate::node) zip_path: String,
    pub(in crate::node) chain_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(in crate::node) struct FastSyncImportReport {
    pub(in crate::node) imported_blocks: u64,
    pub(in crate::node) final_height: Option<u32>,
    pub(in crate::node) final_hash: Option<String>,
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
    pub(in crate::node) throughput_status: FastSyncThroughputStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(in crate::node) struct FastSyncHotMetricsReport {
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
    pub(in crate::node) native_persist_tx_hot_stage: String,
    pub(in crate::node) native_persist_tx_hot_stage_avg_us: u64,
    pub(in crate::node) native_persist_tx_stages: Vec<FastSyncStageMetricReport>,
    pub(in crate::node) rocksdb_batch_avg_flush_duration_ms: u64,
    pub(in crate::node) rocksdb_batch_pending_operations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(in crate::node) struct FastSyncStageMetricReport {
    pub(in crate::node) stage: String,
    pub(in crate::node) calls: u64,
    pub(in crate::node) avg_us: u64,
}

impl FastSyncStageMetricReport {
    fn from_native_tx_stage(stat: neo_runtime::sync_metrics::NativePersistTxStageStats) -> Self {
        Self {
            stage: stat.stage.to_string(),
            calls: stat.calls,
            avg_us: stat.avg_us,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::node) struct FastSyncBlockReferenceProof {
    pub(in crate::node) height: u32,
    pub(in crate::node) hash: UInt256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::node) struct FastSyncStateRootReferenceProof {
    pub(in crate::node) height: u32,
    pub(in crate::node) root_hash: UInt256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(in crate::node) struct FastSyncReferenceReport {
    pub(in crate::node) endpoint: String,
    pub(in crate::node) block_height: u32,
    pub(in crate::node) block_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::node) state_root_height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::node) state_root_hash: Option<String>,
}

impl FastSyncReferenceReport {
    pub(in crate::node) fn from_proofs(
        endpoint: &str,
        block: FastSyncBlockReferenceProof,
        state_root: Option<FastSyncStateRootReferenceProof>,
    ) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            block_height: block.height,
            block_hash: block.hash.to_string(),
            state_root_height: state_root.map(|proof| proof.height),
            state_root_hash: state_root.map(|proof| proof.root_hash.to_string()),
        }
    }
}

pub(in crate::node) fn write_fast_sync_report_sidecar(
    path: &Path,
    report: &FastSyncReport,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating fast-sync report directory {}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(report).context("serializing fast-sync report")?;
    std::fs::write(path, payload)
        .with_context(|| format!("writing fast-sync report {}", path.display()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::node) enum FastSyncThroughputStatus {
    NoImport,
    NoTransactionProof,
    BelowTarget,
    MeetsFloor,
}

pub(in crate::node) fn fast_sync_throughput_status(
    report: &super::super::chain_acc::ChainAccImportReport,
) -> FastSyncThroughputStatus {
    if report.imported == 0 {
        return FastSyncThroughputStatus::NoImport;
    }
    if report.transaction_blocks == 0 {
        return FastSyncThroughputStatus::NoTransactionProof;
    }
    if report.transaction_blocks_per_second < FAST_SYNC_FLOOR_BPS {
        FastSyncThroughputStatus::BelowTarget
    } else {
        FastSyncThroughputStatus::MeetsFloor
    }
}

pub(in crate::node) fn log_fast_sync_throughput(
    package: &FastSyncPackage,
    report: &super::super::chain_acc::ChainAccImportReport,
) {
    let status = fast_sync_throughput_status(report);
    match status {
        FastSyncThroughputStatus::BelowTarget => warn!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            transaction_blocks = report.transaction_blocks,
            transaction_blocks_per_second = report.transaction_blocks_per_second,
            floor_bps = FAST_SYNC_FLOOR_BPS,
            "fast-sync package import finished below transaction-bearing throughput floor"
        ),
        FastSyncThroughputStatus::NoImport => info!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            floor_bps = FAST_SYNC_FLOOR_BPS,
            "fast-sync package import skipped because local ledger already covers requested range"
        ),
        FastSyncThroughputStatus::NoTransactionProof => warn!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            empty_blocks = report.empty_blocks,
            empty_blocks_per_second = report.empty_blocks_per_second,
            floor_bps = FAST_SYNC_FLOOR_BPS,
            "fast-sync package import has no transaction-bearing speed proof"
        ),
        FastSyncThroughputStatus::MeetsFloor => info!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            transaction_blocks = report.transaction_blocks,
            transaction_blocks_per_second = report.transaction_blocks_per_second,
            floor_bps = FAST_SYNC_FLOOR_BPS,
            status = ?status,
            "fast-sync package import throughput summary"
        ),
    }
}
