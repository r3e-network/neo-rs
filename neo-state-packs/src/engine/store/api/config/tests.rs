use super::*;

#[test]
fn default_config_is_complete_bounded_and_accelerator_free() {
    let config = PackStoreConfig::default();

    config.validate().expect("default config must stay valid");
    assert_eq!(
        config.max_frame_rows(),
        PackStoreConfig::HARD_MAX_FRAME_ROWS
    );
    assert_eq!(
        config.max_frame_payload_bytes(),
        PackStoreConfig::HARD_MAX_FRAME_PAYLOAD_BYTES
    );
    assert!(config.target_segment_bytes() <= config.max_segment_bytes());
    assert!(config.max_recent_runs() <= PackStoreConfig::HARD_MAX_RECENT_RUNS);
    assert!(config.max_index_levels() <= PackStoreConfig::HARD_MAX_INDEX_LEVELS);
    assert!(config.max_index_memory_bytes() > 0);
    assert!(config.max_pending_bytes() > 0);
    assert!(config.max_compaction_debt_runs() > 0);
    assert_eq!(config.level_zero_run_bound(), 8);
    assert_eq!(config.compacted_level_run_bound(), 8);
    assert_eq!(config.compaction_fanout(), 16);
    assert_eq!(config.read_options(), PackStoreOptions::default());
}

#[test]
fn consuming_setters_produce_one_copyable_validated_value() {
    let config = PackStoreConfig::default()
        .with_max_frame_rows(100_000)
        .expect("frame rows")
        .with_max_frame_payload_bytes(256 * 1024 * 1024)
        .expect("frame bytes")
        .with_segment_limits(1024 * 1024 * 1024, 2 * 1024 * 1024 * 1024)
        .expect("segment limits")
        .with_max_recent_runs(32)
        .expect("recent runs")
        .with_max_index_levels(16)
        .expect("index levels")
        .with_max_index_memory_bytes(128 * 1024 * 1024)
        .expect("index memory")
        .with_max_pending_bytes(512 * 1024 * 1024)
        .expect("pending bytes")
        .with_max_compaction_debt_runs(8)
        .expect("compaction debt")
        .with_compaction_bounds(4, 6, 8)
        .expect("compaction bounds")
        .with_read_options(PackStoreOptions {
            random_point_mmap: true,
            batch_value_workers: 4,
        })
        .expect("read options");
    let copied = config;

    assert_eq!(copied, config);
    assert_eq!(config.max_frame_rows(), 100_000);
    assert_eq!(config.max_frame_payload_bytes(), 256 * 1024 * 1024);
    assert_eq!(config.target_segment_bytes(), 1024 * 1024 * 1024);
    assert_eq!(config.max_segment_bytes(), 2 * 1024 * 1024 * 1024);
    assert_eq!(config.max_recent_runs(), 32);
    assert_eq!(config.max_index_levels(), 16);
    assert_eq!(config.max_index_memory_bytes(), 128 * 1024 * 1024);
    assert_eq!(config.max_pending_bytes(), 512 * 1024 * 1024);
    assert_eq!(config.max_compaction_debt_runs(), 8);
    assert_eq!(config.level_zero_run_bound(), 4);
    assert_eq!(config.compacted_level_run_bound(), 6);
    assert_eq!(config.compaction_fanout(), 8);
    assert_eq!(config.read_options().batch_value_workers, 4);
}

#[test]
fn scalar_bounds_return_typed_field_errors() {
    let cases = [
        (
            PackStoreConfig::default().with_max_frame_rows(0),
            PackStoreConfigField::MaxFrameRows,
        ),
        (
            PackStoreConfig::default()
                .with_max_frame_payload_bytes(PackStoreConfig::HARD_MAX_FRAME_PAYLOAD_BYTES + 1),
            PackStoreConfigField::MaxFramePayloadBytes,
        ),
        (
            PackStoreConfig::default().with_max_recent_runs(0),
            PackStoreConfigField::MaxRecentRuns,
        ),
        (
            PackStoreConfig::default().with_max_index_levels(1),
            PackStoreConfigField::MaxIndexLevels,
        ),
        (
            PackStoreConfig::default().with_max_index_memory_bytes(0),
            PackStoreConfigField::MaxIndexMemoryBytes,
        ),
        (
            PackStoreConfig::default().with_max_pending_bytes(0),
            PackStoreConfigField::MaxPendingBytes,
        ),
        (
            PackStoreConfig::default().with_max_compaction_debt_runs(0),
            PackStoreConfigField::MaxCompactionDebtRuns,
        ),
    ];

    for (result, expected_field) in cases {
        assert!(matches!(
            result,
            Err(PackStoreConfigError::ValueOutOfRange { field, .. })
                if field == expected_field
        ));
    }
}

#[test]
fn segment_and_engine_bounds_fail_before_store_io() {
    let segment_error = PackStoreConfig::default()
        .with_segment_limits(2 * 1024, 1024)
        .expect_err("target above maximum must fail");
    assert_eq!(
        segment_error,
        PackStoreConfigError::SegmentTargetExceedsMaximum {
            target_bytes: 2 * 1024,
            maximum_bytes: 1024,
        }
    );

    for (result, field) in [
        (
            PackStoreConfig::default().with_compaction_bounds(0, 8, 16),
            PackStoreConfigField::LevelZeroRunBound,
        ),
        (
            PackStoreConfig::default().with_compaction_bounds(8, 0, 16),
            PackStoreConfigField::CompactedLevelRunBound,
        ),
        (
            PackStoreConfig::default().with_compaction_bounds(8, 8, 1),
            PackStoreConfigField::CompactionFanout,
        ),
        (
            PackStoreConfig::default().with_read_options(PackStoreOptions {
                random_point_mmap: false,
                batch_value_workers: PackStoreConfig::HARD_MAX_BATCH_VALUE_WORKERS + 1,
            }),
            PackStoreConfigField::BatchValueWorkers,
        ),
    ] {
        assert!(matches!(
            result,
            Err(PackStoreConfigError::ValueOutOfRange {
                field: actual,
                ..
            }) if actual == field
        ));
    }
}
