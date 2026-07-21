//! Bounded pack-store configuration.

use std::fmt;

use anyhow::Result;

use super::super::read_view::PACK_BATCH_VALUES_PER_WORKER;
use super::identity::PACK_SEGMENT_HEADER_LEN;

/// One caller-configurable pack-store resource field.
///
/// This enum is carried by [`PackStoreConfigError`] so application composition
/// can map invalid operator settings without parsing error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackStoreConfigField {
    /// Maximum rows accepted in one append frame.
    MaxFrameRows,
    /// Maximum encoded payload bytes accepted in one append frame.
    MaxFramePayloadBytes,
    /// Normal segment-rotation target.
    TargetSegmentBytes,
    /// Absolute bytes accepted in one segment.
    MaxSegmentBytes,
    /// Maximum recent immutable runs retained before backpressure.
    MaxRecentRuns,
    /// Maximum derived-index levels retained by the store.
    MaxIndexLevels,
    /// Maximum resident decoded-index bytes.
    MaxIndexMemoryBytes,
    /// Maximum unpublished or queued append bytes.
    MaxPendingBytes,
    /// Maximum excess immutable runs tolerated before producer backpressure.
    MaxCompactionDebtRuns,
    /// Soft run bound for level zero.
    LevelZeroRunBound,
    /// Soft run bound for compacted levels.
    CompactedLevelRunBound,
    /// Maximum inputs consumed by one compaction merge.
    CompactionFanout,
    /// Workers used to copy immutable values for sorted batch reads.
    BatchValueWorkers,
}

impl fmt::Display for PackStoreConfigField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MaxFrameRows => "max_frame_rows",
            Self::MaxFramePayloadBytes => "max_frame_payload_bytes",
            Self::TargetSegmentBytes => "target_segment_bytes",
            Self::MaxSegmentBytes => "max_segment_bytes",
            Self::MaxRecentRuns => "max_recent_runs",
            Self::MaxIndexLevels => "max_index_levels",
            Self::MaxIndexMemoryBytes => "max_index_memory_bytes",
            Self::MaxPendingBytes => "max_pending_bytes",
            Self::MaxCompactionDebtRuns => "max_compaction_debt_runs",
            Self::LevelZeroRunBound => "level_zero_run_bound",
            Self::CompactedLevelRunBound => "compacted_level_run_bound",
            Self::CompactionFanout => "compaction_fanout",
            Self::BatchValueWorkers => "batch_value_workers",
        })
    }
}

/// Typed validation failures for [`PackStoreConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PackStoreConfigError {
    /// A scalar resource setting falls outside its supported closed interval.
    #[error("{field} must be in {minimum}..={maximum}, got {actual}")]
    ValueOutOfRange {
        /// Invalid configuration field.
        field: PackStoreConfigField,
        /// Supplied value.
        actual: u64,
        /// Inclusive minimum.
        minimum: u64,
        /// Inclusive maximum.
        maximum: u64,
    },
    /// The normal rotation target is greater than the absolute segment bound.
    #[error("target_segment_bytes {target_bytes} exceeds max_segment_bytes {maximum_bytes}")]
    SegmentTargetExceedsMaximum {
        /// Supplied normal rotation target.
        target_bytes: u64,
        /// Supplied absolute segment bound.
        maximum_bytes: u64,
    },
}

/// Complete resource contract for one pack-store handle.
///
/// Fields are private so every value is validated before it can reach store
/// I/O. Consuming `with_*` methods preserve value semantics and keep the type
/// [`Copy`]; configuration never becomes mutable shared runtime state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackStoreConfig {
    max_frame_rows: u64,
    max_frame_payload_bytes: u64,
    target_segment_bytes: u64,
    max_segment_bytes: u64,
    max_recent_runs: usize,
    max_index_levels: u32,
    max_index_memory_bytes: u64,
    max_pending_bytes: u64,
    max_compaction_debt_runs: usize,
    compaction: CompactionConfig,
    read_options: PackStoreOptions,
}

impl PackStoreConfig {
    /// Absolute decoder cap for rows in one append frame.
    pub const HARD_MAX_FRAME_ROWS: u64 = 4_000_000;
    /// Absolute decoder cap for encoded payload bytes in one append frame.
    pub const HARD_MAX_FRAME_PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    /// Absolute number of recent runs accepted by this implementation.
    pub const HARD_MAX_RECENT_RUNS: usize = 4_096;
    /// Absolute derived-index level count accepted by this implementation.
    pub const HARD_MAX_INDEX_LEVELS: u32 = 64;
    /// Absolute soft run bound accepted for an individual index level.
    pub const HARD_MAX_LEVEL_RUN_BOUND: usize = 4_096;
    /// Absolute number of inputs accepted by one compaction merge.
    pub const HARD_MAX_COMPACTION_FANOUT: usize = 256;
    /// Absolute excess-run debt accepted before producer backpressure.
    pub const HARD_MAX_COMPACTION_DEBT_RUNS: usize = 4_096;
    /// Absolute immutable-value copy workers accepted by the read path.
    pub const HARD_MAX_BATCH_VALUE_WORKERS: usize = 8;

    /// Valid production-oriented baseline with all accelerators disabled.
    pub const DEFAULT: Self = Self {
        max_frame_rows: Self::HARD_MAX_FRAME_ROWS,
        max_frame_payload_bytes: Self::HARD_MAX_FRAME_PAYLOAD_BYTES,
        target_segment_bytes: 4 * 1024 * 1024 * 1024,
        max_segment_bytes: 8 * 1024 * 1024 * 1024,
        max_recent_runs: 64,
        max_index_levels: 32,
        max_index_memory_bytes: 512 * 1024 * 1024,
        max_pending_bytes: 4 * 1024 * 1024 * 1024,
        max_compaction_debt_runs: 16,
        compaction: CompactionConfig {
            l0_bound: 8,
            l1_bound: 8,
            fanout: 16,
        },
        read_options: PackStoreOptions {
            random_point_mmap: false,
            batch_value_workers: 1,
        },
    };

    /// Returns the configured maximum frame-row count.
    pub const fn max_frame_rows(self) -> u64 {
        self.max_frame_rows
    }

    /// Returns the configured maximum encoded frame-payload bytes.
    pub const fn max_frame_payload_bytes(self) -> u64 {
        self.max_frame_payload_bytes
    }

    /// Returns the normal segment-rotation target in bytes.
    pub const fn target_segment_bytes(self) -> u64 {
        self.target_segment_bytes
    }

    /// Returns the absolute segment byte bound.
    pub const fn max_segment_bytes(self) -> u64 {
        self.max_segment_bytes
    }

    /// Returns the maximum recent immutable-run count.
    pub const fn max_recent_runs(self) -> usize {
        self.max_recent_runs
    }

    /// Returns the maximum derived-index level count.
    pub const fn max_index_levels(self) -> u32 {
        self.max_index_levels
    }

    /// Returns the resident decoded-index memory bound in bytes.
    pub const fn max_index_memory_bytes(self) -> u64 {
        self.max_index_memory_bytes
    }

    /// Returns the unpublished/queued append byte bound.
    pub const fn max_pending_bytes(self) -> u64 {
        self.max_pending_bytes
    }

    /// Returns the maximum excess-run compaction debt.
    pub const fn max_compaction_debt_runs(self) -> usize {
        self.max_compaction_debt_runs
    }

    /// Returns the level-zero soft run bound.
    pub const fn level_zero_run_bound(self) -> usize {
        self.compaction.l0_bound
    }

    /// Returns the soft run bound shared by compacted levels.
    pub const fn compacted_level_run_bound(self) -> usize {
        self.compaction.l1_bound
    }

    /// Returns the maximum compaction input count.
    pub const fn compaction_fanout(self) -> usize {
        self.compaction.fanout
    }

    /// Returns physical read options, which do not affect durable bytes.
    pub const fn read_options(self) -> PackStoreOptions {
        self.read_options
    }

    /// Replaces the maximum frame-row count and validates the complete value.
    pub fn with_max_frame_rows(self, max_frame_rows: u64) -> Result<Self, PackStoreConfigError> {
        Self {
            max_frame_rows,
            ..self
        }
        .validated()
    }

    /// Replaces the maximum encoded frame-payload bytes.
    pub fn with_max_frame_payload_bytes(
        self,
        max_frame_payload_bytes: u64,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            max_frame_payload_bytes,
            ..self
        }
        .validated()
    }

    /// Replaces the normal and absolute segment byte limits together.
    pub fn with_segment_limits(
        self,
        target_segment_bytes: u64,
        max_segment_bytes: u64,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            target_segment_bytes,
            max_segment_bytes,
            ..self
        }
        .validated()
    }

    /// Replaces the maximum recent immutable-run count.
    pub fn with_max_recent_runs(
        self,
        max_recent_runs: usize,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            max_recent_runs,
            ..self
        }
        .validated()
    }

    /// Replaces the maximum derived-index level count.
    pub fn with_max_index_levels(
        self,
        max_index_levels: u32,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            max_index_levels,
            ..self
        }
        .validated()
    }

    /// Replaces the resident decoded-index memory bound.
    pub fn with_max_index_memory_bytes(
        self,
        max_index_memory_bytes: u64,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            max_index_memory_bytes,
            ..self
        }
        .validated()
    }

    /// Replaces the unpublished/queued append byte bound.
    pub fn with_max_pending_bytes(
        self,
        max_pending_bytes: u64,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            max_pending_bytes,
            ..self
        }
        .validated()
    }

    /// Replaces the maximum excess-run compaction debt.
    pub fn with_max_compaction_debt_runs(
        self,
        max_compaction_debt_runs: usize,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            max_compaction_debt_runs,
            ..self
        }
        .validated()
    }

    /// Replaces leveled-compaction soft bounds and merge fanout together.
    pub fn with_compaction_bounds(
        self,
        level_zero_run_bound: usize,
        compacted_level_run_bound: usize,
        compaction_fanout: usize,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            compaction: CompactionConfig {
                l0_bound: level_zero_run_bound,
                l1_bound: compacted_level_run_bound,
                fanout: compaction_fanout,
            },
            ..self
        }
        .validated()
    }

    /// Replaces physical read options without changing format semantics.
    pub fn with_read_options(
        self,
        read_options: PackStoreOptions,
    ) -> Result<Self, PackStoreConfigError> {
        Self {
            read_options,
            ..self
        }
        .validated()
    }

    /// Validates every scalar and cross-field resource invariant.
    pub fn validate(self) -> Result<(), PackStoreConfigError> {
        validate_range(
            PackStoreConfigField::MaxFrameRows,
            self.max_frame_rows,
            1,
            Self::HARD_MAX_FRAME_ROWS,
        )?;
        validate_range(
            PackStoreConfigField::MaxFramePayloadBytes,
            self.max_frame_payload_bytes,
            1,
            Self::HARD_MAX_FRAME_PAYLOAD_BYTES,
        )?;
        validate_range(
            PackStoreConfigField::TargetSegmentBytes,
            self.target_segment_bytes,
            PACK_SEGMENT_HEADER_LEN + 1,
            i64::MAX as u64,
        )?;
        validate_range(
            PackStoreConfigField::MaxSegmentBytes,
            self.max_segment_bytes,
            PACK_SEGMENT_HEADER_LEN + 1,
            i64::MAX as u64,
        )?;
        if self.target_segment_bytes > self.max_segment_bytes {
            return Err(PackStoreConfigError::SegmentTargetExceedsMaximum {
                target_bytes: self.target_segment_bytes,
                maximum_bytes: self.max_segment_bytes,
            });
        }
        validate_range(
            PackStoreConfigField::MaxRecentRuns,
            self.max_recent_runs as u64,
            1,
            Self::HARD_MAX_RECENT_RUNS as u64,
        )?;
        validate_range(
            PackStoreConfigField::MaxIndexLevels,
            u64::from(self.max_index_levels),
            2,
            u64::from(Self::HARD_MAX_INDEX_LEVELS),
        )?;
        validate_range(
            PackStoreConfigField::MaxIndexMemoryBytes,
            self.max_index_memory_bytes,
            1,
            i64::MAX as u64,
        )?;
        validate_range(
            PackStoreConfigField::MaxPendingBytes,
            self.max_pending_bytes,
            1,
            i64::MAX as u64,
        )?;
        validate_range(
            PackStoreConfigField::MaxCompactionDebtRuns,
            self.max_compaction_debt_runs as u64,
            1,
            Self::HARD_MAX_COMPACTION_DEBT_RUNS as u64,
        )?;
        validate_range(
            PackStoreConfigField::LevelZeroRunBound,
            self.compaction.l0_bound as u64,
            1,
            Self::HARD_MAX_LEVEL_RUN_BOUND as u64,
        )?;
        validate_range(
            PackStoreConfigField::CompactedLevelRunBound,
            self.compaction.l1_bound as u64,
            1,
            Self::HARD_MAX_LEVEL_RUN_BOUND as u64,
        )?;
        validate_range(
            PackStoreConfigField::CompactionFanout,
            self.compaction.fanout as u64,
            2,
            Self::HARD_MAX_COMPACTION_FANOUT as u64,
        )?;
        validate_range(
            PackStoreConfigField::BatchValueWorkers,
            self.read_options.batch_value_workers as u64,
            1,
            Self::HARD_MAX_BATCH_VALUE_WORKERS as u64,
        )
    }

    fn validated(self) -> Result<Self, PackStoreConfigError> {
        self.validate()?;
        Ok(self)
    }

    pub(in crate::engine::store) const fn compaction_config(self) -> CompactionConfig {
        self.compaction
    }
}

impl Default for PackStoreConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn validate_range(
    field: PackStoreConfigField,
    actual: u64,
    minimum: u64,
    maximum: u64,
) -> Result<(), PackStoreConfigError> {
    if !(minimum..=maximum).contains(&actual) {
        return Err(PackStoreConfigError::ValueOutOfRange {
            field,
            actual,
            minimum,
            maximum,
        });
    }
    Ok(())
}

/// Physical read-path options that do not change pack bytes or lookup
/// semantics. Every accelerator is disabled by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackStoreOptions {
    /// Map immutable pack and index files a second time with `MADV_RANDOM`.
    /// All index-located payloads and sparse index-window probes use that view;
    /// compaction, validation, and scrub keep the ordinary mapping.
    pub random_point_mmap: bool,
    /// Workers used to copy values for large sorted batch reads. A value of
    /// one keeps the sequential path. Values above one split only immutable
    /// payload reads; index lookup and result publication remain ordered.
    pub batch_value_workers: usize,
}

impl PackStoreOptions {
    /// Configured worker count capped by the logical CPUs visible to this
    /// process. Failure to query the host fails closed to the sequential path.
    pub fn effective_batch_value_workers(self) -> usize {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        self.batch_value_workers.min(available)
    }

    /// Minimum number of located values required before parallel copying is
    /// worthwhile for this configuration.
    pub fn batch_value_parallel_threshold(self) -> usize {
        self.effective_batch_value_workers()
            .saturating_mul(PACK_BATCH_VALUES_PER_WORKER)
    }

    pub(in crate::engine::store) fn normalized_for_host(self) -> Self {
        Self {
            random_point_mmap: self.random_point_mmap,
            batch_value_workers: self.effective_batch_value_workers(),
        }
    }
}

impl Default for PackStoreOptions {
    fn default() -> Self {
        Self {
            random_point_mmap: false,
            batch_value_workers: 1,
        }
    }
}

/// Leveled compaction bounds for the derived index runs. Level 0 holds the
/// most recent append runs; when a level exceeds its run bound the oldest
/// runs (up to `fanout`) merge into one run at the next level. Payload
/// frames are never rewritten; compacted records keep pointing at the
/// original frame bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CompactionConfig {
    pub(in crate::engine::store) l0_bound: usize,
    pub(in crate::engine::store) l1_bound: usize,
    pub(in crate::engine::store) fanout: usize,
}

impl Default for CompactionConfig {
    /// Every level holds at most 8 runs; one cycle merges up to 16 inputs.
    fn default() -> Self {
        PackStoreConfig::DEFAULT.compaction_config()
    }
}

#[cfg(test)]
mod tests;
