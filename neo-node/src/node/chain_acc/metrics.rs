//! Progress and hot-path metrics projection for `chain.acc` imports.

use std::time::Duration;

use neo_storage::persistence::store::Store;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct StateServiceMptImportMetrics {
    pub(super) sync_blocks_persisted: u64,
    pub(super) sync_avg_total_us: u64,
    pub(super) sync_avg_verify_us: u64,
    pub(super) sync_avg_persist_us: u64,
    pub(super) sync_avg_commit_us: u64,
    pub(super) native_persist_avg_total_us: u64,
    pub(super) native_persist_avg_onpersist_us: u64,
    pub(super) native_persist_avg_tx_us: u64,
    pub(super) native_persist_avg_postpersist_us: u64,
    pub(super) native_persist_avg_cache_commit_us: u64,
    pub(super) native_persist_avg_tx_count: u64,
    pub(super) native_persist_tx_hot_stage: &'static str,
    pub(super) native_persist_tx_hot_stage_avg_us: u64,
    pub(super) native_contract_hook_hot_trigger: &'static str,
    pub(super) native_contract_hook_hot_contract: &'static str,
    pub(super) native_contract_hook_hot_contract_id: i32,
    pub(super) native_contract_hook_hot_avg_us: u64,
    pub(super) neotoken_onpersist_hot_stage: &'static str,
    pub(super) neotoken_onpersist_hot_stage_avg_us: u64,
    pub(super) neotoken_committee_compute_hot_stage: &'static str,
    pub(super) neotoken_committee_compute_hot_stage_avg_us: u64,
    pub(super) neotoken_committee_candidate_hot_kind: &'static str,
    pub(super) neotoken_committee_candidate_hot_avg: u64,
    pub(super) apply_attempts: u64,
    pub(super) apply_failures: u64,
    pub(super) apply_height: u64,
    pub(super) avg_total_us: u64,
    pub(super) avg_project_us: u64,
    pub(super) avg_trie_us: u64,
    pub(super) avg_changes: u64,
    pub(super) enqueue_blocking_avg_us: u64,
    pub(super) queue_wait_avg_us: u64,
    pub(super) mutate_changes_avg_us: u64,
    pub(super) root_hash_avg_us: u64,
    pub(super) trie_commit_avg_us: u64,
    pub(super) backing_commit_avg_us: u64,
    pub(super) publish_generation_avg_us: u64,
    pub(super) overlay_entries_avg: u64,
    pub(super) batch_blocks_avg: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct NativePersistTxStageImportMetrics {
    pub(super) load_execute_avg_us: u64,
    pub(super) load_script_avg_us: u64,
    pub(super) execute_avg_us: u64,
}

impl NativePersistTxStageImportMetrics {
    pub(super) fn current() -> Self {
        let stages = neo_runtime::sync_metrics::native_persist_tx_stage_stats();
        Self::from_stats(&stages)
    }

    pub(super) fn from_stats(
        stages: &[neo_runtime::sync_metrics::NativePersistTxStageStats],
    ) -> Self {
        let avg = |name: &str| -> u64 {
            stages
                .iter()
                .find(|stat| stat.stage == name)
                .map_or(0, |stat| stat.avg_us)
        };
        Self {
            load_execute_avg_us: avg("load_execute"),
            load_script_avg_us: avg("load_script"),
            execute_avg_us: avg("execute"),
        }
    }
}

impl StateServiceMptImportMetrics {
    pub(super) fn current() -> Self {
        Self::from_direct_hot_snapshot(
            SyncHotPathMetrics::current(),
            neo_state_service::StateRootApplyMetrics::state_root_apply_stats(),
            neo_state_service::StateRootApplyMetrics::state_root_apply_hot_stats(),
            neo_runtime::sync_metrics::native_contract_hook_hot_stats(),
            neo_runtime::sync_metrics::native_persist_tx_hot_stats(),
            neo_runtime::sync_metrics::neo_token_onpersist_hot_stats(),
            neo_runtime::sync_metrics::neo_token_committee_compute_hot_stats(),
            neo_runtime::sync_metrics::neo_token_committee_candidate_hot_stats(),
        )
    }

    pub(super) fn from_direct_hot_snapshot(
        sync: SyncHotPathMetrics,
        apply: neo_state_service::StateRootApplyStats,
        apply_hot: neo_state_service::metrics::StateRootApplyHotStats,
        native_hook_hot: Option<neo_runtime::sync_metrics::NativeContractHookStats>,
        native_tx_hot: Option<neo_runtime::sync_metrics::NativePersistTxStageStats>,
        neotoken_onpersist_hot: Option<neo_runtime::sync_metrics::NeoTokenOnPersistStageStats>,
        neotoken_committee_hot: Option<
            neo_runtime::sync_metrics::NeoTokenCommitteeComputeStageStats,
        >,
        neotoken_candidate_hot: Option<
            neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCountStats,
        >,
    ) -> Self {
        Self {
            sync_blocks_persisted: sync.blocks_persisted,
            sync_avg_total_us: sync.avg_total_us,
            sync_avg_verify_us: sync.avg_verify_us,
            sync_avg_persist_us: sync.avg_persist_us,
            sync_avg_commit_us: sync.avg_commit_us,
            native_persist_avg_total_us: sync.native_persist_avg_total_us,
            native_persist_avg_onpersist_us: sync.native_persist_avg_onpersist_us,
            native_persist_avg_tx_us: sync.native_persist_avg_tx_us,
            native_persist_avg_postpersist_us: sync.native_persist_avg_postpersist_us,
            native_persist_avg_cache_commit_us: sync.native_persist_avg_cache_commit_us,
            native_persist_avg_tx_count: sync.native_persist_avg_tx_count,
            native_persist_tx_hot_stage: native_tx_hot.map_or("", |stat| stat.stage),
            native_persist_tx_hot_stage_avg_us: native_tx_hot.map_or(0, |stat| stat.avg_us),
            native_contract_hook_hot_trigger: native_hook_hot.map_or("", |stat| stat.trigger),
            native_contract_hook_hot_contract: native_hook_hot.map_or("", |stat| stat.contract),
            native_contract_hook_hot_contract_id: native_hook_hot
                .map_or(0, |stat| stat.contract_id),
            native_contract_hook_hot_avg_us: native_hook_hot.map_or(0, |stat| stat.avg_us),
            neotoken_onpersist_hot_stage: neotoken_onpersist_hot.map_or("", |stat| stat.stage),
            neotoken_onpersist_hot_stage_avg_us: neotoken_onpersist_hot
                .map_or(0, |stat| stat.avg_us),
            neotoken_committee_compute_hot_stage: neotoken_committee_hot
                .map_or("", |stat| stat.stage),
            neotoken_committee_compute_hot_stage_avg_us: neotoken_committee_hot
                .map_or(0, |stat| stat.avg_us),
            neotoken_committee_candidate_hot_kind: neotoken_candidate_hot
                .map_or("", |stat| stat.kind),
            neotoken_committee_candidate_hot_avg: neotoken_candidate_hot.map_or(0, |stat| stat.avg),
            apply_attempts: apply.attempts,
            apply_failures: apply.failures,
            apply_height: apply.latest_height,
            avg_total_us: apply.avg_total_us,
            avg_project_us: apply.avg_project_us,
            avg_trie_us: apply.avg_apply_us,
            avg_changes: apply.avg_changes,
            enqueue_blocking_avg_us: apply_hot.enqueue_blocking_avg_us,
            queue_wait_avg_us: apply_hot.queue_wait_avg_us,
            mutate_changes_avg_us: apply_hot.mutate_changes_avg_us,
            root_hash_avg_us: apply_hot.root_hash_avg_us,
            trie_commit_avg_us: apply_hot.trie_commit_avg_us,
            backing_commit_avg_us: apply_hot.backing_commit_avg_us,
            publish_generation_avg_us: apply_hot.publish_generation_avg_us,
            overlay_entries_avg: apply_hot.overlay_entries_avg,
            batch_blocks_avg: apply_hot.batch_blocks_avg,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RocksDbBatchImportMetrics {
    pub(super) pending_operations: u64,
    pub(super) batches_flushed: u64,
    pub(super) operations_written: u64,
    pub(super) bytes_written: u64,
    pub(super) flush_timeouts: u64,
    pub(super) avg_ops_per_flush: u64,
    pub(super) avg_bytes_per_flush: u64,
    pub(super) avg_flush_duration_ms: u64,
    pub(super) max_batch_size: u64,
    pub(super) max_batch_bytes: u64,
    pub(super) disable_wal: bool,
}

impl RocksDbBatchImportMetrics {
    pub(super) fn from_store<S>(store: &S) -> Option<Self>
    where
        S: Store,
    {
        store.rocksdb_batch_metrics().map(Self::from_metrics)
    }

    pub(super) fn from_metrics(metrics: neo_storage::persistence::RocksDbBatchMetrics) -> Self {
        Self {
            pending_operations: metrics.pending_operations,
            batches_flushed: metrics.batches_flushed,
            operations_written: metrics.operations_written,
            bytes_written: metrics.bytes_written,
            flush_timeouts: metrics.flush_timeouts,
            avg_ops_per_flush: metrics.avg_ops_per_flush,
            avg_bytes_per_flush: metrics.avg_bytes_per_flush,
            avg_flush_duration_ms: metrics.avg_flush_duration_ms,
            max_batch_size: metrics.max_batch_size,
            max_batch_bytes: metrics.max_batch_bytes,
            disable_wal: metrics.disable_wal,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SyncHotPathMetrics {
    pub(super) blocks_persisted: u64,
    pub(super) avg_total_us: u64,
    pub(super) avg_verify_us: u64,
    pub(super) avg_persist_us: u64,
    pub(super) avg_commit_us: u64,
    pub(super) native_persist_avg_total_us: u64,
    pub(super) native_persist_avg_onpersist_us: u64,
    pub(super) native_persist_avg_tx_us: u64,
    pub(super) native_persist_avg_postpersist_us: u64,
    pub(super) native_persist_avg_cache_commit_us: u64,
    pub(super) native_persist_avg_tx_count: u64,
}

impl SyncHotPathMetrics {
    fn current() -> Self {
        Self {
            blocks_persisted: neo_runtime::sync_metrics::blocks_persisted(),
            avg_total_us: neo_runtime::sync_metrics::avg_total_us(),
            avg_verify_us: neo_runtime::sync_metrics::avg_verify_us(),
            avg_persist_us: neo_runtime::sync_metrics::avg_persist_us(),
            avg_commit_us: neo_runtime::sync_metrics::avg_commit_us(),
            native_persist_avg_total_us: neo_runtime::sync_metrics::native_persist_avg_total_us(),
            native_persist_avg_onpersist_us:
                neo_runtime::sync_metrics::native_persist_avg_onpersist_us(),
            native_persist_avg_tx_us: neo_runtime::sync_metrics::native_persist_avg_tx_us(),
            native_persist_avg_postpersist_us:
                neo_runtime::sync_metrics::native_persist_avg_postpersist_us(),
            native_persist_avg_cache_commit_us:
                neo_runtime::sync_metrics::native_persist_avg_cache_commit_us(),
            native_persist_avg_tx_count: neo_runtime::sync_metrics::native_persist_avg_tx_count(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ChainAccImportProgressSample {
    pub(super) imported: u64,
    pub(super) total: usize,
    pub(super) batch_imported: usize,
    pub(super) batch_blocks_per_second: f64,
    pub(super) average_blocks_per_second: f64,
    pub(super) elapsed_seconds: f64,
}

#[derive(Debug)]
pub(super) struct ChainAccImportProgress {
    total: usize,
    imported: u64,
    elapsed: Duration,
}

impl ChainAccImportProgress {
    pub(super) fn new(total: usize) -> Self {
        Self {
            total,
            imported: 0,
            elapsed: Duration::ZERO,
        }
    }

    pub(super) fn record_batch(&mut self, batch_imported: usize, batch_elapsed: Duration) {
        self.imported += batch_imported as u64;
        self.elapsed += batch_elapsed;
    }

    pub(super) fn imported(&self) -> u64 {
        self.imported
    }

    pub(super) fn elapsed_seconds(&self) -> f64 {
        self.elapsed.as_secs_f64()
    }

    pub(super) fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub(super) fn average_blocks_per_second(&self) -> f64 {
        blocks_per_second(self.imported, self.elapsed)
    }

    pub(super) fn sample(
        &self,
        batch_imported: usize,
        batch_elapsed: Duration,
    ) -> ChainAccImportProgressSample {
        ChainAccImportProgressSample {
            imported: self.imported,
            total: self.total,
            batch_imported,
            batch_blocks_per_second: blocks_per_second(batch_imported as u64, batch_elapsed),
            average_blocks_per_second: blocks_per_second(self.imported, self.elapsed),
            elapsed_seconds: self.elapsed.as_secs_f64(),
        }
    }
}

fn blocks_per_second(blocks: u64, elapsed: Duration) -> f64 {
    let elapsed = elapsed.as_secs_f64();
    if elapsed > 0.0 {
        blocks as f64 / elapsed
    } else {
        0.0
    }
}

const IMPORT_PROGRESS_LOG_INTERVAL: u64 = 10_000;

pub(super) fn should_log_import_progress(
    imported: u64,
    batch_imported: usize,
    batch_len: usize,
    total: usize,
) -> bool {
    let previous_imported = imported.saturating_sub(batch_imported as u64);
    batch_imported < batch_len
        || imported as usize >= total
        || (previous_imported / IMPORT_PROGRESS_LOG_INTERVAL
            < imported / IMPORT_PROGRESS_LOG_INTERVAL)
}
