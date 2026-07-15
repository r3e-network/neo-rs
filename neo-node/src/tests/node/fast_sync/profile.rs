use super::*;

#[test]
fn report_preserves_transaction_throughput_and_profile_window() {
    let package = test_package(0, 100);
    let import_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([100; 32]),
    };
    let mut state_service_mpt = chain_acc::StateServiceMptWindowMetrics {
        apply_attempts: 101,
        end_to_end_total_us: 50_500,
        avg_end_to_end_us: 500,
        apply_total_us: 20_200,
        avg_apply_us: 200,
        stages: vec![Default::default()],
        counts: vec![Default::default()],
        ..Default::default()
    };
    state_service_mpt.stages[0].stage = "backing_commit";
    state_service_mpt.stages[0].calls = 101;
    state_service_mpt.stages[0].total_us = 30_300;
    state_service_mpt.stages[0].avg_us = 300;
    state_service_mpt.counts[0].kind = "batch_blocks";
    state_service_mpt.counts[0].samples = 1;
    state_service_mpt.counts[0].total = 101;
    state_service_mpt.counts[0].avg = 101;
    let mut import = import_report_with_composition(101, Some(import_tip), 0.25, 404.0, 81, 20, 45);
    import
        .profile_windows
        .push(chain_acc::ChainAccProfileWindow {
            start_height: 0,
            end_height: 100,
            blocks: 101,
            elapsed_seconds: 0.25,
            blocks_per_second: 404.0,
            empty_blocks: 81,
            empty_block_import_seconds: 0.1,
            empty_blocks_per_second: 810.0,
            transaction_blocks: 20,
            transactions: 45,
            transaction_block_import_seconds: 0.125,
            transaction_blocks_per_second: 160.0,
            finalization_seconds: 0.02,
            finalization_commit_handlers_seconds: 0.005,
            finalization_canonical_commit_seconds: 0.015,
            hot_metrics: chain_acc::ImportHotMetrics::default(),
            state_service_mpt,
            mdbx_commit: Default::default(),
        });
    let report = FastSyncReport::from_parts(
        &package,
        Path::new("/cache/chain.0.acc.zip"),
        Path::new("/cache/chain.0.acc/chain.0.acc"),
        import,
        None,
    );

    assert_eq!(report.import.imported_blocks, 101);
    assert_eq!(report.import.empty_blocks, 81);
    assert_eq!(report.import.transaction_blocks, 20);
    assert_eq!(report.import.transactions, 45);
    assert_eq!(report.import.transaction_block_import_seconds, 0.25);
    assert_eq!(report.import.transaction_blocks_per_second, 80.0);
    assert_eq!(
        report.import.throughput_status,
        FastSyncThroughputStatus::BelowTarget
    );
    assert_eq!(report.import.profile_windows.len(), 1);
    let window = &report.import.profile_windows[0];
    assert_eq!((window.start_height, window.end_height), (0, 100));
    assert_eq!(window.transaction_blocks_per_second, 160.0);
    assert_eq!(window.finalization_canonical_commit_seconds, 0.015);
    assert_eq!(window.state_service_mpt.apply_attempts, 101);
    assert_eq!(window.state_service_mpt.stages[0].total_us, 30_300);
    assert_eq!(window.state_service_mpt.counts[0].avg, 101);

    let payload = serde_json::to_value(&report).expect("serialize profile report");
    assert_eq!(
        payload["import"]["profile_windows"][0]["state_service_mpt"]["stages"][0]["stage"],
        "backing_commit"
    );
    assert_eq!(
        payload["import"]["profile_windows"][0]["state_service_mpt"]["counts"][0]["total"],
        101
    );
}
