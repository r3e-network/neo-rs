//! Low-overhead cumulative metrics for durable MDBX overlay commits.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Timing stage within one raw MDBX overlay commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdbxCommitStage {
    /// End-to-end time from entering the commit path until it returns.
    Total,
    /// Time spent obtaining the single MDBX write transaction.
    TransactionOpen,
    /// Time spent opening or creating tables in the write transaction.
    TableOpen,
    /// Time spent opening writable table cursors.
    CursorOpen,
    /// Time spent ordering an owned raw overlay before opening MDBX.
    OverlaySort,
    /// Total time spent visiting overlay sources, including source ordering.
    OverlayVisit,
    /// Estimated time spent inside MDBX cursor put/delete operations. Dense
    /// overlays use systematic samples after a small exact prefix.
    CursorWrite,
    /// Time spent resolving cursor-driven overlay entries (deferred full-state
    /// MPT reference resolution) through read-modify-write at the write cursor.
    CursorResolve,
    /// Time spent committing the MDBX transaction with durable sync semantics.
    Commit,
}

impl MdbxCommitStage {
    fn label(self) -> &'static str {
        match self {
            Self::Total => "total",
            Self::TransactionOpen => "transaction_open",
            Self::TableOpen => "table_open",
            Self::CursorOpen => "cursor_open",
            Self::OverlaySort => "overlay_sort",
            Self::OverlayVisit => "overlay_visit",
            Self::CursorWrite => "cursor_write",
            Self::CursorResolve => "cursor_resolve",
            Self::Commit => "commit",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::Total => 0,
            Self::TransactionOpen => 1,
            Self::TableOpen => 2,
            Self::CursorOpen => 3,
            Self::OverlaySort => 4,
            Self::OverlayVisit => 5,
            Self::CursorWrite => 6,
            Self::CursorResolve => 7,
            Self::Commit => 8,
        }
    }
}

/// Volume counted while raw overlays are written to MDBX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdbxCommitCountKind {
    /// Tables written across commit attempts.
    Tables,
    /// Put and delete entries visited across commit attempts.
    Entries,
    /// Put entries visited across commit attempts.
    Puts,
    /// Delete entries visited across commit attempts.
    Deletes,
    /// Key bytes supplied to cursor operations.
    KeyBytes,
    /// Value bytes supplied to cursor put operations.
    ValueBytes,
    /// Cursor-resolved keys already present in the authoritative table.
    CursorResolvePresent,
    /// Cursor-resolved keys absent from the authoritative table.
    CursorResolveAbsent,
    /// Process-attributed physical read bytes during non-empty cursor resolution.
    CursorResolveReadBytes,
    /// Process-attributed physical write bytes during non-empty cursor resolution.
    CursorResolveWriteBytes,
    /// Process minor faults observed during non-empty cursor resolution.
    CursorResolveMinorFaults,
    /// Process major faults observed during non-empty cursor resolution.
    CursorResolveMajorFaults,
    /// Put values from zero through 64 bytes.
    ValueSize0To64,
    /// Put values from 65 through 128 bytes.
    ValueSize65To128,
    /// Put values from 129 through 256 bytes.
    ValueSize129To256,
    /// Put values from 257 through 512 bytes.
    ValueSize257To512,
    /// Put values from 513 through 1,024 bytes.
    ValueSize513To1024,
    /// Put values from 1,025 through 4,096 bytes.
    ValueSize1025To4096,
    /// Put values from 4,097 through 16,384 bytes.
    ValueSize4097To16384,
    /// Put values larger than 16,384 bytes.
    ValueSizeOver16384,
}

impl MdbxCommitCountKind {
    fn label(self) -> &'static str {
        match self {
            Self::Tables => "tables",
            Self::Entries => "entries",
            Self::Puts => "puts",
            Self::Deletes => "deletes",
            Self::KeyBytes => "key_bytes",
            Self::ValueBytes => "value_bytes",
            Self::CursorResolvePresent => "cursor_resolve_present",
            Self::CursorResolveAbsent => "cursor_resolve_absent",
            Self::CursorResolveReadBytes => "cursor_resolve_read_bytes",
            Self::CursorResolveWriteBytes => "cursor_resolve_write_bytes",
            Self::CursorResolveMinorFaults => "cursor_resolve_minor_faults",
            Self::CursorResolveMajorFaults => "cursor_resolve_major_faults",
            Self::ValueSize0To64 => "value_size_0_64",
            Self::ValueSize65To128 => "value_size_65_128",
            Self::ValueSize129To256 => "value_size_129_256",
            Self::ValueSize257To512 => "value_size_257_512",
            Self::ValueSize513To1024 => "value_size_513_1024",
            Self::ValueSize1025To4096 => "value_size_1025_4096",
            Self::ValueSize4097To16384 => "value_size_4097_16384",
            Self::ValueSizeOver16384 => "value_size_over_16384",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::Tables => 0,
            Self::Entries => 1,
            Self::Puts => 2,
            Self::Deletes => 3,
            Self::KeyBytes => 4,
            Self::ValueBytes => 5,
            Self::ValueSize0To64 => 6,
            Self::ValueSize65To128 => 7,
            Self::ValueSize129To256 => 8,
            Self::ValueSize257To512 => 9,
            Self::ValueSize513To1024 => 10,
            Self::ValueSize1025To4096 => 11,
            Self::ValueSize4097To16384 => 12,
            Self::ValueSizeOver16384 => 13,
            Self::CursorResolvePresent => 14,
            Self::CursorResolveAbsent => 15,
            Self::CursorResolveReadBytes => 16,
            Self::CursorResolveWriteBytes => 17,
            Self::CursorResolveMinorFaults => 18,
            Self::CursorResolveMajorFaults => 19,
        }
    }
}

pub(super) const VALUE_SIZE_COUNT_KINDS: [MdbxCommitCountKind; 8] = [
    MdbxCommitCountKind::ValueSize0To64,
    MdbxCommitCountKind::ValueSize65To128,
    MdbxCommitCountKind::ValueSize129To256,
    MdbxCommitCountKind::ValueSize257To512,
    MdbxCommitCountKind::ValueSize513To1024,
    MdbxCommitCountKind::ValueSize1025To4096,
    MdbxCommitCountKind::ValueSize4097To16384,
    MdbxCommitCountKind::ValueSizeOver16384,
];

pub(super) const fn value_size_bucket_index(value_len: usize) -> usize {
    match value_len {
        0..=64 => 0,
        65..=128 => 1,
        129..=256 => 2,
        257..=512 => 3,
        513..=1_024 => 4,
        1_025..=4_096 => 5,
        4_097..=16_384 => 6,
        _ => 7,
    }
}

/// Cumulative outcome counters for MDBX raw-overlay commit attempts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MdbxCommitStats {
    /// Commit paths entered, including successful empty overlays.
    pub attempts: u64,
    /// Commit paths that returned an error or unwound.
    pub failures: u64,
    /// MDBX write transactions committed successfully.
    pub committed_transactions: u64,
}

/// Snapshot of one MDBX commit timing series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdbxCommitStageStats {
    /// Stable stage label used by logs and Prometheus output.
    pub stage: &'static str,
    /// Number of stage observations.
    pub calls: u64,
    /// Cumulative duration in microseconds.
    pub total_us: u64,
    /// Arithmetic mean duration in microseconds.
    pub avg_us: u64,
}

/// Snapshot of one MDBX commit volume series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdbxCommitCountStats {
    /// Stable count label used by logs and Prometheus output.
    pub kind: &'static str,
    /// Number of commit attempts contributing a sample.
    pub samples: u64,
    /// Cumulative item or byte count.
    pub total: u64,
    /// Arithmetic mean count per commit attempt.
    pub avg: u64,
}

/// Coherent point-in-time snapshot of all MDBX commit metric families.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdbxCommitMetricsSnapshot {
    /// Cumulative commit outcomes.
    pub stats: MdbxCommitStats,
    /// Cumulative stage timings.
    pub stages: Vec<MdbxCommitStageStats>,
    /// Cumulative entry and byte counts.
    pub counts: Vec<MdbxCommitCountStats>,
}

#[derive(Debug)]
struct TimingSlot {
    calls: AtomicU64,
    total_us: AtomicU64,
}

impl TimingSlot {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
struct CountSlot {
    samples: AtomicU64,
    total: AtomicU64,
}

impl CountSlot {
    const fn new() -> Self {
        Self {
            samples: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }
}

const STAGE_ORDER: [MdbxCommitStage; 9] = [
    MdbxCommitStage::Total,
    MdbxCommitStage::TransactionOpen,
    MdbxCommitStage::TableOpen,
    MdbxCommitStage::CursorOpen,
    MdbxCommitStage::OverlaySort,
    MdbxCommitStage::OverlayVisit,
    MdbxCommitStage::CursorWrite,
    MdbxCommitStage::CursorResolve,
    MdbxCommitStage::Commit,
];
const COUNT_ORDER: [MdbxCommitCountKind; 20] = [
    MdbxCommitCountKind::Tables,
    MdbxCommitCountKind::Entries,
    MdbxCommitCountKind::Puts,
    MdbxCommitCountKind::Deletes,
    MdbxCommitCountKind::KeyBytes,
    MdbxCommitCountKind::ValueBytes,
    MdbxCommitCountKind::ValueSize0To64,
    MdbxCommitCountKind::ValueSize65To128,
    MdbxCommitCountKind::ValueSize129To256,
    MdbxCommitCountKind::ValueSize257To512,
    MdbxCommitCountKind::ValueSize513To1024,
    MdbxCommitCountKind::ValueSize1025To4096,
    MdbxCommitCountKind::ValueSize4097To16384,
    MdbxCommitCountKind::ValueSizeOver16384,
    MdbxCommitCountKind::CursorResolvePresent,
    MdbxCommitCountKind::CursorResolveAbsent,
    MdbxCommitCountKind::CursorResolveReadBytes,
    MdbxCommitCountKind::CursorResolveWriteBytes,
    MdbxCommitCountKind::CursorResolveMinorFaults,
    MdbxCommitCountKind::CursorResolveMajorFaults,
];

static ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static FAILURES: AtomicU64 = AtomicU64::new(0);
static COMMITTED_TRANSACTIONS: AtomicU64 = AtomicU64::new(0);
static SHADOW_COMMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
static SHADOW_MARKERS_COMMITTED: AtomicU64 = AtomicU64::new(0);
static STAGES: [TimingSlot; 9] = [
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
    TimingSlot::new(),
];
static COUNTS: [CountSlot; 20] = [
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
    CountSlot::new(),
];
static PUBLICATION_LOCK: Mutex<()> = Mutex::new(());

/// Access to cumulative MDBX commit metrics.
pub struct MdbxCommitMetrics;

impl MdbxCommitMetrics {
    /// Returns all metric families from one publication boundary.
    pub fn snapshot() -> MdbxCommitMetricsSnapshot {
        let _guard = PUBLICATION_LOCK.lock();
        MdbxCommitMetricsSnapshot {
            stats: Self::load_stats(),
            stages: Self::load_stage_stats(),
            counts: Self::load_count_stats(),
        }
    }

    /// Returns cumulative commit outcomes.
    pub fn stats() -> MdbxCommitStats {
        Self::snapshot().stats
    }

    /// Returns how often a coordinated-commit shadow dual-write failed.
    /// Failures are counted, not propagated: the canonical commit always
    /// continues without the pack high-water marker.
    pub fn shadow_commit_failures() -> u64 {
        SHADOW_COMMIT_FAILURES.load(Ordering::Relaxed)
    }

    /// Returns how many pack high-water markers were committed atomically
    /// with their mirrored overlays.
    pub fn shadow_markers_committed() -> u64 {
        SHADOW_MARKERS_COMMITTED.load(Ordering::Relaxed)
    }

    fn load_stats() -> MdbxCommitStats {
        MdbxCommitStats {
            attempts: ATTEMPTS.load(Ordering::Relaxed),
            failures: FAILURES.load(Ordering::Relaxed),
            committed_transactions: COMMITTED_TRANSACTIONS.load(Ordering::Relaxed),
        }
    }

    /// Returns cumulative timings for every commit stage.
    pub fn stage_stats() -> Vec<MdbxCommitStageStats> {
        Self::snapshot().stages
    }

    fn load_stage_stats() -> Vec<MdbxCommitStageStats> {
        STAGE_ORDER
            .iter()
            .map(|stage| {
                let slot = &STAGES[stage.slot_index()];
                let calls = slot.calls.load(Ordering::Relaxed);
                let total_us = slot.total_us.load(Ordering::Relaxed);
                MdbxCommitStageStats {
                    stage: stage.label(),
                    calls,
                    total_us,
                    avg_us: average(total_us, calls),
                }
            })
            .collect()
    }

    /// Returns cumulative entry and byte volumes for raw-overlay commits.
    pub fn count_stats() -> Vec<MdbxCommitCountStats> {
        Self::snapshot().counts
    }

    fn load_count_stats() -> Vec<MdbxCommitCountStats> {
        COUNT_ORDER
            .iter()
            .map(|kind| {
                let slot = &COUNTS[kind.slot_index()];
                let samples = slot.samples.load(Ordering::Relaxed);
                let total = slot.total.load(Ordering::Relaxed);
                MdbxCommitCountStats {
                    kind: kind.label(),
                    samples,
                    total,
                    avg: average(total, samples),
                }
            })
            .collect()
    }
}

/// Per-call accumulator that publishes a coherent metric sample on drop.
pub(super) struct MdbxCommitRecorder {
    started_at: Instant,
    succeeded: bool,
    committed: bool,
    stage_calls: [u64; 9],
    stage_totals_us: [u64; 9],
    counts: [u64; 20],
}

impl MdbxCommitRecorder {
    pub(super) fn start() -> Self {
        Self {
            started_at: Instant::now(),
            succeeded: false,
            committed: false,
            stage_calls: [0; 9],
            stage_totals_us: [0; 9],
            counts: [0; 20],
        }
    }

    pub(super) fn record_stage(&mut self, stage: MdbxCommitStage, elapsed_us: u64) {
        let index = stage.slot_index();
        self.stage_calls[index] = self.stage_calls[index].saturating_add(1);
        self.stage_totals_us[index] = self.stage_totals_us[index].saturating_add(elapsed_us);
    }

    pub(super) fn add_count(&mut self, kind: MdbxCommitCountKind, count: u64) {
        self.counts[kind.slot_index()] = self.counts[kind.slot_index()].saturating_add(count);
    }

    pub(super) fn mark_committed(&mut self) {
        self.committed = true;
    }

    pub(super) fn finish_success(&mut self) {
        self.succeeded = true;
    }
}

impl Drop for MdbxCommitRecorder {
    fn drop(&mut self) {
        self.record_stage(MdbxCommitStage::Total, elapsed_us(self.started_at));
        let _guard = PUBLICATION_LOCK.lock();
        ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        if !self.succeeded {
            FAILURES.fetch_add(1, Ordering::Relaxed);
        }
        if self.committed {
            COMMITTED_TRANSACTIONS.fetch_add(1, Ordering::Relaxed);
        }
        for stage in STAGE_ORDER {
            let index = stage.slot_index();
            let slot = &STAGES[index];
            slot.calls
                .fetch_add(self.stage_calls[index], Ordering::Relaxed);
            slot.total_us
                .fetch_add(self.stage_totals_us[index], Ordering::Relaxed);
        }
        for (kind, count) in COUNT_ORDER.iter().zip(self.counts) {
            let slot = &COUNTS[kind.slot_index()];
            slot.samples.fetch_add(1, Ordering::Relaxed);
            slot.total.fetch_add(count, Ordering::Relaxed);
        }
    }
}

pub(super) fn elapsed_us(started_at: Instant) -> u64 {
    started_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

pub(super) fn record_shadow_commit_failure() {
    SHADOW_COMMIT_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn record_shadow_marker_committed() {
    SHADOW_MARKERS_COMMITTED.fetch_add(1, Ordering::Relaxed);
}

fn average(total: u64, samples: u64) -> u64 {
    total.checked_div(samples).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::value_size_bucket_index;

    #[test]
    fn value_size_buckets_cover_every_boundary_without_gaps() {
        for (value_len, expected) in [
            (0, 0),
            (64, 0),
            (65, 1),
            (128, 1),
            (129, 2),
            (256, 2),
            (257, 3),
            (512, 3),
            (513, 4),
            (1_024, 4),
            (1_025, 5),
            (4_096, 5),
            (4_097, 6),
            (16_384, 6),
            (16_385, 7),
            (usize::MAX, 7),
        ] {
            assert_eq!(value_size_bucket_index(value_len), expected);
        }
    }
}
