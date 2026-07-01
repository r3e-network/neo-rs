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
    pub(super) mutate_changes_avg_us: u64,
    pub(super) root_hash_avg_us: u64,
    pub(super) trie_commit_avg_us: u64,
    pub(super) backing_commit_avg_us: u64,
    pub(super) publish_generation_avg_us: u64,
    pub(super) overlay_entries_avg: u64,
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

    fn from_direct_hot_snapshot(
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
            mutate_changes_avg_us: apply_hot.mutate_changes_avg_us,
            root_hash_avg_us: apply_hot.root_hash_avg_us,
            trie_commit_avg_us: apply_hot.trie_commit_avg_us,
            backing_commit_avg_us: apply_hot.backing_commit_avg_us,
            publish_generation_avg_us: apply_hot.publish_generation_avg_us,
            overlay_entries_avg: apply_hot.overlay_entries_avg,
        }
    }

    #[cfg(test)]
    fn from_parts(
        sync: SyncHotPathMetrics,
        apply: neo_state_service::StateRootApplyStats,
        stages: &[neo_state_service::metrics::StateRootApplyStageStats],
        counts: &[neo_state_service::metrics::StateRootApplyCountStats],
        native_hooks: &[neo_runtime::sync_metrics::NativeContractHookStats],
        native_tx_stages: &[neo_runtime::sync_metrics::NativePersistTxStageStats],
        neotoken_onpersist: &[neo_runtime::sync_metrics::NeoTokenOnPersistStageStats],
        neotoken_committee: &[neo_runtime::sync_metrics::NeoTokenCommitteeComputeStageStats],
        neotoken_candidate_counts: &[neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCountStats],
    ) -> Self {
        let stage_avg = |name: &str| -> u64 {
            stages
                .iter()
                .find(|stat| stat.stage == name)
                .map_or(0, |stat| stat.avg_us)
        };
        let count_avg = |name: &str| -> u64 {
            counts
                .iter()
                .find(|stat| stat.kind == name)
                .map_or(0, |stat| stat.avg)
        };
        let native_hook_hot = native_hooks
            .iter()
            .filter(|stat| stat.avg_us > 0)
            .max_by_key(|stat| stat.avg_us);
        let native_tx_hot = native_tx_stages
            .iter()
            .filter(|stat| stat.avg_us > 0)
            .max_by_key(|stat| stat.avg_us);
        let neotoken_onpersist_hot = neotoken_onpersist
            .iter()
            .filter(|stat| stat.avg_us > 0)
            .max_by_key(|stat| stat.avg_us);
        let neotoken_committee_hot = neotoken_committee
            .iter()
            .filter(|stat| stat.avg_us > 0)
            .max_by_key(|stat| stat.avg_us);
        let neotoken_candidate_hot = neotoken_candidate_counts
            .iter()
            .filter(|stat| stat.avg > 0)
            .max_by_key(|stat| stat.avg);

        Self::from_direct_hot_snapshot(
            sync,
            apply,
            neo_state_service::metrics::StateRootApplyHotStats {
                mutate_changes_avg_us: stage_avg("mutate_changes"),
                root_hash_avg_us: stage_avg("root_hash"),
                trie_commit_avg_us: stage_avg("trie_commit"),
                backing_commit_avg_us: stage_avg("backing_commit"),
                publish_generation_avg_us: stage_avg("publish_generation"),
                overlay_entries_avg: count_avg("overlay_entries"),
            },
            native_hook_hot.copied(),
            native_tx_hot.copied(),
            neotoken_onpersist_hot.copied(),
            neotoken_committee_hot.copied(),
            neotoken_candidate_hot.copied(),
        )
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
    pub(super) fn from_store(store: &dyn Store) -> Option<Self> {
        let rocksdb = store
            .as_any()
            .downcast_ref::<neo_storage::rocksdb::RocksDbStore>()?;
        Some(Self::from_parts(
            rocksdb.batch_commit_stats(),
            rocksdb.write_batch_config(),
        ))
    }

    fn from_parts(
        stats: neo_storage::rocksdb::WriteBatchStatsSnapshot,
        config: neo_storage::rocksdb::WriteBatchConfig,
    ) -> Self {
        Self {
            pending_operations: stats.pending_operations as u64,
            batches_flushed: stats.batches_flushed,
            operations_written: stats.operations_written,
            bytes_written: stats.bytes_written,
            flush_timeouts: stats.flush_timeouts,
            avg_ops_per_flush: stats.avg_ops_per_flush() as u64,
            avg_bytes_per_flush: stats.avg_bytes_per_flush() as u64,
            avg_flush_duration_ms: stats.avg_flush_duration_ms() as u64,
            max_batch_size: config.max_batch_size as u64,
            max_batch_bytes: config.max_batch_bytes as u64,
            disable_wal: config.disable_wal,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SyncHotPathMetrics {
    blocks_persisted: u64,
    avg_total_us: u64,
    avg_verify_us: u64,
    avg_persist_us: u64,
    avg_commit_us: u64,
    native_persist_avg_total_us: u64,
    native_persist_avg_onpersist_us: u64,
    native_persist_avg_tx_us: u64,
    native_persist_avg_postpersist_us: u64,
    native_persist_avg_cache_commit_us: u64,
    native_persist_avg_tx_count: u64,
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
    batch_imported < batch_len
        || imported as usize >= total
        || (imported > 0 && imported % IMPORT_PROGRESS_LOG_INTERVAL == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_progress_reports_batch_and_average_rates() {
        let mut progress = ChainAccImportProgress::new(100);

        progress.record_batch(25, std::time::Duration::from_millis(50));
        let first = progress.sample(25, std::time::Duration::from_millis(50));
        progress.record_batch(25, std::time::Duration::from_millis(100));
        let second = progress.sample(25, std::time::Duration::from_millis(100));

        assert_eq!(first.imported, 25);
        assert_eq!(first.total, 100);
        assert_eq!(first.batch_imported, 25);
        assert_eq!(first.batch_blocks_per_second, 500.0);
        assert_eq!(first.average_blocks_per_second, 500.0);
        assert_eq!(second.imported, 50);
        assert_eq!(second.batch_blocks_per_second, 250.0);
        assert!((second.average_blocks_per_second - (50.0 / 0.15)).abs() < 1e-9);
    }

    #[test]
    fn import_progress_reports_zero_rate_without_elapsed_time() {
        let progress = ChainAccImportProgress::new(100);

        assert_eq!(progress.imported(), 0);
        assert_eq!(progress.elapsed_seconds(), 0.0);
        assert_eq!(progress.average_blocks_per_second(), 0.0);
    }

    #[test]
    fn import_progress_logging_is_limited_to_boundaries_failures_and_final_batch() {
        assert!(!should_log_import_progress(9_500, 500, 500, 20_000));
        assert!(should_log_import_progress(10_000, 500, 500, 20_000));
        assert!(should_log_import_progress(10_500, 499, 500, 20_000));
        assert!(should_log_import_progress(20_000, 500, 500, 20_000));
    }

    #[test]
    fn state_service_import_metrics_projects_hot_stage_fields() {
        let sync = SyncHotPathMetrics {
            blocks_persisted: 11,
            avg_total_us: 1_000,
            avg_verify_us: 100,
            avg_persist_us: 600,
            avg_commit_us: 300,
            native_persist_avg_total_us: 700,
            native_persist_avg_onpersist_us: 200,
            native_persist_avg_tx_us: 50,
            native_persist_avg_postpersist_us: 250,
            native_persist_avg_cache_commit_us: 100,
            native_persist_avg_tx_count: 3,
        };
        let apply = neo_state_service::StateRootApplyStats {
            attempts: 12,
            failures: 1,
            latest_height: 42,
            avg_total_us: 9_000,
            avg_project_us: 700,
            avg_apply_us: 8_200,
            avg_changes: 17,
        };
        let stages = vec![
            neo_state_service::metrics::StateRootApplyStageStats {
                stage: "mutate_changes",
                calls: 12,
                avg_us: 2_000,
            },
            neo_state_service::metrics::StateRootApplyStageStats {
                stage: "trie_commit",
                calls: 12,
                avg_us: 3_000,
            },
            neo_state_service::metrics::StateRootApplyStageStats {
                stage: "backing_commit",
                calls: 12,
                avg_us: 4_000,
            },
            neo_state_service::metrics::StateRootApplyStageStats {
                stage: "publish_generation",
                calls: 12,
                avg_us: 5_000,
            },
        ];
        let counts = vec![neo_state_service::metrics::StateRootApplyCountStats {
            kind: "overlay_entries",
            samples: 12,
            total: 240,
            avg: 20,
        }];
        let native_hooks = vec![
            neo_runtime::sync_metrics::NativeContractHookStats {
                trigger: "onpersist",
                contract_id: -5,
                contract: "NeoToken",
                calls: 10,
                avg_us: 1_200,
            },
            neo_runtime::sync_metrics::NativeContractHookStats {
                trigger: "onpersist",
                contract_id: -6,
                contract: "GasToken",
                calls: 10,
                avg_us: 7_100,
            },
        ];
        let native_tx_stages = vec![
            neo_runtime::sync_metrics::NativePersistTxStageStats {
                stage: "execute",
                calls: 10,
                avg_us: 8_100,
            },
            neo_runtime::sync_metrics::NativePersistTxStageStats {
                stage: "ledger_vm_state",
                calls: 10,
                avg_us: 1_700,
            },
        ];
        let neotoken_onpersist = vec![
            neo_runtime::sync_metrics::NeoTokenOnPersistStageStats {
                stage: "read_cached_committee",
                calls: 10,
                avg_us: 300,
            },
            neo_runtime::sync_metrics::NeoTokenOnPersistStageStats {
                stage: "compute_committee",
                calls: 10,
                avg_us: 3_300,
            },
        ];
        let neotoken_committee = vec![
            neo_runtime::sync_metrics::NeoTokenCommitteeComputeStageStats {
                stage: "candidate_state_decode",
                calls: 10,
                avg_us: 2_100,
            },
            neo_runtime::sync_metrics::NeoTokenCommitteeComputeStageStats {
                stage: "top_candidate_maintenance",
                calls: 10,
                avg_us: 700,
            },
        ];
        let neotoken_candidate_counts = vec![
            neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCountStats {
                kind: "registered_entries",
                samples: 10,
                total: 120,
                avg: 12,
            },
            neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCountStats {
                kind: "eligible_candidates",
                samples: 10,
                total: 420,
                avg: 42,
            },
        ];

        let metrics = StateServiceMptImportMetrics::from_parts(
            sync,
            apply,
            &stages,
            &counts,
            &native_hooks,
            &native_tx_stages,
            &neotoken_onpersist,
            &neotoken_committee,
            &neotoken_candidate_counts,
        );

        assert_eq!(metrics.sync_blocks_persisted, 11);
        assert_eq!(metrics.sync_avg_total_us, 1_000);
        assert_eq!(metrics.sync_avg_verify_us, 100);
        assert_eq!(metrics.sync_avg_persist_us, 600);
        assert_eq!(metrics.sync_avg_commit_us, 300);
        assert_eq!(metrics.native_persist_avg_total_us, 700);
        assert_eq!(metrics.native_persist_avg_onpersist_us, 200);
        assert_eq!(metrics.native_persist_avg_tx_us, 50);
        assert_eq!(metrics.native_persist_avg_postpersist_us, 250);
        assert_eq!(metrics.native_persist_avg_cache_commit_us, 100);
        assert_eq!(metrics.native_persist_avg_tx_count, 3);
        assert_eq!(metrics.apply_attempts, 12);
        assert_eq!(metrics.apply_failures, 1);
        assert_eq!(metrics.apply_height, 42);
        assert_eq!(metrics.avg_total_us, 9_000);
        assert_eq!(metrics.avg_project_us, 700);
        assert_eq!(metrics.avg_trie_us, 8_200);
        assert_eq!(metrics.avg_changes, 17);
        assert_eq!(metrics.mutate_changes_avg_us, 2_000);
        assert_eq!(metrics.trie_commit_avg_us, 3_000);
        assert_eq!(metrics.backing_commit_avg_us, 4_000);
        assert_eq!(metrics.publish_generation_avg_us, 5_000);
        assert_eq!(metrics.overlay_entries_avg, 20);
        assert_eq!(metrics.native_contract_hook_hot_trigger, "onpersist");
        assert_eq!(metrics.native_contract_hook_hot_contract, "GasToken");
        assert_eq!(metrics.native_contract_hook_hot_contract_id, -6);
        assert_eq!(metrics.native_contract_hook_hot_avg_us, 7_100);
        assert_eq!(metrics.native_persist_tx_hot_stage, "execute");
        assert_eq!(metrics.native_persist_tx_hot_stage_avg_us, 8_100);
        assert_eq!(metrics.neotoken_onpersist_hot_stage, "compute_committee");
        assert_eq!(metrics.neotoken_onpersist_hot_stage_avg_us, 3_300);
        assert_eq!(
            metrics.neotoken_committee_compute_hot_stage,
            "candidate_state_decode"
        );
        assert_eq!(metrics.neotoken_committee_compute_hot_stage_avg_us, 2_100);
        assert_eq!(
            metrics.neotoken_committee_candidate_hot_kind,
            "eligible_candidates"
        );
        assert_eq!(metrics.neotoken_committee_candidate_hot_avg, 42);
    }

    #[test]
    fn state_service_import_metrics_projects_direct_hot_snapshot_fields() {
        let sync = SyncHotPathMetrics {
            blocks_persisted: 11,
            avg_total_us: 1_000,
            avg_verify_us: 100,
            avg_persist_us: 600,
            avg_commit_us: 300,
            native_persist_avg_total_us: 700,
            native_persist_avg_onpersist_us: 200,
            native_persist_avg_tx_us: 50,
            native_persist_avg_postpersist_us: 250,
            native_persist_avg_cache_commit_us: 100,
            native_persist_avg_tx_count: 3,
        };
        let apply = neo_state_service::StateRootApplyStats {
            attempts: 12,
            failures: 1,
            latest_height: 42,
            avg_total_us: 9_000,
            avg_project_us: 700,
            avg_apply_us: 8_200,
            avg_changes: 17,
        };
        let apply_hot = neo_state_service::metrics::StateRootApplyHotStats {
            mutate_changes_avg_us: 2_000,
            root_hash_avg_us: 2_500,
            trie_commit_avg_us: 3_000,
            backing_commit_avg_us: 4_000,
            publish_generation_avg_us: 5_000,
            overlay_entries_avg: 20,
        };
        let native_hook_hot = Some(neo_runtime::sync_metrics::NativeContractHookStats {
            trigger: "onpersist",
            contract_id: -6,
            contract: "GasToken",
            calls: 10,
            avg_us: 7_100,
        });
        let native_tx_hot = Some(neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "execute",
            calls: 10,
            avg_us: 8_100,
        });
        let neotoken_onpersist_hot = Some(neo_runtime::sync_metrics::NeoTokenOnPersistStageStats {
            stage: "compute_committee",
            calls: 10,
            avg_us: 3_300,
        });
        let neotoken_committee_hot = Some(
            neo_runtime::sync_metrics::NeoTokenCommitteeComputeStageStats {
                stage: "candidate_state_decode",
                calls: 10,
                avg_us: 2_100,
            },
        );
        let neotoken_candidate_hot = Some(
            neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCountStats {
                kind: "eligible_candidates",
                samples: 10,
                total: 420,
                avg: 42,
            },
        );

        let metrics = StateServiceMptImportMetrics::from_direct_hot_snapshot(
            sync,
            apply,
            apply_hot,
            native_hook_hot,
            native_tx_hot,
            neotoken_onpersist_hot,
            neotoken_committee_hot,
            neotoken_candidate_hot,
        );

        assert_eq!(metrics.sync_blocks_persisted, 11);
        assert_eq!(metrics.apply_attempts, 12);
        assert_eq!(metrics.mutate_changes_avg_us, 2_000);
        assert_eq!(metrics.root_hash_avg_us, 2_500);
        assert_eq!(metrics.trie_commit_avg_us, 3_000);
        assert_eq!(metrics.overlay_entries_avg, 20);
        assert_eq!(metrics.native_contract_hook_hot_contract, "GasToken");
        assert_eq!(metrics.native_persist_tx_hot_stage, "execute");
        assert_eq!(metrics.neotoken_onpersist_hot_stage, "compute_committee");
        assert_eq!(
            metrics.neotoken_committee_compute_hot_stage,
            "candidate_state_decode"
        );
        assert_eq!(
            metrics.neotoken_committee_candidate_hot_kind,
            "eligible_candidates"
        );
    }

    #[test]
    fn rocksdb_batch_import_metrics_projects_buffer_stats() {
        let stats = neo_storage::rocksdb::WriteBatchStatsSnapshot {
            batches_flushed: 2,
            operations_written: 17,
            bytes_written: 4096,
            total_flush_duration_ms: 12,
            flush_timeouts: 1,
            pending_operations: 5,
        };
        let config = neo_storage::rocksdb::WriteBatchConfig::high_throughput();

        let metrics = RocksDbBatchImportMetrics::from_parts(stats, config);

        assert_eq!(metrics.pending_operations, 5);
        assert_eq!(metrics.batches_flushed, 2);
        assert_eq!(metrics.operations_written, 17);
        assert_eq!(metrics.bytes_written, 4096);
        assert_eq!(metrics.flush_timeouts, 1);
        assert_eq!(metrics.avg_ops_per_flush, 8);
        assert_eq!(metrics.avg_bytes_per_flush, 2048);
        assert_eq!(metrics.avg_flush_duration_ms, 6);
        assert_eq!(metrics.max_batch_size, 50_000);
        assert_eq!(metrics.max_batch_bytes, 64 * 1024 * 1024);
        assert!(metrics.disable_wal);
    }
}
