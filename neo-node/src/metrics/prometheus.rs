use std::fmt::Display;
use std::fmt::Write as _;

use neo_system::ExecutionMode;

use super::acceleration::{AccelerationSnapshot, acceleration_snapshot};
use super::enums::{FastPathOutcome, MptStage, NativeHookPhase, RejectedInsert};
use super::telemetry;

impl AccelerationSnapshot {
    /// Renders the acceleration families as Prometheus exposition text.
    pub fn export_prometheus(&self) -> String {
        let mut out = String::with_capacity(4 * 1024);
        self.write_prometheus(&mut out);
        out
    }

    fn write_prometheus(&self, out: &mut String) {
        self.write_fast_path(out);
        self.write_parallel(out);
        self.write_read_cache(out);
        self.write_write_buffer(out);
        self.write_state_root(out);
        self.write_mpt(out);
        self.write_native_hooks(out);
    }

    /// Renders the speculative scheduler families.
    ///
    /// All values are counters the scheduler maintained while persisting blocks;
    /// they describe work already decided and never feed back into execution.
    fn write_parallel(&self, out: &mut String) {
        header(
            out,
            "neo_parallel_speculative_blocks_total",
            "counter",
            "Blocks whose transactions were executed speculatively before validation",
        );
        sample(
            out,
            "neo_parallel_speculative_blocks_total",
            "",
            self.parallel.speculative_blocks,
        );

        header(
            out,
            "neo_parallel_speculative_transactions_total",
            "counter",
            "Transactions executed speculatively, including runs later discarded",
        );
        sample(
            out,
            "neo_parallel_speculative_transactions_total",
            "",
            self.parallel.speculative_transactions,
        );

        header(
            out,
            "neo_parallel_conflicts_total",
            "counter",
            "Speculative runs rejected by ordered validation",
        );
        sample(
            out,
            "neo_parallel_conflicts_total",
            "",
            self.parallel.conflicts,
        );

        header(
            out,
            "neo_parallel_reexecutions_total",
            "counter",
            "Transactions re-executed serially after a detected conflict",
        );
        sample(
            out,
            "neo_parallel_reexecutions_total",
            "",
            self.parallel.reexecutions,
        );

        header(
            out,
            "neo_parallel_serial_fallbacks_total",
            "counter",
            "Blocks routed to the serial loop while parallel execution was enabled",
        );
        sample(
            out,
            "neo_parallel_serial_fallbacks_total",
            "",
            self.parallel.serial_fallbacks,
        );

        header(
            out,
            "neo_parallel_seconds_total",
            "counter",
            "Wall-clock seconds spent in the speculative scheduler by phase",
        );
        sample(
            out,
            "neo_parallel_seconds_total",
            "{phase=\"speculative\"}",
            nanos_to_seconds(self.parallel.parallel_nanos),
        );
        sample(
            out,
            "neo_parallel_seconds_total",
            "{phase=\"serial\"}",
            nanos_to_seconds(self.parallel.serial_nanos),
        );
    }

    fn write_fast_path(&self, out: &mut String) {
        header(
            out,
            "neo_fast_path_direct_attempts_total",
            "counter",
            "Direct native NEP-17 dispatches that ran, including faulted ones",
        );
        sample(
            out,
            "neo_fast_path_direct_attempts_total",
            "",
            self.fast_path.direct_attempts,
        );

        header(
            out,
            "neo_fast_path_direct_outcomes_total",
            "counter",
            "Direct native NEP-17 dispatches by final VM state",
        );
        for outcome in FastPathOutcome::ALL {
            sample(
                out,
                "neo_fast_path_direct_outcomes_total",
                &format!("{{outcome=\"{}\"}}", outcome.label()),
                self.direct_outcome(outcome),
            );
        }

        header(
            out,
            "neo_fast_path_eligibility_declines_total",
            "counter",
            "Parsed transfers rejected by the engine-side eligibility checks",
        );
        sample(
            out,
            "neo_fast_path_eligibility_declines_total",
            "",
            self.fast_path.eligibility_declines,
        );

        header(
            out,
            "neo_fast_path_canonical_fallbacks_total",
            "counter",
            "Verify-mode executions that fell back to the canonical interpreter",
        );
        sample(
            out,
            "neo_fast_path_canonical_fallbacks_total",
            "",
            self.fast_path.canonical_fallbacks,
        );

        header(
            out,
            "neo_fast_path_verification_mismatches_total",
            "counter",
            "Dual executions whose direct and canonical fingerprints diverged",
        );
        sample(
            out,
            "neo_fast_path_verification_mismatches_total",
            "",
            self.fast_path.verification_mismatches,
        );

        header(
            out,
            "neo_fast_path_mode",
            "gauge",
            "Selected execution mode, one-hot: 1 for the active mode",
        );
        let active = self.execution_mode.as_str();
        for mode in ExecutionMode::ALL {
            let value = u8::from(mode.as_str() == active);
            sample(
                out,
                "neo_fast_path_mode",
                &format!("{{mode=\"{}\"}}", mode.as_str()),
                value,
            );
        }
    }

    fn write_read_cache(&self, out: &mut String) {
        header(
            out,
            "neo_storage_acceleration_enabled",
            "gauge",
            "Whether the active store runs the named acceleration component and it is producing telemetry",
        );
        sample(
            out,
            "neo_storage_acceleration_enabled",
            "{component=\"read_cache\"}",
            u8::from(self.read_cache.is_some()),
        );
        sample(
            out,
            "neo_storage_acceleration_enabled",
            "{component=\"write_buffer\"}",
            u8::from(self.write_buffer.is_some()),
        );

        let cache = self.read_cache_or_default();

        header(
            out,
            "neo_storage_read_cache_hits_total",
            "counter",
            "Read cache lookups served from resident entries",
        );
        sample(out, "neo_storage_read_cache_hits_total", "", cache.hits);

        header(
            out,
            "neo_storage_read_cache_misses_total",
            "counter",
            "Read cache lookups that found no resident entry",
        );
        sample(out, "neo_storage_read_cache_misses_total", "", cache.misses);

        header(
            out,
            "neo_storage_read_cache_evictions_total",
            "counter",
            "Read cache entries removed by LRU or TTL expiry",
        );
        sample(
            out,
            "neo_storage_read_cache_evictions_total",
            "",
            cache.evictions,
        );

        header(
            out,
            "neo_storage_read_cache_admissions_total",
            "counter",
            "Read cache entries accepted by the admission checks",
        );
        sample(
            out,
            "neo_storage_read_cache_admissions_total",
            "",
            cache.inserts,
        );

        header(
            out,
            "neo_storage_read_cache_rejected_total",
            "counter",
            "Read cache inserts rejected by the admission checks",
        );
        for reason in RejectedInsert::ALL {
            sample(
                out,
                "neo_storage_read_cache_rejected_total",
                &format!("{{reason=\"{}\"}}", reason.label()),
                self.rejected_insert(reason),
            );
        }

        header(
            out,
            "neo_storage_read_cache_prefetches_total",
            "counter",
            "Speculative values offered to the read cache by prefetch",
        );
        sample(
            out,
            "neo_storage_read_cache_prefetches_total",
            "",
            cache.prefetches,
        );

        header(
            out,
            "neo_storage_read_cache_prefetch_hits_total",
            "counter",
            "Read cache hits served by a prefetched entry",
        );
        sample(
            out,
            "neo_storage_read_cache_prefetch_hits_total",
            "",
            cache.prefetch_hits,
        );

        header(
            out,
            "neo_storage_read_cache_prefetch_stale_skips_total",
            "counter",
            "Prefetched values dropped because a fresher entry was resident",
        );
        sample(
            out,
            "neo_storage_read_cache_prefetch_stale_skips_total",
            "",
            cache.prefetch_stale_skips,
        );

        header(
            out,
            "neo_storage_read_cache_bloom_checks_total",
            "counter",
            "Read cache lookups that probed the bloom filter",
        );
        sample(
            out,
            "neo_storage_read_cache_bloom_checks_total",
            "",
            cache.bloom_filter_checks,
        );

        header(
            out,
            "neo_storage_read_cache_bloom_negatives_total",
            "counter",
            "Bloom filter probes that proved the key absent without a lookup",
        );
        sample(
            out,
            "neo_storage_read_cache_bloom_negatives_total",
            "",
            cache.bloom_filter_negatives,
        );

        header(
            out,
            "neo_storage_read_cache_entries",
            "gauge",
            "Resident read cache entries",
        );
        sample(
            out,
            "neo_storage_read_cache_entries",
            "",
            cache.current_entries,
        );

        header(
            out,
            "neo_storage_read_cache_bytes",
            "gauge",
            "Resident read cache payload bytes",
        );
        sample(out, "neo_storage_read_cache_bytes", "", cache.current_bytes);
    }

    fn write_write_buffer(&self, out: &mut String) {
        let buffer = self.write_buffer_or_default();

        header(
            out,
            "neo_storage_write_buffer_flushes_total",
            "counter",
            "Write-batch buffer flushes completed",
        );
        sample(
            out,
            "neo_storage_write_buffer_flushes_total",
            "",
            buffer.batches_flushed,
        );

        header(
            out,
            "neo_storage_write_buffer_operations_written_total",
            "counter",
            "Store operations written by buffer flushes",
        );
        sample(
            out,
            "neo_storage_write_buffer_operations_written_total",
            "",
            buffer.operations_written,
        );

        header(
            out,
            "neo_storage_write_buffer_bytes_written_total",
            "counter",
            "Bytes written by buffer flushes",
        );
        sample(
            out,
            "neo_storage_write_buffer_bytes_written_total",
            "",
            buffer.bytes_written,
        );

        header(
            out,
            "neo_storage_write_buffer_flush_seconds_total",
            "counter",
            "Cumulative buffer flush wall-clock time in seconds",
        );
        sample(
            out,
            "neo_storage_write_buffer_flush_seconds_total",
            "",
            buffer.total_flush_duration_ms as f64 / 1000.0,
        );

        header(
            out,
            "neo_storage_write_buffer_flush_timeouts_total",
            "counter",
            "Buffer flushes triggered by the delay timeout",
        );
        sample(
            out,
            "neo_storage_write_buffer_flush_timeouts_total",
            "",
            buffer.flush_timeouts,
        );

        header(
            out,
            "neo_storage_write_buffer_pending_operations",
            "gauge",
            "Operations buffered and not yet flushed",
        );
        sample(
            out,
            "neo_storage_write_buffer_pending_operations",
            "",
            buffer.pending_operations,
        );

        header(
            out,
            "neo_storage_write_buffer_pending_bytes",
            "gauge",
            "Payload bytes buffered and not yet flushed",
        );
        sample(
            out,
            "neo_storage_write_buffer_pending_bytes",
            "",
            buffer.pending_bytes,
        );
    }

    fn write_state_root(&self, out: &mut String) {
        let cache = self.state_root_node_cache.unwrap_or_default();

        header(
            out,
            "neo_state_root_node_cache_enabled",
            "gauge",
            "Whether the state-root node cache is active",
        );
        sample(
            out,
            "neo_state_root_node_cache_enabled",
            "",
            u8::from(self.state_root_enabled()),
        );

        header(
            out,
            "neo_state_root_node_cache_hits_total",
            "counter",
            "State-root MPT node lookups served from the shared node cache",
        );
        sample(out, "neo_state_root_node_cache_hits_total", "", cache.hits);

        header(
            out,
            "neo_state_root_node_cache_misses_total",
            "counter",
            "State-root MPT node lookups that missed the shared node cache",
        );
        sample(
            out,
            "neo_state_root_node_cache_misses_total",
            "",
            cache.misses,
        );

        header(
            out,
            "neo_state_root_node_cache_entries",
            "gauge",
            "Resident state-root node cache entries",
        );
        sample(out, "neo_state_root_node_cache_entries", "", cache.entries);

        header(
            out,
            "neo_state_root_node_cache_capacity",
            "gauge",
            "Configured state-root node cache capacity in entries",
        );
        sample(
            out,
            "neo_state_root_node_cache_capacity",
            "",
            cache.capacity,
        );

        header(
            out,
            "neo_state_root_warm_root_nodes",
            "gauge",
            "Resolved nodes retained for the warm state root, excluding hash stubs",
        );
        sample(
            out,
            "neo_state_root_warm_root_nodes",
            "",
            cache.warm_root_nodes,
        );

        header(
            out,
            "neo_state_root_warm_root_bytes",
            "gauge",
            "Estimated bytes retained for the warm state root",
        );
        sample(
            out,
            "neo_state_root_warm_root_bytes",
            "",
            cache.warm_root_estimated_bytes,
        );
    }

    fn write_mpt(&self, out: &mut String) {
        header(
            out,
            "neo_execution_profiling_enabled",
            "gauge",
            "Whether persistence profiling is accumulating MPT timings",
        );
        sample(
            out,
            "neo_execution_profiling_enabled",
            "",
            u8::from(self.profiling_enabled),
        );

        header(
            out,
            "neo_mpt_stage_seconds_total",
            "counter",
            "Cumulative MPT persistence stage wall-clock time in seconds",
        );
        for stage in MptStage::ALL {
            sample(
                out,
                "neo_mpt_stage_seconds_total",
                &format!("{{stage=\"{}\"}}", stage.label()),
                self.mpt_stage_seconds(stage),
            );
        }
    }

    fn write_native_hooks(&self, out: &mut String) {
        header(
            out,
            "neo_native_hook_seconds_total",
            "counter",
            "Cumulative native OnPersist and PostPersist wall-clock time in seconds",
        );
        for phase in NativeHookPhase::ALL {
            sample(
                out,
                "neo_native_hook_seconds_total",
                &format!("{{phase=\"{}\"}}", phase.label()),
                self.native_hook_seconds(phase),
            );
        }

        header(
            out,
            "neo_native_hook_empty_block_seconds_total",
            "counter",
            "The empty-block subset of neo_native_hook_seconds_total",
        );
        for phase in NativeHookPhase::ALL {
            sample(
                out,
                "neo_native_hook_empty_block_seconds_total",
                &format!("{{phase=\"{}\"}}", phase.label()),
                self.native_hook_empty_block_seconds(phase),
            );
        }
    }
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn sample(out: &mut String, name: &str, labels: &str, value: impl Display) {
    let _ = writeln!(out, "{name}{labels} {value}");
}

/// Converts accumulated nanoseconds into the seconds a Prometheus counter expects.
fn nanos_to_seconds(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000_000.0
}

/// Gathers Prometheus metrics
pub fn gather_prometheus() -> Vec<u8> {
    let mut text = telemetry().export_prometheus();
    text.push_str(&acceleration_snapshot().export_prometheus());
    text.into_bytes()
}
