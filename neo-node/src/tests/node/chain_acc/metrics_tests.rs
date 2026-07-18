//! Tests for `chain_acc::metrics` projection helpers.

use super::metrics::{
    ChainAccImportProgress, MdbxCommitCumulativeMetrics, NativePersistTxCumulativeMetrics,
    NativePersistTxStageImportMetrics, StateServiceMptCumulativeMetrics,
    StateServiceMptImportMetrics, SyncHotPathMetrics, should_log_import_progress,
};

fn state_service_metrics_from_parts(
    sync: SyncHotPathMetrics,
    apply: neo_state_service::StateRootApplyStats,
    stages: &[neo_state_service::metrics::StateRootApplyStageStats],
    counts: &[neo_state_service::metrics::StateRootApplyCountStats],
    native_hooks: &[neo_runtime::sync_metrics::NativeContractHookStats],
    native_tx_stages: &[neo_runtime::sync_metrics::NativePersistTxStageStats],
    neotoken_onpersist: &[neo_runtime::sync_metrics::NeoTokenOnPersistStageStats],
    neotoken_committee: &[neo_runtime::sync_metrics::NeoTokenCommitteeComputeStageStats],
    neotoken_candidate_counts: &[neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCountStats],
) -> StateServiceMptImportMetrics {
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

    StateServiceMptImportMetrics::from_direct_hot_snapshot(
        sync,
        apply,
        neo_state_service::metrics::StateRootApplyHotStats {
            enqueue_blocking_avg_us: stage_avg("enqueue_blocking"),
            queue_wait_avg_us: stage_avg("queue_wait"),
            mutate_changes_avg_us: stage_avg("mutate_changes"),
            root_hash_avg_us: stage_avg("root_hash"),
            trie_commit_avg_us: stage_avg("trie_commit"),
            backing_sort_avg_us: stage_avg("backing_sort"),
            backing_commit_avg_us: stage_avg("backing_commit"),
            publish_generation_avg_us: stage_avg("publish_generation"),
            overlay_entries_avg: count_avg("overlay_entries"),
            batch_blocks_avg: count_avg("batch_blocks"),
        },
        native_hook_hot.copied(),
        native_tx_hot.copied(),
        neotoken_onpersist_hot.copied(),
        neotoken_committee_hot.copied(),
        neotoken_candidate_hot.copied(),
    )
}

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
    assert!(should_log_import_progress(10_112, 128, 128, 20_000));
    assert!(should_log_import_progress(10_500, 499, 500, 20_000));
    assert!(should_log_import_progress(20_000, 500, 500, 20_000));
}

#[test]
fn native_tx_stage_import_metrics_project_split_load_execute_stages() {
    let stages = vec![
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "load_execute",
            calls: 5,
            total_us: 2_100,
            avg_us: 420,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "load_script",
            calls: 5,
            total_us: 175,
            avg_us: 35,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "execute",
            calls: 5,
            total_us: 1_925,
            avg_us: 385,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "ledger_vm_state",
            calls: 5,
            total_us: 55,
            avg_us: 11,
        },
    ];

    let metrics = NativePersistTxStageImportMetrics::from_stats(&stages);

    assert_eq!(metrics.load_execute_avg_us, 420);
    assert_eq!(metrics.load_script_avg_us, 35);
    assert_eq!(metrics.execute_avg_us, 385);
}

#[test]
fn native_tx_window_metrics_use_exact_cumulative_deltas() {
    let before = NativePersistTxCumulativeMetrics::from_stats(vec![
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "execute",
            calls: 10,
            total_us: 1_000,
            avg_us: 999,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "load_script",
            calls: 2,
            total_us: 50,
            avg_us: 25,
        },
    ]);
    let after = NativePersistTxCumulativeMetrics::from_stats(vec![
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "execute",
            calls: 14,
            total_us: 1_800,
            avg_us: 123,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "load_script",
            calls: 2,
            total_us: 50,
            avg_us: 25,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "ledger_vm_state",
            calls: 3,
            total_us: 90,
            avg_us: 777,
        },
    ]);

    let window = after.window_since(&before);
    let avg_us = |stage: &str| {
        window
            .stages
            .iter()
            .find(|metric| metric.stage == stage)
            .map_or(0, |metric| metric.avg_us)
    };

    assert_eq!(window.stage_calls("execute"), 4);
    assert_eq!(window.stage_total_us("execute"), 800);
    assert_eq!(avg_us("execute"), 200);
    assert_eq!(window.stage_calls("load_script"), 0);
    assert_eq!(avg_us("load_script"), 0);
    assert_eq!(window.stage_calls("ledger_vm_state"), 3);
    assert_eq!(window.stage_total_us("ledger_vm_state"), 90);
    assert_eq!(avg_us("ledger_vm_state"), 30);
    assert_eq!(window.stage_total_us("missing"), 0);
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
        total_us: 108_000,
        project_total_us: 8_400,
        apply_total_us: 98_400,
        changes_total: 204,
    };
    let stages = vec![
        neo_state_service::metrics::StateRootApplyStageStats {
            stage: "enqueue_blocking",
            calls: 12,
            total_us: 9_600,
            avg_us: 800,
        },
        neo_state_service::metrics::StateRootApplyStageStats {
            stage: "queue_wait",
            calls: 12,
            total_us: 22_800,
            avg_us: 1_900,
        },
        neo_state_service::metrics::StateRootApplyStageStats {
            stage: "mutate_changes",
            calls: 12,
            total_us: 24_000,
            avg_us: 2_000,
        },
        neo_state_service::metrics::StateRootApplyStageStats {
            stage: "trie_commit",
            calls: 12,
            total_us: 36_000,
            avg_us: 3_000,
        },
        neo_state_service::metrics::StateRootApplyStageStats {
            stage: "backing_commit",
            calls: 12,
            total_us: 48_000,
            avg_us: 4_000,
        },
        neo_state_service::metrics::StateRootApplyStageStats {
            stage: "publish_generation",
            calls: 12,
            total_us: 60_000,
            avg_us: 5_000,
        },
    ];
    let counts = vec![
        neo_state_service::metrics::StateRootApplyCountStats {
            kind: "overlay_entries",
            samples: 12,
            total: 240,
            avg: 20,
        },
        neo_state_service::metrics::StateRootApplyCountStats {
            kind: "batch_blocks",
            samples: 6,
            total: 30,
            avg: 5,
        },
    ];
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
            total_us: 81_000,
            avg_us: 8_100,
        },
        neo_runtime::sync_metrics::NativePersistTxStageStats {
            stage: "ledger_vm_state",
            calls: 10,
            total_us: 17_000,
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

    let metrics = state_service_metrics_from_parts(
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
    assert_eq!(metrics.enqueue_blocking_avg_us, 800);
    assert_eq!(metrics.queue_wait_avg_us, 1_900);
    assert_eq!(metrics.mutate_changes_avg_us, 2_000);
    assert_eq!(metrics.trie_commit_avg_us, 3_000);
    assert_eq!(metrics.backing_commit_avg_us, 4_000);
    assert_eq!(metrics.publish_generation_avg_us, 5_000);
    assert_eq!(metrics.overlay_entries_avg, 20);
    assert_eq!(metrics.batch_blocks_avg, 5);
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
        total_us: 108_000,
        project_total_us: 8_400,
        apply_total_us: 98_400,
        changes_total: 204,
    };
    let apply_hot = neo_state_service::metrics::StateRootApplyHotStats {
        enqueue_blocking_avg_us: 800,
        queue_wait_avg_us: 1_900,
        mutate_changes_avg_us: 2_000,
        root_hash_avg_us: 2_500,
        trie_commit_avg_us: 3_000,
        backing_sort_avg_us: 3_500,
        backing_commit_avg_us: 4_000,
        publish_generation_avg_us: 5_000,
        overlay_entries_avg: 20,
        batch_blocks_avg: 5,
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
        total_us: 81_000,
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
    assert_eq!(metrics.enqueue_blocking_avg_us, 800);
    assert_eq!(metrics.queue_wait_avg_us, 1_900);
    assert_eq!(metrics.mutate_changes_avg_us, 2_000);
    assert_eq!(metrics.root_hash_avg_us, 2_500);
    assert_eq!(metrics.trie_commit_avg_us, 3_000);
    assert_eq!(metrics.backing_sort_avg_us, 3_500);
    assert_eq!(metrics.overlay_entries_avg, 20);
    assert_eq!(metrics.batch_blocks_avg, 5);
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
fn state_service_window_metrics_use_exact_cumulative_deltas() {
    let before = StateServiceMptCumulativeMetrics::from_stats(
        neo_state_service::StateRootApplyStats {
            attempts: 100,
            failures: 2,
            latest_height: 99,
            avg_total_us: 999,
            avg_project_us: 999,
            avg_apply_us: 999,
            avg_changes: 999,
            total_us: 10_000,
            project_total_us: 1_000,
            apply_total_us: 8_000,
            changes_total: 200,
        },
        vec![neo_state_service::metrics::StateRootApplyStageStats {
            stage: "backing_commit",
            calls: 100,
            total_us: 1_000,
            avg_us: 999,
        }],
        vec![
            neo_state_service::metrics::StateRootApplyCountStats {
                kind: "batch_blocks",
                samples: 10,
                total: 1_000,
                avg: 999,
            },
            neo_state_service::metrics::StateRootApplyCountStats {
                kind: "changes",
                samples: 100,
                total: 200,
                avg: 999,
            },
        ],
    );
    let after = StateServiceMptCumulativeMetrics::from_stats(
        neo_state_service::StateRootApplyStats {
            attempts: 110,
            failures: 3,
            latest_height: 109,
            avg_total_us: 1,
            avg_project_us: 1,
            avg_apply_us: 1,
            avg_changes: 1,
            total_us: 20_000,
            project_total_us: 2_000,
            apply_total_us: 17_000,
            changes_total: 370,
        },
        vec![neo_state_service::metrics::StateRootApplyStageStats {
            stage: "backing_commit",
            calls: 110,
            total_us: 4_000,
            avg_us: 1,
        }],
        vec![
            neo_state_service::metrics::StateRootApplyCountStats {
                kind: "batch_blocks",
                samples: 11,
                total: 11_000,
                avg: 1,
            },
            neo_state_service::metrics::StateRootApplyCountStats {
                kind: "changes",
                samples: 110,
                total: 370,
                avg: 1,
            },
        ],
    );

    let window = after.window_since(&before);

    assert_eq!(window.apply_attempts, 10);
    assert_eq!(window.apply_failures, 1);
    assert_eq!(
        (window.end_to_end_total_us, window.avg_end_to_end_us),
        (10_000, 1_000)
    );
    assert_eq!((window.apply_total_us, window.avg_apply_us), (9_000, 900));
    assert_eq!(
        (window.project_total_us, window.avg_project_us),
        (1_000, 100)
    );
    assert_eq!((window.changes_total, window.avg_changes), (170, 17));
    assert_eq!(window.stages[0].stage, "backing_commit");
    assert_eq!(
        (window.stages[0].calls, window.stages[0].total_us),
        (10, 3_000)
    );
    assert_eq!(window.stages[0].avg_us, 300);
    assert_eq!(window.counts[0].kind, "batch_blocks");
    assert_eq!(
        (window.counts[0].samples, window.counts[0].total),
        (1, 10_000)
    );
    assert_eq!(window.counts[0].avg, 10_000);
    assert_eq!(window.counts[1].avg, 17);
    assert_eq!(window.stage_total_us("backing_commit"), 3_000);
    assert_eq!(window.stage_avg_us("backing_commit"), 300);
    assert_eq!(window.count_total("batch_blocks"), 10_000);
    assert_eq!(window.count_avg("batch_blocks"), 10_000);
    assert_eq!(window.stage_total_us("missing"), 0);
    assert_eq!(window.count_total("missing"), 0);
}

#[test]
fn mdbx_commit_window_metrics_use_exact_cumulative_deltas() {
    let before = MdbxCommitCumulativeMetrics::from_stats(
        neo_storage::mdbx::MdbxCommitStats {
            attempts: 4,
            failures: 1,
            committed_transactions: 3,
        },
        vec![neo_storage::mdbx::MdbxCommitStageStats {
            stage: "cursor_write",
            calls: 6,
            total_us: 2_000,
            avg_us: 999,
        }],
        vec![neo_storage::mdbx::MdbxCommitCountStats {
            kind: "value_bytes",
            samples: 4,
            total: 8_000,
            avg: 999,
        }],
    );
    let after = MdbxCommitCumulativeMetrics::from_stats(
        neo_storage::mdbx::MdbxCommitStats {
            attempts: 6,
            failures: 1,
            committed_transactions: 5,
        },
        vec![neo_storage::mdbx::MdbxCommitStageStats {
            stage: "cursor_write",
            calls: 10,
            total_us: 5_000,
            avg_us: 1,
        }],
        vec![neo_storage::mdbx::MdbxCommitCountStats {
            kind: "value_bytes",
            samples: 6,
            total: 20_000,
            avg: 1,
        }],
    );

    let window = after.window_since(&before);

    assert_eq!(window.attempts, 2);
    assert_eq!(window.failures, 0);
    assert_eq!(window.committed_transactions, 2);
    assert_eq!(window.stage_total_us("cursor_write"), 3_000);
    assert_eq!(window.stage_avg_us("cursor_write"), 750);
    assert_eq!(window.count_total("value_bytes"), 12_000);
    assert_eq!(window.stage_total_us("missing"), 0);
    assert_eq!(window.count_total("missing"), 0);
}
