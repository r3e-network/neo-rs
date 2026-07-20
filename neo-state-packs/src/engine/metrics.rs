//! Low-overhead, fixed-cardinality pack-store counters.
//!
//! The counters are shared by the live store and its pinned snapshots.  They
//! intentionally contain no dynamic labels: callers get one aggregate view
//! that is cheap enough to keep enabled while profiling a replay.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::PackStageTotals;

/// Aggregate read counters shared by a store and all snapshots it creates.
#[derive(Debug, Default)]
pub(super) struct ReadCounters {
    point_reads: AtomicU64,
    point_hits: AtomicU64,
    point_misses: AtomicU64,
    sorted_batches: AtomicU64,
    sorted_keys: AtomicU64,
    sorted_hits: AtomicU64,
    sorted_value_bytes: AtomicU64,
}

impl ReadCounters {
    pub(super) fn record_point(&self, hit: bool) {
        self.point_reads.fetch_add(1, Ordering::Relaxed);
        if hit {
            self.point_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.point_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn record_sorted(&self, keys: usize, results: &[Option<Vec<u8>>]) {
        self.sorted_batches.fetch_add(1, Ordering::Relaxed);
        self.sorted_keys
            .fetch_add(u64::try_from(keys).unwrap_or(u64::MAX), Ordering::Relaxed);
        let hits = results.iter().filter(|value| value.is_some()).count();
        self.sorted_hits
            .fetch_add(u64::try_from(hits).unwrap_or(u64::MAX), Ordering::Relaxed);
        let value_bytes = results
            .iter()
            .filter_map(Option::as_ref)
            .fold(0u64, |total, value| {
                total.saturating_add(u64::try_from(value.len()).unwrap_or(u64::MAX))
            });
        self.sorted_value_bytes
            .fetch_add(value_bytes, Ordering::Relaxed);
    }

    pub(super) fn snapshot(&self) -> ReadMetrics {
        ReadMetrics {
            point_reads: self.point_reads.load(Ordering::Relaxed),
            point_hits: self.point_hits.load(Ordering::Relaxed),
            point_misses: self.point_misses.load(Ordering::Relaxed),
            sorted_batches: self.sorted_batches.load(Ordering::Relaxed),
            sorted_keys: self.sorted_keys.load(Ordering::Relaxed),
            sorted_hits: self.sorted_hits.load(Ordering::Relaxed),
            sorted_value_bytes: self.sorted_value_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Read-side counters included in [`PackMetrics`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadMetrics {
    /// Number of point reads issued through the store or a pinned snapshot.
    pub point_reads: u64,
    /// Point reads that returned a value.
    pub point_hits: u64,
    /// Point reads that returned no value.
    pub point_misses: u64,
    /// Number of sorted batches issued.
    pub sorted_batches: u64,
    /// Keys supplied to sorted batches.
    pub sorted_keys: u64,
    /// Sorted-batch keys that returned a value.
    pub sorted_hits: u64,
    /// Value bytes copied by sorted batches.
    pub sorted_value_bytes: u64,
}

/// Current derived-index compaction debt.
///
/// The soft bound schedules background maintenance. The hard bound is one
/// additional fanout beyond it; callers must apply backpressure there rather
/// than allowing an unbounded run queue to accumulate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompactionDebt {
    /// Immutable runs referenced by the current manifest.
    pub live_runs: u64,
    /// Runs above their per-level soft bounds.
    pub excess_runs: u64,
    /// Resident filter/fence metadata charged to the configured memory bound.
    pub decoded_index_bytes: u64,
    /// Configured resident metadata hard bound.
    pub max_index_memory_bytes: u64,
    /// A level reached its bounded producer-stall threshold.
    pub backpressure_required: bool,
}

/// Cumulative compaction and reclamation evidence for one pack store.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompactionStats {
    /// Completed compaction cycles (one merge plus manifest publication each).
    pub cycles: u64,
    /// Input runs consumed across all cycles.
    pub runs_merged: u64,
    /// Output runs produced across all cycles.
    pub runs_produced: u64,
    /// Index records read from compaction inputs.
    pub input_records: u64,
    /// Index records written after newest-epoch-wins dedup.
    pub output_records: u64,
    /// Bytes of compacted run files written.
    pub bytes_written: u64,
    /// Wall time spent building and adopting compacted runs.
    pub wall_ns: u64,
    /// High-water mark of concurrently live runs.
    pub peak_live_runs: u64,
    /// Explicit reclamation passes executed.
    pub gc_cycles: u64,
    /// Superseded run files deleted by reclamation.
    pub gc_runs_deleted: u64,
    /// Superseded manifests deleted by reclamation.
    pub gc_manifests_deleted: u64,
    /// Bytes reclaimed by deletion.
    pub gc_bytes_reclaimed: u64,
}

/// Fixed-cardinality cumulative evidence for one live pack-store handle.
///
/// Stage timings cover successful appends after this handle was opened;
/// read counters include the handle and every snapshot derived from it.
/// Values are aggregate rather than per-key or per-contract so this snapshot
/// can be exported without creating unbounded telemetry labels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackMetrics {
    /// Successful append-stage totals since open.
    pub append: PackStageTotals,
    /// Shared point and sorted-batch read counters.
    pub reads: ReadMetrics,
    /// Cumulative derived-index compaction and reclamation evidence.
    pub compaction: CompactionStats,
    /// Number of frames replayed while rebuilding derived indexes after open.
    pub rebuild_frames: u64,
    /// Number of immutable runs emitted by recovery rebuild.
    pub rebuild_runs: u64,
    /// Index entries decoded during recovery rebuild.
    pub rebuild_index_entries: u64,
    /// Wall time spent rebuilding derived indexes during open.
    pub rebuild_wall_ns: u64,
    /// Logical frame payload bytes written since open.
    pub logical_payload_bytes: u64,
    /// Current physical append-pack bytes.
    pub physical_pack_bytes: u64,
    /// Current physical live-index bytes.
    pub physical_index_bytes: u64,
    /// Current live immutable run count.
    pub live_runs: u64,
    /// Current resident index metadata bytes.
    pub decoded_index_memory_bytes: u64,
    /// Current soft/hard compaction-debt view.
    pub debt: CompactionDebt,
}

impl PackMetrics {
    /// Physical live-layout bytes per logical frame-payload byte, scaled by
    /// 1,000 for integer telemetry. `None` means no payload was written by
    /// this handle, so a ratio would be undefined.
    #[must_use]
    pub fn physical_layout_amplification_milli(self) -> Option<u64> {
        if self.logical_payload_bytes == 0 {
            return None;
        }
        self.physical_pack_bytes
            .saturating_add(self.physical_index_bytes)
            .saturating_mul(1_000)
            .checked_div(self.logical_payload_bytes)
    }
}

/// One explicit reclamation pass over superseded runs and manifests.
#[derive(Clone, Copy, Debug, Default)]
pub struct GcStats {
    /// Superseded run files deleted in this pass.
    pub runs_deleted: u64,
    /// Superseded manifests deleted in this pass.
    pub manifests_deleted: u64,
    /// Bytes reclaimed by deletion in this pass.
    pub bytes_reclaimed: u64,
}
