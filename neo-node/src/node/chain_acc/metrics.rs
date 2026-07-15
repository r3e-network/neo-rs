//! Progress and hot-path metrics projection for `chain.acc` imports.

use std::time::Duration;

use serde::Serialize;

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
    pub(super) backing_sort_avg_us: u64,
    pub(super) backing_commit_avg_us: u64,
    pub(super) publish_generation_avg_us: u64,
    pub(super) overlay_entries_avg: u64,
    pub(super) batch_blocks_avg: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StateServiceMptCumulativeMetrics {
    apply: neo_state_service::StateRootApplyStats,
    stages: Vec<neo_state_service::metrics::StateRootApplyStageStats>,
    counts: Vec<neo_state_service::metrics::StateRootApplyCountStats>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct StateServiceMptWindowMetrics {
    pub(in crate::node) apply_attempts: u64,
    pub(in crate::node) apply_failures: u64,
    /// Sum of request queue-to-completion latency for this window.
    pub(in crate::node) end_to_end_total_us: u64,
    pub(in crate::node) avg_end_to_end_us: u64,
    /// Sum of actual trie application time for this window.
    pub(in crate::node) apply_total_us: u64,
    pub(in crate::node) avg_apply_us: u64,
    pub(in crate::node) project_total_us: u64,
    pub(in crate::node) avg_project_us: u64,
    pub(in crate::node) changes_total: u64,
    pub(in crate::node) avg_changes: u64,
    pub(in crate::node) stages: Vec<StateServiceMptStageWindowMetrics>,
    pub(in crate::node) counts: Vec<StateServiceMptCountWindowMetrics>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct StateServiceMptStageWindowMetrics {
    pub(in crate::node) stage: &'static str,
    pub(in crate::node) calls: u64,
    pub(in crate::node) total_us: u64,
    pub(in crate::node) avg_us: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct StateServiceMptCountWindowMetrics {
    pub(in crate::node) kind: &'static str,
    pub(in crate::node) samples: u64,
    pub(in crate::node) total: u64,
    pub(in crate::node) avg: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MdbxCommitCumulativeMetrics {
    stats: neo_storage::mdbx::MdbxCommitStats,
    stages: Vec<neo_storage::mdbx::MdbxCommitStageStats>,
    counts: Vec<neo_storage::mdbx::MdbxCommitCountStats>,
}

/// Exact MDBX commit work observed during one import window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct MdbxCommitWindowMetrics {
    pub(in crate::node) attempts: u64,
    pub(in crate::node) failures: u64,
    pub(in crate::node) committed_transactions: u64,
    pub(in crate::node) stages: Vec<MdbxCommitStageWindowMetrics>,
    pub(in crate::node) counts: Vec<MdbxCommitCountWindowMetrics>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct MdbxCommitStageWindowMetrics {
    pub(in crate::node) stage: &'static str,
    pub(in crate::node) calls: u64,
    pub(in crate::node) total_us: u64,
    pub(in crate::node) avg_us: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(in crate::node) struct MdbxCommitCountWindowMetrics {
    pub(in crate::node) kind: &'static str,
    pub(in crate::node) samples: u64,
    pub(in crate::node) total: u64,
    pub(in crate::node) avg: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct NativePersistTxStageImportMetrics {
    pub(super) hash_avg_us: u64,
    pub(super) cache_prepare_avg_us: u64,
    pub(super) container_prepare_avg_us: u64,
    pub(super) engine_create_avg_us: u64,
    pub(super) load_execute_avg_us: u64,
    pub(super) load_script_avg_us: u64,
    pub(super) execute_avg_us: u64,
    pub(super) application_executed_avg_us: u64,
    pub(super) tx_cache_commit_avg_us: u64,
    pub(super) ledger_vm_state_avg_us: u64,
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
            hash_avg_us: avg("hash"),
            cache_prepare_avg_us: avg("cache_prepare"),
            container_prepare_avg_us: avg("container_prepare"),
            engine_create_avg_us: avg("engine_create"),
            load_execute_avg_us: avg("load_execute"),
            load_script_avg_us: avg("load_script"),
            execute_avg_us: avg("execute"),
            application_executed_avg_us: avg("application_executed"),
            tx_cache_commit_avg_us: avg("tx_cache_commit"),
            ledger_vm_state_avg_us: avg("ledger_vm_state"),
        }
    }
}

impl StateServiceMptCumulativeMetrics {
    pub(super) fn current() -> Self {
        Self::from_stats(
            neo_state_service::StateRootApplyMetrics::state_root_apply_stats(),
            neo_state_service::StateRootApplyMetrics::state_root_apply_stage_stats(),
            neo_state_service::StateRootApplyMetrics::state_root_apply_count_stats(),
        )
    }

    pub(super) fn from_stats(
        apply: neo_state_service::StateRootApplyStats,
        stages: Vec<neo_state_service::metrics::StateRootApplyStageStats>,
        counts: Vec<neo_state_service::metrics::StateRootApplyCountStats>,
    ) -> Self {
        Self {
            apply,
            stages,
            counts,
        }
    }

    pub(super) fn window_since(&self, previous: &Self) -> StateServiceMptWindowMetrics {
        let apply_attempts = self.apply.attempts.saturating_sub(previous.apply.attempts);
        let apply_failures = self.apply.failures.saturating_sub(previous.apply.failures);
        let end_to_end_total_us = self.apply.total_us.saturating_sub(previous.apply.total_us);
        let apply_total_us = self
            .apply
            .apply_total_us
            .saturating_sub(previous.apply.apply_total_us);
        let project_total_us = self
            .apply
            .project_total_us
            .saturating_sub(previous.apply.project_total_us);
        let changes_total = self
            .apply
            .changes_total
            .saturating_sub(previous.apply.changes_total);

        StateServiceMptWindowMetrics {
            apply_attempts,
            apply_failures,
            end_to_end_total_us,
            avg_end_to_end_us: exact_average(end_to_end_total_us, apply_attempts),
            apply_total_us,
            avg_apply_us: exact_average(apply_total_us, apply_attempts),
            project_total_us,
            avg_project_us: exact_average(project_total_us, apply_attempts),
            changes_total,
            avg_changes: exact_average(changes_total, apply_attempts),
            stages: self
                .stages
                .iter()
                .map(|current| {
                    let previous = previous
                        .stages
                        .iter()
                        .find(|candidate| candidate.stage == current.stage);
                    let calls = current
                        .calls
                        .saturating_sub(previous.map_or(0, |stat| stat.calls));
                    let total_us = current
                        .total_us
                        .saturating_sub(previous.map_or(0, |stat| stat.total_us));
                    StateServiceMptStageWindowMetrics {
                        stage: current.stage,
                        calls,
                        total_us,
                        avg_us: exact_average(total_us, calls),
                    }
                })
                .collect(),
            counts: self
                .counts
                .iter()
                .map(|current| {
                    let previous = previous
                        .counts
                        .iter()
                        .find(|candidate| candidate.kind == current.kind);
                    let samples = current
                        .samples
                        .saturating_sub(previous.map_or(0, |stat| stat.samples));
                    let total = current
                        .total
                        .saturating_sub(previous.map_or(0, |stat| stat.total));
                    StateServiceMptCountWindowMetrics {
                        kind: current.kind,
                        samples,
                        total,
                        avg: exact_average(total, samples),
                    }
                })
                .collect(),
        }
    }
}

impl MdbxCommitCumulativeMetrics {
    pub(super) fn current() -> Self {
        let snapshot = neo_storage::mdbx::MdbxCommitMetrics::snapshot();
        Self::from_stats(snapshot.stats, snapshot.stages, snapshot.counts)
    }

    pub(super) fn from_stats(
        stats: neo_storage::mdbx::MdbxCommitStats,
        stages: Vec<neo_storage::mdbx::MdbxCommitStageStats>,
        counts: Vec<neo_storage::mdbx::MdbxCommitCountStats>,
    ) -> Self {
        Self {
            stats,
            stages,
            counts,
        }
    }

    pub(super) fn window_since(&self, previous: &Self) -> MdbxCommitWindowMetrics {
        MdbxCommitWindowMetrics {
            attempts: self.stats.attempts.saturating_sub(previous.stats.attempts),
            failures: self.stats.failures.saturating_sub(previous.stats.failures),
            committed_transactions: self
                .stats
                .committed_transactions
                .saturating_sub(previous.stats.committed_transactions),
            stages: self
                .stages
                .iter()
                .map(|current| {
                    let previous = previous
                        .stages
                        .iter()
                        .find(|candidate| candidate.stage == current.stage);
                    let calls = current
                        .calls
                        .saturating_sub(previous.map_or(0, |stat| stat.calls));
                    let total_us = current
                        .total_us
                        .saturating_sub(previous.map_or(0, |stat| stat.total_us));
                    MdbxCommitStageWindowMetrics {
                        stage: current.stage,
                        calls,
                        total_us,
                        avg_us: exact_average(total_us, calls),
                    }
                })
                .collect(),
            counts: self
                .counts
                .iter()
                .map(|current| {
                    let previous = previous
                        .counts
                        .iter()
                        .find(|candidate| candidate.kind == current.kind);
                    let samples = current
                        .samples
                        .saturating_sub(previous.map_or(0, |stat| stat.samples));
                    let total = current
                        .total
                        .saturating_sub(previous.map_or(0, |stat| stat.total));
                    MdbxCommitCountWindowMetrics {
                        kind: current.kind,
                        samples,
                        total,
                        avg: exact_average(total, samples),
                    }
                })
                .collect(),
        }
    }
}

impl StateServiceMptWindowMetrics {
    pub(super) fn stage_total_us(&self, stage: &str) -> u64 {
        self.stages
            .iter()
            .find(|metric| metric.stage == stage)
            .map_or(0, |metric| metric.total_us)
    }

    pub(super) fn stage_avg_us(&self, stage: &str) -> u64 {
        self.stages
            .iter()
            .find(|metric| metric.stage == stage)
            .map_or(0, |metric| metric.avg_us)
    }

    pub(super) fn count_total(&self, kind: &str) -> u64 {
        self.counts
            .iter()
            .find(|metric| metric.kind == kind)
            .map_or(0, |metric| metric.total)
    }

    pub(super) fn count_avg(&self, kind: &str) -> u64 {
        self.counts
            .iter()
            .find(|metric| metric.kind == kind)
            .map_or(0, |metric| metric.avg)
    }
}

impl MdbxCommitWindowMetrics {
    pub(super) fn stage_total_us(&self, stage: &str) -> u64 {
        self.stages
            .iter()
            .find(|metric| metric.stage == stage)
            .map_or(0, |metric| metric.total_us)
    }

    pub(super) fn stage_avg_us(&self, stage: &str) -> u64 {
        self.stages
            .iter()
            .find(|metric| metric.stage == stage)
            .map_or(0, |metric| metric.avg_us)
    }

    pub(super) fn count_total(&self, kind: &str) -> u64 {
        self.counts
            .iter()
            .find(|metric| metric.kind == kind)
            .map_or(0, |metric| metric.total)
    }
}

fn exact_average(total: u64, samples: u64) -> u64 {
    total.checked_div(samples).unwrap_or(0)
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
            backing_sort_avg_us: apply_hot.backing_sort_avg_us,
            backing_commit_avg_us: apply_hot.backing_commit_avg_us,
            publish_generation_avg_us: apply_hot.publish_generation_avg_us,
            overlay_entries_avg: apply_hot.overlay_entries_avg,
            batch_blocks_avg: apply_hot.batch_blocks_avg,
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
