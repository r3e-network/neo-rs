use super::*;

use super::acceleration::*;
use super::enums::*;

use neo_storage::persistence::read_cache::ReadCacheStatsSnapshot;
use neo_storage::persistence::store::StoreAccelerationStats;
use neo_storage::persistence::write_batch_buffer::WriteBatchStatsSnapshot;
use neo_system::state_service::StateRootNodeCacheStats;
use neo_system::{
    ExecutionMode, ExecutionProfileSnapshot, FastPathStatsSnapshot, ParallelExecutionSnapshot,
};
use std::time::Duration;

/// Every label key the acceleration surface is allowed to emit.
const LABEL_SCHEMA: &[(&str, &[&str])] = &[
    ("outcome", &["halt", "fault"]),
    ("reason", &["oversized", "budget"]),
    ("mode", &["canonical", "auto", "verify"]),
    ("component", &["read_cache", "write_buffer"]),
    (
        "phase",
        &["on_persist", "post_persist", "speculative", "serial"],
    ),
    (
        "stage",
        &[
            "update",
            "commit",
            "root_hash",
            "hash",
            "serialization",
            "hash256",
            "node_cache",
            "node_read",
            "node_deserialize",
            "node_encode",
            "node_write",
        ],
    ),
];

fn populated_snapshot() -> AccelerationSnapshot {
    let fast_path = FastPathStatsSnapshot {
        direct_attempts: 11,
        direct_halts: 9,
        direct_faults: 2,
        eligibility_declines: 4,
        canonical_fallbacks: 3,
        verification_mismatches: 1,
    };
    let mut profile = ExecutionProfileSnapshot {
        enabled: true,
        mpt_update: Duration::from_millis(1500),
        mpt_hash: Duration::from_millis(250),
        mpt_hash256: Duration::from_micros(500),
        on_persist: Duration::from_millis(2000),
        post_persist: Duration::from_millis(500),
        ..ExecutionProfileSnapshot::default()
    };
    profile.empty_blocks.on_persist = Duration::from_millis(250);
    profile.empty_blocks.post_persist = Duration::from_millis(50);
    let acceleration = StoreAccelerationStats {
        read_cache: Some(ReadCacheStatsSnapshot {
            hits: 100,
            misses: 20,
            evictions: 7,
            prefetches: 30,
            prefetch_hits: 25,
            inserts: 90,
            current_entries: 42,
            current_bytes: 4096,
            rejected_oversized: 5,
            rejected_budget: 2,
            prefetch_stale_skips: 6,
            bloom_filter_checks: 40,
            bloom_filter_negatives: 12,
        }),
        write_buffer: Some(WriteBatchStatsSnapshot {
            batches_flushed: 8,
            operations_written: 800,
            bytes_written: 65536,
            total_flush_duration_ms: 4000,
            flush_timeouts: 1,
            pending_operations: 12,
            pending_bytes: 2048,
        }),
    };
    let state_root = StateRootNodeCacheStats {
        enabled: true,
        capacity: 65536,
        entries: 512,
        hits: 64,
        misses: 16,
        hit_rate_percent: 80.0,
        warm_root_nodes: 300,
        warm_root_estimated_bytes: 98304,
    };
    AccelerationSnapshot::capture(
        fast_path,
        ParallelExecutionSnapshot::default(),
        ExecutionMode::Auto,
        &profile,
        Some(&acceleration),
        Some(state_root),
    )
}

/// Collects `(metric_name, label_key, label_value)` for every labelled sample.
fn labelled_samples(text: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || !line.contains('{') {
            continue;
        }
        let (name, rest) = line.split_once('{').expect("labelled sample");
        let (labels, _) = rest.split_once('}').expect("closed label set");
        for pair in labels.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').expect("key=value label");
            found.push((
                name.to_string(),
                key.to_string(),
                value.trim_matches('"').to_string(),
            ));
        }
    }
    found
}

fn value_of(text: &str, sample_line: &str) -> f64 {
    text.lines()
        .find(|line| line.starts_with(sample_line))
        .unwrap_or_else(|| panic!("missing sample `{sample_line}`"))
        .rsplit_once(' ')
        .expect("sample has a value")
        .1
        .parse()
        .expect("numeric sample value")
}

#[test]
fn telemetry_is_accessible() {
    let _ = telemetry();
}

#[test]
fn update_metrics_exports_prometheus_format() {
    update_node_metrics(100, 100, 100, 50, 10, 5);
    let exported = gather_prometheus();
    let text = String::from_utf8(exported).unwrap();

    assert!(text.contains("neo_block_height"));
    assert!(text.contains("neo_mempool_verified"));
    assert!(text.contains("100"));
}

#[test]
fn storage_metrics_work() {
    update_storage_metrics(50.5, 100.0);
    let exported = gather_prometheus();
    let text = String::from_utf8(exported).unwrap();

    assert!(text.contains("neo_storage_free_gb"));
    assert!(text.contains("50."));
}

#[test]
fn mpt_stage_table_covers_every_exported_stage() {
    assert_eq!(MptStage::ALL.len(), MPT_STAGE_COUNT);
    let mut labels: Vec<&str> = MptStage::ALL.iter().map(|stage| stage.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), MPT_STAGE_COUNT, "stage labels are unique");
}

#[test]
fn native_hook_phase_table_is_bounded() {
    assert_eq!(NativeHookPhase::ALL.len(), NATIVE_HOOK_PHASE_COUNT);
    let mut labels: Vec<&str> = NativeHookPhase::ALL
        .iter()
        .map(|phase| phase.label())
        .collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(
        labels.len(),
        NATIVE_HOOK_PHASE_COUNT,
        "phase labels are unique"
    );
    let mut indexes: Vec<usize> = NativeHookPhase::ALL
        .iter()
        .map(|phase| phase.index())
        .collect();
    indexes.sort_unstable();
    assert_eq!(indexes, (0..NATIVE_HOOK_PHASE_COUNT).collect::<Vec<_>>());
}

#[test]
fn acceleration_metric_names_are_stable() {
    let text = populated_snapshot().export_prometheus();
    for name in [
        "neo_fast_path_direct_attempts_total",
        "neo_fast_path_direct_outcomes_total",
        "neo_fast_path_eligibility_declines_total",
        "neo_fast_path_canonical_fallbacks_total",
        "neo_fast_path_verification_mismatches_total",
        "neo_fast_path_mode",
        "neo_parallel_speculative_blocks_total",
        "neo_parallel_speculative_transactions_total",
        "neo_parallel_conflicts_total",
        "neo_parallel_reexecutions_total",
        "neo_parallel_serial_fallbacks_total",
        "neo_parallel_seconds_total",
        "neo_storage_acceleration_enabled",
        "neo_storage_read_cache_hits_total",
        "neo_storage_read_cache_misses_total",
        "neo_storage_read_cache_evictions_total",
        "neo_storage_read_cache_admissions_total",
        "neo_storage_read_cache_rejected_total",
        "neo_storage_read_cache_prefetches_total",
        "neo_storage_read_cache_prefetch_hits_total",
        "neo_storage_read_cache_prefetch_stale_skips_total",
        "neo_storage_read_cache_bloom_checks_total",
        "neo_storage_read_cache_bloom_negatives_total",
        "neo_storage_read_cache_entries",
        "neo_storage_read_cache_bytes",
        "neo_storage_write_buffer_flushes_total",
        "neo_storage_write_buffer_operations_written_total",
        "neo_storage_write_buffer_bytes_written_total",
        "neo_storage_write_buffer_flush_seconds_total",
        "neo_storage_write_buffer_flush_timeouts_total",
        "neo_storage_write_buffer_pending_operations",
        "neo_storage_write_buffer_pending_bytes",
        "neo_state_root_node_cache_enabled",
        "neo_state_root_node_cache_hits_total",
        "neo_state_root_node_cache_misses_total",
        "neo_state_root_node_cache_entries",
        "neo_state_root_node_cache_capacity",
        "neo_state_root_warm_root_nodes",
        "neo_state_root_warm_root_bytes",
        "neo_execution_profiling_enabled",
        "neo_mpt_stage_seconds_total",
        "neo_native_hook_seconds_total",
        "neo_native_hook_empty_block_seconds_total",
    ] {
        assert!(
            text.contains(&format!("# TYPE {name} ")),
            "missing HELP/TYPE metadata for {name}"
        );
    }
}

#[test]
fn acceleration_labels_are_bounded() {
    let text = populated_snapshot().export_prometheus();
    let samples = labelled_samples(&text);
    assert!(!samples.is_empty(), "expected labelled samples");

    for (name, key, value) in samples {
        let Some((_, allowed)) = LABEL_SCHEMA.iter().find(|(known, _)| *known == key) else {
            panic!("unbounded label key `{key}` on `{name}`");
        };
        assert!(
            allowed.contains(&value.as_str()),
            "label `{key}=\"{value}\"` on `{name}` is outside the bounded set"
        );
    }
}

#[test]
fn acceleration_families_have_no_label_collisions() {
    let text = populated_snapshot().export_prometheus();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let series = line.rsplit_once(' ').expect("sample line").0;
        assert!(seen.insert(series.to_string()), "duplicate series {series}");
    }
}

#[test]
fn snapshot_maps_counters_to_samples() {
    let text = populated_snapshot().export_prometheus();

    assert_eq!(value_of(&text, "neo_fast_path_direct_attempts_total"), 11.0);
    assert_eq!(
        value_of(
            &text,
            "neo_fast_path_direct_outcomes_total{outcome=\"halt\"}"
        ),
        9.0
    );
    assert_eq!(
        value_of(
            &text,
            "neo_fast_path_direct_outcomes_total{outcome=\"fault\"}"
        ),
        2.0
    );
    assert_eq!(
        value_of(&text, "neo_fast_path_eligibility_declines_total"),
        4.0
    );
    assert_eq!(
        value_of(&text, "neo_fast_path_canonical_fallbacks_total"),
        3.0
    );
    assert_eq!(
        value_of(&text, "neo_fast_path_verification_mismatches_total"),
        1.0
    );

    assert_eq!(value_of(&text, "neo_storage_read_cache_hits_total"), 100.0);
    assert_eq!(value_of(&text, "neo_storage_read_cache_misses_total"), 20.0);
    assert_eq!(
        value_of(&text, "neo_storage_read_cache_evictions_total"),
        7.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_read_cache_admissions_total"),
        90.0
    );
    assert_eq!(
        value_of(
            &text,
            "neo_storage_read_cache_rejected_total{reason=\"oversized\"}"
        ),
        5.0
    );
    assert_eq!(
        value_of(
            &text,
            "neo_storage_read_cache_rejected_total{reason=\"budget\"}"
        ),
        2.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_read_cache_prefetch_stale_skips_total"),
        6.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_read_cache_bloom_checks_total"),
        40.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_read_cache_bloom_negatives_total"),
        12.0
    );
    assert_eq!(value_of(&text, "neo_storage_read_cache_entries"), 42.0);
    assert_eq!(value_of(&text, "neo_storage_read_cache_bytes"), 4096.0);

    assert_eq!(
        value_of(&text, "neo_storage_write_buffer_flushes_total"),
        8.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_write_buffer_pending_operations"),
        12.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_write_buffer_pending_bytes"),
        2048.0
    );
    assert_eq!(
        value_of(&text, "neo_storage_write_buffer_bytes_written_total"),
        65536.0
    );

    assert_eq!(
        value_of(&text, "neo_state_root_node_cache_hits_total"),
        64.0
    );
    assert_eq!(
        value_of(&text, "neo_state_root_node_cache_misses_total"),
        16.0
    );
    assert_eq!(value_of(&text, "neo_state_root_warm_root_nodes"), 300.0);
    assert_eq!(value_of(&text, "neo_state_root_warm_root_bytes"), 98304.0);

    assert_eq!(
        value_of(&text, "neo_native_hook_seconds_total{phase=\"on_persist\"}"),
        2.0
    );
    assert_eq!(
        value_of(
            &text,
            "neo_native_hook_seconds_total{phase=\"post_persist\"}"
        ),
        0.5
    );
    assert_eq!(
        value_of(
            &text,
            "neo_native_hook_empty_block_seconds_total{phase=\"on_persist\"}"
        ),
        0.25
    );
    assert_eq!(
        value_of(
            &text,
            "neo_native_hook_empty_block_seconds_total{phase=\"post_persist\"}"
        ),
        0.05
    );
}

#[test]
fn snapshot_converts_durations_to_seconds() {
    let snapshot = populated_snapshot();
    assert!((snapshot.mpt_stage_seconds(MptStage::Update) - 1.5).abs() < 1e-9);
    assert!((snapshot.mpt_stage_seconds(MptStage::Hash) - 0.25).abs() < 1e-9);
    assert!((snapshot.mpt_stage_seconds(MptStage::Hash256) - 0.0005).abs() < 1e-9);
    assert_eq!(snapshot.mpt_stage_seconds(MptStage::Commit), 0.0);
    assert!((snapshot.native_hook_seconds(NativeHookPhase::OnPersist) - 2.0).abs() < 1e-9);
    assert!((snapshot.native_hook_seconds(NativeHookPhase::PostPersist) - 0.5).abs() < 1e-9);
    assert!(
        (snapshot.native_hook_empty_block_seconds(NativeHookPhase::OnPersist) - 0.25).abs() < 1e-9
    );

    let text = snapshot.export_prometheus();
    assert!((value_of(&text, "neo_mpt_stage_seconds_total{stage=\"update\"}") - 1.5).abs() < 1e-9);
    assert!((value_of(&text, "neo_storage_write_buffer_flush_seconds_total") - 4.0).abs() < 1e-9);
    assert_eq!(value_of(&text, "neo_execution_profiling_enabled"), 1.0);
}

#[test]
fn parallel_counters_export_every_family() {
    let parallel = ParallelExecutionSnapshot {
        speculative_blocks: 3,
        speculative_transactions: 11,
        conflicts: 4,
        reexecutions: 4,
        serial_fallbacks: 2,
        parallel_nanos: 1_500_000_000,
        serial_nanos: 2_000_000_000,
    };
    let snapshot = AccelerationSnapshot::capture(
        FastPathStatsSnapshot::default(),
        parallel,
        ExecutionMode::Auto,
        &ExecutionProfileSnapshot::default(),
        None,
        None,
    );
    let text = snapshot.export_prometheus();

    assert_eq!(
        value_of(&text, "neo_parallel_speculative_blocks_total"),
        3.0
    );
    assert_eq!(
        value_of(&text, "neo_parallel_speculative_transactions_total"),
        11.0
    );
    assert_eq!(value_of(&text, "neo_parallel_conflicts_total"), 4.0);
    assert_eq!(value_of(&text, "neo_parallel_reexecutions_total"), 4.0);
    assert_eq!(value_of(&text, "neo_parallel_serial_fallbacks_total"), 2.0);
    assert_eq!(
        value_of(&text, "neo_parallel_seconds_total{phase=\"speculative\"}"),
        1.5
    );
    assert_eq!(
        value_of(&text, "neo_parallel_seconds_total{phase=\"serial\"}"),
        2.0
    );
}

#[test]
fn mode_family_is_one_hot() {
    let text = populated_snapshot().export_prometheus();
    let mut active = 0.0;
    for mode in ExecutionMode::ALL {
        let value = value_of(
            &text,
            &format!("neo_fast_path_mode{{mode=\"{}\"}}", mode.as_str()),
        );
        assert!(value == 0.0 || value == 1.0, "mode gauge is 0/1");
        active += value;
    }
    assert_eq!(active, 1.0);
    assert_eq!(
        value_of(&text, "neo_fast_path_mode{mode=\"auto\"}"),
        1.0,
        "auto is the configured mode"
    );
}

#[test]
fn absent_components_report_zero_and_disabled() {
    let snapshot = AccelerationSnapshot::capture(
        FastPathStatsSnapshot::default(),
        ParallelExecutionSnapshot::default(),
        ExecutionMode::Canonical,
        &ExecutionProfileSnapshot::default(),
        None,
        None,
    );
    let text = snapshot.export_prometheus();

    assert_eq!(
        value_of(
            &text,
            "neo_storage_acceleration_enabled{component=\"read_cache\"}"
        ),
        0.0
    );
    assert_eq!(
        value_of(
            &text,
            "neo_storage_acceleration_enabled{component=\"write_buffer\"}"
        ),
        0.0
    );
    assert_eq!(value_of(&text, "neo_state_root_node_cache_enabled"), 0.0);
    assert_eq!(value_of(&text, "neo_execution_profiling_enabled"), 0.0);
    assert_eq!(value_of(&text, "neo_storage_read_cache_hits_total"), 0.0);
    assert_eq!(
        value_of(&text, "neo_storage_write_buffer_flushes_total"),
        0.0
    );
    assert_eq!(
        value_of(&text, "neo_mpt_stage_seconds_total{stage=\"hash\"}"),
        0.0
    );
    assert_eq!(
        value_of(&text, "neo_native_hook_seconds_total{phase=\"on_persist\"}"),
        0.0
    );
    assert_eq!(
        value_of(
            &text,
            "neo_native_hook_empty_block_seconds_total{phase=\"post_persist\"}"
        ),
        0.0
    );
}

#[test]
fn present_components_report_enabled() {
    let text = populated_snapshot().export_prometheus();
    for component in ["read_cache", "write_buffer"] {
        assert_eq!(
            value_of(
                &text,
                &format!("neo_storage_acceleration_enabled{{component=\"{component}\"}}")
            ),
            1.0
        );
    }
    assert_eq!(value_of(&text, "neo_state_root_node_cache_enabled"), 1.0);
}

#[test]
fn gather_prometheus_includes_acceleration_families() {
    let _guard = TEST_LOCK.lock();
    update_acceleration_metrics(populated_snapshot());
    let text = String::from_utf8(gather_prometheus()).expect("utf-8 exposition");

    assert!(text.contains("neo_fast_path_direct_attempts_total"));
    assert!(text.contains("neo_block_height"), "node families are kept");
    assert_eq!(acceleration_snapshot().fast_path.direct_attempts, 11);

    update_acceleration_metrics(AccelerationSnapshot::default());
    let cleared = String::from_utf8(gather_prometheus()).expect("utf-8 exposition");
    assert_eq!(
        value_of(&cleared, "neo_fast_path_direct_attempts_total"),
        0.0
    );
}

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
