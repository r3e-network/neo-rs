//! Observation-only telemetry shared by the offline `.acc` importer.
//!
//! Everything here is arithmetic over counters the runtime already maintains,
//! so nothing in this module can change execution, commit ordering, or
//! consensus state. The module is compiled for every `neo-node` feature set
//! because both the RocksDB-backed importer and the stub importer expose the
//! same `ImportSummary` surface.
//!
//! Two kinds of values live here:
//!
//! - **run-cumulative counters** ([`ImportCounters`]) describe what *this*
//!   process actually imported. Skipped archive records (blocks already
//!   persisted before the run) are never recorded, so a resumed import does not
//!   report them as imported, empty, non-empty, or transaction-bearing.
//! - **deltas** ([`FastPathDeltas`], [`ParallelDeltas`],
//!   [`PersistenceProfileDeltas`]) subtract a snapshot captured at import start
//!   from the snapshot captured at import end. Process-lifetime counters that
//!   existed before the run are therefore excluded instead of being reported as
//!   if this run produced them. Deltas are saturating, so a counter that is
//!   reset or truncated mid-run reports zero instead of wrapping.

use neo_system::{
    EmptyBlockProfileSnapshot, ExecutionProfileSnapshot, FastPathStatsSnapshot,
    ParallelExecutionSnapshot,
};
use std::time::Duration;

/// Converts a recorded nanosecond total into milliseconds for log fields.
pub fn nanos_to_millis(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

/// Per-run block accounting that never counts skipped archive records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounters {
    /// Blocks this run accepted into the local chain.
    pub imported_blocks: u64,
    /// Accepted blocks with zero transactions.
    pub empty_blocks: u64,
    /// Accepted blocks with at least one transaction.
    pub non_empty_blocks: u64,
    /// Transactions contained in accepted blocks.
    pub transactions: u64,
    /// Coalesced empty-block write-backs that carried two or more blocks.
    ///
    /// A run persisted through the single-block path never increments this, so
    /// the value stays zero when `NEO_IMPORT_EMPTY_BATCH_SIZE` is disabled.
    pub empty_block_batches: u64,
    /// Empty blocks accepted through a coalesced write-back.
    ///
    /// Always a subset of `empty_blocks`: it counts only the blocks that would
    /// otherwise have been persisted one at a time, which is what makes the
    /// batching win observable in the window and cumulative reports.
    pub batched_empty_blocks: u64,
}

impl ImportCounters {
    /// Records one accepted block.
    ///
    /// Callers must only invoke this for blocks they actually persisted. Archive
    /// records skipped because the height was already present must not reach
    /// this method.
    pub fn record_block(&mut self, transaction_count: usize) {
        self.imported_blocks = self.imported_blocks.saturating_add(1);
        if transaction_count == 0 {
            self.empty_blocks = self.empty_blocks.saturating_add(1);
        } else {
            self.non_empty_blocks = self.non_empty_blocks.saturating_add(1);
        }
        let transactions = u64::try_from(transaction_count).unwrap_or(u64::MAX);
        self.transactions = self.transactions.saturating_add(transactions);
    }

    /// Records one coalesced write-back covering `blocks` transaction-free blocks.
    ///
    /// Callers must only invoke this for a run persisted through the multi-block
    /// path; a run of one block is a single-block persist and must use
    /// [`Self::record_block`] instead. Those blocks are accepted exactly like any
    /// other block, so the block and empty-block totals advance by the batch size
    /// while the batch counters record how the work was grouped.
    pub fn record_empty_batch(&mut self, blocks: usize) {
        let blocks = u64::try_from(blocks).unwrap_or(u64::MAX);
        self.imported_blocks = self.imported_blocks.saturating_add(blocks);
        self.empty_blocks = self.empty_blocks.saturating_add(blocks);
        self.empty_block_batches = self.empty_block_batches.saturating_add(1);
        self.batched_empty_blocks = self.batched_empty_blocks.saturating_add(blocks);
    }

    /// Returns `self - earlier` field by field, saturating at zero.
    pub fn delta_since(&self, earlier: Self) -> Self {
        Self {
            imported_blocks: self.imported_blocks.saturating_sub(earlier.imported_blocks),
            empty_blocks: self.empty_blocks.saturating_sub(earlier.empty_blocks),
            non_empty_blocks: self
                .non_empty_blocks
                .saturating_sub(earlier.non_empty_blocks),
            transactions: self.transactions.saturating_sub(earlier.transactions),
            empty_block_batches: self
                .empty_block_batches
                .saturating_sub(earlier.empty_block_batches),
            batched_empty_blocks: self
                .batched_empty_blocks
                .saturating_sub(earlier.batched_empty_blocks),
        }
    }

    /// Blocks per second over `elapsed`.
    ///
    /// A zero (or sub-epsilon) window yields `0.0` instead of an infinity or a
    /// NaN, so a progress emission that lands in the same clock tick as the
    /// previous one stays numerically safe.
    pub fn blocks_per_sec(&self, elapsed: Duration) -> f64 {
        let secs = elapsed.as_secs_f64();
        if secs <= f64::EPSILON {
            return 0.0;
        }
        self.imported_blocks as f64 / secs
    }
}

/// ApplicationLogs write-coalescing telemetry of one import session.
///
/// Application logs are produced per block but committed per checkpoint, so
/// these counters describe the writes rather than the blocks: a run that
/// coalesced 13 million blocks into one commit per checkpoint reports a few
/// thousand flushes carrying every record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogsDeltas {
    /// Commits that wrote at least one application-log record.
    pub flush_count: u64,
    /// Records those commits wrote.
    pub flushed_records: u64,
    /// Serialized bytes those commits wrote.
    pub flushed_bytes: u64,
    /// Records still pending when the session closed.
    ///
    /// A healthy run reports zero: the final checkpoint plus the closing session
    /// flush write everything the archive produced.
    pub pending_records: u64,
}

fn u64_delta(current: u64, earlier: u64) -> u64 {
    current.saturating_sub(earlier)
}

fn duration_delta_nanos(current: Duration, earlier: Duration) -> u64 {
    u64::try_from(current.as_nanos().saturating_sub(earlier.as_nanos())).unwrap_or(u64::MAX)
}

/// Delta of the direct-dispatch counters across one import run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FastPathDeltas {
    pub direct_attempts: u64,
    pub direct_halts: u64,
    pub direct_faults: u64,
    pub eligibility_declines: u64,
    pub canonical_fallbacks: u64,
    pub verification_mismatches: u64,
}

impl FastPathDeltas {
    /// Subtracts a start snapshot from an end snapshot.
    pub fn between(start: FastPathStatsSnapshot, end: FastPathStatsSnapshot) -> Self {
        Self {
            direct_attempts: u64_delta(end.direct_attempts, start.direct_attempts),
            direct_halts: u64_delta(end.direct_halts, start.direct_halts),
            direct_faults: u64_delta(end.direct_faults, start.direct_faults),
            eligibility_declines: u64_delta(end.eligibility_declines, start.eligibility_declines),
            canonical_fallbacks: u64_delta(end.canonical_fallbacks, start.canonical_fallbacks),
            verification_mismatches: u64_delta(
                end.verification_mismatches,
                start.verification_mismatches,
            ),
        }
    }
}

/// Delta of the speculative-scheduler counters across one import run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParallelDeltas {
    pub speculative_blocks: u64,
    pub speculative_transactions: u64,
    pub conflicts: u64,
    pub reexecutions: u64,
    pub serial_fallbacks: u64,
    pub parallel_nanos: u64,
    pub serial_nanos: u64,
}

impl ParallelDeltas {
    /// Subtracts a start snapshot from an end snapshot.
    pub fn between(start: ParallelExecutionSnapshot, end: ParallelExecutionSnapshot) -> Self {
        Self {
            speculative_blocks: u64_delta(end.speculative_blocks, start.speculative_blocks),
            speculative_transactions: u64_delta(
                end.speculative_transactions,
                start.speculative_transactions,
            ),
            conflicts: u64_delta(end.conflicts, start.conflicts),
            reexecutions: u64_delta(end.reexecutions, start.reexecutions),
            serial_fallbacks: u64_delta(end.serial_fallbacks, start.serial_fallbacks),
            parallel_nanos: u64_delta(end.parallel_nanos, start.parallel_nanos),
            serial_nanos: u64_delta(end.serial_nanos, start.serial_nanos),
        }
    }
}

/// Delta of the opt-in persistence-profile totals across one import run.
///
/// The recorded stages are **nested**, never disjoint wall time:
/// `transaction_vm` is inside `transaction_loop`, `state_change_extraction` and
/// `mpt` are inside `participant_precommit`, and `store_database_write` is
/// inside `store_commit`. Summing them would double-count, so consumers must
/// treat each field as its own stage measurement.
///
/// These totals are only non-zero while persistence profiling is enabled; when
/// it is disabled the runtime drops every stage sample, so the deltas stay zero
/// and [`Self::state_label`] reports that explicitly instead of implying a
/// measured zero cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PersistenceProfileDeltas {
    /// Profiling state when the start snapshot was captured.
    pub start_enabled: bool,
    /// Profiling state when the end snapshot was captured.
    pub end_enabled: bool,
    /// Blocks whose persistence ran while profiling was enabled.
    pub blocks: u64,
    /// Transaction executions observed while profiling was enabled.
    pub transaction_executions: u64,
    /// Instructions the fast interpreter executed. Unlike the stage timings this
    /// counter is maintained independently of the profiling switch, so it is
    /// meaningful even when `state_label()` reports `disabled`.
    pub external_instructions: u64,
    pub snapshot_open_nanos: u64,
    pub transaction_hash_nanos: u64,
    pub on_persist_nanos: u64,
    pub transaction_loop_nanos: u64,
    pub transaction_vm_nanos: u64,
    pub post_persist_nanos: u64,
    pub participant_precommit_nanos: u64,
    pub store_commit_nanos: u64,
    pub store_database_write_nanos: u64,
    pub runtime_update_nanos: u64,
    pub post_commit_nanos: u64,
    pub state_change_extraction_nanos: u64,
    pub mpt_nanos: u64,
    pub store_writer_snapshot_open_nanos: u64,
    pub store_change_application_nanos: u64,
    pub store_write_batch_build_nanos: u64,
    pub store_cache_invalidation_nanos: u64,
    pub mpt_update_nanos: u64,
    pub mpt_commit_nanos: u64,
    pub mpt_root_hash_nanos: u64,
    pub mpt_hash_nanos: u64,
    pub mpt_serialization_nanos: u64,
    pub mpt_hash256_nanos: u64,
    pub mpt_node_cache_nanos: u64,
    pub mpt_node_read_nanos: u64,
    pub mpt_node_deserialize_nanos: u64,
    pub mpt_node_encode_nanos: u64,
    pub mpt_node_write_nanos: u64,
    /// The same stages restricted to transaction-free blocks. This is a
    /// *subset* of the aggregate stages, not an overlapping one: subtract it
    /// from an aggregate stage to obtain that stage's non-empty-block cost.
    pub empty_blocks: EmptyBlockProfileDeltas,
}

/// Delta of the transaction-free-block subset of the persistence profile.
///
/// Every field mirrors the aggregate stage of the same name and keeps the same
/// nesting: `mpt_*` stages are inside `participant_precommit` (through `mpt`),
/// and the `store_*` stages are inside `store_commit`. Each value is a separate
/// stage measurement and must never be summed into disjoint wall time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmptyBlockProfileDeltas {
    /// Transaction-free blocks persisted while profiling was enabled.
    pub blocks: u64,
    pub snapshot_open_nanos: u64,
    pub on_persist_nanos: u64,
    pub transaction_loop_nanos: u64,
    pub post_persist_nanos: u64,
    pub participant_precommit_nanos: u64,
    pub state_change_extraction_nanos: u64,
    pub mpt_nanos: u64,
    pub mpt_update_nanos: u64,
    pub mpt_commit_nanos: u64,
    pub mpt_hash_nanos: u64,
    pub mpt_serialization_nanos: u64,
    pub mpt_hash256_nanos: u64,
    pub mpt_node_cache_nanos: u64,
    pub mpt_node_read_nanos: u64,
    pub mpt_node_deserialize_nanos: u64,
    pub mpt_node_encode_nanos: u64,
    pub mpt_node_write_nanos: u64,
    pub store_database_write_nanos: u64,
    pub runtime_update_nanos: u64,
    pub post_commit_nanos: u64,
    pub store_commit_nanos: u64,
}

impl EmptyBlockProfileDeltas {
    /// Subtracts a start snapshot from an end snapshot.
    pub fn between(start: &EmptyBlockProfileSnapshot, end: &EmptyBlockProfileSnapshot) -> Self {
        Self {
            blocks: u64_delta(end.blocks, start.blocks),
            snapshot_open_nanos: duration_delta_nanos(end.snapshot_open, start.snapshot_open),
            on_persist_nanos: duration_delta_nanos(end.on_persist, start.on_persist),
            transaction_loop_nanos: duration_delta_nanos(
                end.transaction_loop,
                start.transaction_loop,
            ),
            post_persist_nanos: duration_delta_nanos(end.post_persist, start.post_persist),
            participant_precommit_nanos: duration_delta_nanos(
                end.participant_precommit,
                start.participant_precommit,
            ),
            state_change_extraction_nanos: duration_delta_nanos(
                end.state_change_extraction,
                start.state_change_extraction,
            ),
            mpt_nanos: duration_delta_nanos(end.mpt, start.mpt),
            mpt_update_nanos: duration_delta_nanos(end.mpt_update, start.mpt_update),
            mpt_commit_nanos: duration_delta_nanos(end.mpt_commit, start.mpt_commit),
            mpt_hash_nanos: duration_delta_nanos(end.mpt_hash, start.mpt_hash),
            mpt_serialization_nanos: duration_delta_nanos(
                end.mpt_serialization,
                start.mpt_serialization,
            ),
            mpt_hash256_nanos: duration_delta_nanos(end.mpt_hash256, start.mpt_hash256),
            mpt_node_cache_nanos: duration_delta_nanos(end.mpt_node_cache, start.mpt_node_cache),
            mpt_node_read_nanos: duration_delta_nanos(end.mpt_node_read, start.mpt_node_read),
            mpt_node_deserialize_nanos: duration_delta_nanos(
                end.mpt_node_deserialize,
                start.mpt_node_deserialize,
            ),
            mpt_node_encode_nanos: duration_delta_nanos(end.mpt_node_encode, start.mpt_node_encode),
            mpt_node_write_nanos: duration_delta_nanos(end.mpt_node_write, start.mpt_node_write),
            store_database_write_nanos: duration_delta_nanos(
                end.store_database_write,
                start.store_database_write,
            ),
            runtime_update_nanos: duration_delta_nanos(end.runtime_update, start.runtime_update),
            post_commit_nanos: duration_delta_nanos(end.post_commit, start.post_commit),
            store_commit_nanos: duration_delta_nanos(end.store_commit, start.store_commit),
        }
    }
}

impl PersistenceProfileDeltas {
    /// Subtracts a start snapshot from an end snapshot.
    pub fn between(start: &ExecutionProfileSnapshot, end: &ExecutionProfileSnapshot) -> Self {
        Self {
            start_enabled: start.enabled,
            end_enabled: end.enabled,
            blocks: u64_delta(end.blocks, start.blocks),
            transaction_executions: u64_delta(
                end.transaction_executions,
                start.transaction_executions,
            ),
            external_instructions: u64_delta(
                end.external_instructions,
                start.external_instructions,
            ),
            snapshot_open_nanos: duration_delta_nanos(end.snapshot_open, start.snapshot_open),
            transaction_hash_nanos: duration_delta_nanos(
                end.transaction_hash,
                start.transaction_hash,
            ),
            on_persist_nanos: duration_delta_nanos(end.on_persist, start.on_persist),
            transaction_loop_nanos: duration_delta_nanos(
                end.transaction_loop,
                start.transaction_loop,
            ),
            transaction_vm_nanos: duration_delta_nanos(end.transaction_vm, start.transaction_vm),
            post_persist_nanos: duration_delta_nanos(end.post_persist, start.post_persist),
            participant_precommit_nanos: duration_delta_nanos(
                end.participant_precommit,
                start.participant_precommit,
            ),
            store_commit_nanos: duration_delta_nanos(end.store_commit, start.store_commit),
            store_database_write_nanos: duration_delta_nanos(
                end.store_database_write,
                start.store_database_write,
            ),
            runtime_update_nanos: duration_delta_nanos(end.runtime_update, start.runtime_update),
            post_commit_nanos: duration_delta_nanos(end.post_commit, start.post_commit),
            state_change_extraction_nanos: duration_delta_nanos(
                end.state_change_extraction,
                start.state_change_extraction,
            ),
            mpt_nanos: duration_delta_nanos(end.mpt, start.mpt),
            store_writer_snapshot_open_nanos: duration_delta_nanos(
                end.store_writer_snapshot_open,
                start.store_writer_snapshot_open,
            ),
            store_change_application_nanos: duration_delta_nanos(
                end.store_change_application,
                start.store_change_application,
            ),
            store_write_batch_build_nanos: duration_delta_nanos(
                end.store_write_batch_build,
                start.store_write_batch_build,
            ),
            store_cache_invalidation_nanos: duration_delta_nanos(
                end.store_cache_invalidation,
                start.store_cache_invalidation,
            ),
            mpt_update_nanos: duration_delta_nanos(end.mpt_update, start.mpt_update),
            mpt_commit_nanos: duration_delta_nanos(end.mpt_commit, start.mpt_commit),
            mpt_root_hash_nanos: duration_delta_nanos(end.mpt_root_hash, start.mpt_root_hash),
            mpt_hash_nanos: duration_delta_nanos(end.mpt_hash, start.mpt_hash),
            mpt_serialization_nanos: duration_delta_nanos(
                end.mpt_serialization,
                start.mpt_serialization,
            ),
            mpt_hash256_nanos: duration_delta_nanos(end.mpt_hash256, start.mpt_hash256),
            mpt_node_cache_nanos: duration_delta_nanos(end.mpt_node_cache, start.mpt_node_cache),
            mpt_node_read_nanos: duration_delta_nanos(end.mpt_node_read, start.mpt_node_read),
            mpt_node_deserialize_nanos: duration_delta_nanos(
                end.mpt_node_deserialize,
                start.mpt_node_deserialize,
            ),
            mpt_node_encode_nanos: duration_delta_nanos(end.mpt_node_encode, start.mpt_node_encode),
            mpt_node_write_nanos: duration_delta_nanos(end.mpt_node_write, start.mpt_node_write),
            empty_blocks: EmptyBlockProfileDeltas::between(&start.empty_blocks, &end.empty_blocks),
        }
    }

    /// Label describing whether the stage deltas were actually measured.
    pub fn state_label(&self) -> &'static str {
        match (self.start_enabled, self.end_enabled) {
            (true, true) => "enabled",
            // Nested MPT/store sinks follow the profiling switch, so a state
            // change mid-run makes the deltas partial rather than complete.
            (false, false) => "disabled",
            _ => "toggled-mid-run",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_counters_split_empty_and_non_empty_blocks() {
        let mut counters = ImportCounters::default();
        counters.record_block(0);
        counters.record_block(3);
        counters.record_block(0);

        assert_eq!(counters.imported_blocks, 3);
        assert_eq!(counters.empty_blocks, 2);
        assert_eq!(counters.non_empty_blocks, 1);
        assert_eq!(counters.transactions, 3);
    }

    #[test]
    fn window_delta_counts_only_blocks_accepted_after_the_previous_emission() {
        let mut counters = ImportCounters::default();
        counters.record_block(2);
        counters.record_block(0);
        let first_window_end = counters;

        counters.record_block(4);
        let second_window = counters.delta_since(first_window_end);

        assert_eq!(
            second_window,
            ImportCounters {
                imported_blocks: 1,
                empty_blocks: 0,
                non_empty_blocks: 1,
                transactions: 4,
                empty_block_batches: 0,
                batched_empty_blocks: 0,
            }
        );
        // The run-cumulative totals still describe the whole run.
        assert_eq!(counters.imported_blocks, 3);
        assert_eq!(counters.transactions, 6);
    }

    #[test]
    fn empty_batches_are_counted_separately_from_single_block_persists() {
        let mut counters = ImportCounters::default();
        counters.record_block(0);
        let after_single = counters;

        counters.record_empty_batch(256);
        counters.record_block(2);

        // The batch advances the plain totals exactly like the single-block path
        // so windows and run rates never disagree about accepted blocks.
        assert_eq!(counters.imported_blocks, 258);
        assert_eq!(counters.empty_blocks, 257);
        assert_eq!(counters.non_empty_blocks, 1);
        assert_eq!(counters.transactions, 2);
        assert_eq!(counters.empty_block_batches, 1);
        assert_eq!(counters.batched_empty_blocks, 256);

        // The window delta carries the batching shape, which is what a progress
        // line needs to explain a sudden jump in blocks per second.
        let window = counters.delta_since(after_single);
        assert_eq!(window.imported_blocks, 257);
        assert_eq!(window.empty_blocks, 256);
        assert_eq!(window.empty_block_batches, 1);
        assert_eq!(window.batched_empty_blocks, 256);
    }

    #[test]
    fn window_rate_is_finite_and_safe_for_zero_duration() {
        let mut counters = ImportCounters::default();
        counters.record_block(0);
        counters.record_block(0);

        assert_eq!(counters.blocks_per_sec(Duration::ZERO), 0.0);
        let tiny_window = counters.blocks_per_sec(Duration::from_nanos(1));
        assert!(tiny_window.is_finite());
        assert!((tiny_window - 2_000_000_000.0).abs() < 1.0);
        assert!(counters.blocks_per_sec(Duration::from_secs(2)).is_finite());
    }

    #[test]
    fn fast_path_delta_excludes_counters_that_predate_the_run() {
        let start = FastPathStatsSnapshot {
            direct_attempts: 11,
            direct_halts: 7,
            direct_faults: 1,
            eligibility_declines: 3,
            canonical_fallbacks: 2,
            verification_mismatches: 1,
        };
        let end = FastPathStatsSnapshot {
            direct_attempts: 14,
            direct_halts: 9,
            direct_faults: 2,
            eligibility_declines: 3,
            canonical_fallbacks: 4,
            verification_mismatches: 1,
        };

        let delta = FastPathDeltas::between(start, end);
        assert_eq!(delta.direct_attempts, 3);
        assert_eq!(delta.direct_halts, 2);
        assert_eq!(delta.direct_faults, 1);
        assert_eq!(delta.eligibility_declines, 0);
        assert_eq!(delta.canonical_fallbacks, 2);
        assert_eq!(delta.verification_mismatches, 0);
    }

    #[test]
    fn parallel_delta_excludes_counters_that_predate_the_run() {
        let start = ParallelExecutionSnapshot {
            speculative_blocks: 5,
            speculative_transactions: 40,
            conflicts: 9,
            reexecutions: 4,
            serial_fallbacks: 2,
            parallel_nanos: 1_000,
            serial_nanos: 500,
        };
        let end = ParallelExecutionSnapshot {
            speculative_blocks: 8,
            speculative_transactions: 55,
            conflicts: 9,
            reexecutions: 6,
            serial_fallbacks: 3,
            parallel_nanos: 2_500,
            serial_nanos: 500,
        };

        let delta = ParallelDeltas::between(start, end);
        assert_eq!(delta.speculative_blocks, 3);
        assert_eq!(delta.speculative_transactions, 15);
        assert_eq!(delta.conflicts, 0);
        assert_eq!(delta.reexecutions, 2);
        assert_eq!(delta.serial_fallbacks, 1);
        assert_eq!(delta.parallel_nanos, 1_500);
        assert_eq!(delta.serial_nanos, 0);
    }

    #[test]
    fn saturating_deltas_never_wrap_when_a_counter_is_reset() {
        let start = ImportCounters {
            imported_blocks: 9,
            empty_blocks: 4,
            non_empty_blocks: 5,
            transactions: 20,
            empty_block_batches: 1,
            batched_empty_blocks: 3,
        };
        let end = ImportCounters {
            imported_blocks: 2,
            empty_blocks: 1,
            non_empty_blocks: 1,
            transactions: 3,
            empty_block_batches: 0,
            batched_empty_blocks: 0,
        };
        assert_eq!(end.delta_since(start), ImportCounters::default());

        let resets = ParallelDeltas::between(
            ParallelExecutionSnapshot {
                parallel_nanos: 900,
                ..ParallelExecutionSnapshot::default()
            },
            ParallelExecutionSnapshot::default(),
        );
        assert_eq!(resets.parallel_nanos, 0);
    }

    #[test]
    fn profile_deltas_exclude_prior_totals_and_label_disabled_state() {
        let start = ExecutionProfileSnapshot {
            enabled: true,
            blocks: 100,
            transaction_executions: 900,
            external_instructions: 700,
            snapshot_open: Duration::from_millis(5),
            on_persist: Duration::from_millis(7),
            transaction_loop: Duration::from_millis(11),
            post_persist: Duration::from_millis(3),
            participant_precommit: Duration::from_millis(13),
            state_change_extraction: Duration::from_millis(17),
            mpt: Duration::from_millis(19),
            store_commit: Duration::from_millis(23),
            store_database_write: Duration::from_millis(29),
            store_write_batch_build: Duration::from_millis(3),
            mpt_update: Duration::from_millis(31),
            empty_blocks: EmptyBlockProfileSnapshot {
                blocks: 5,
                on_persist: Duration::from_millis(2),
                store_database_write: Duration::from_millis(1),
                mpt_node_write: Duration::from_millis(4),
                post_commit: Duration::from_millis(6),
                ..EmptyBlockProfileSnapshot::default()
            },
            ..ExecutionProfileSnapshot::default()
        };
        let end = ExecutionProfileSnapshot {
            enabled: true,
            blocks: 140,
            transaction_executions: 1300,
            external_instructions: 1100,
            snapshot_open: Duration::from_millis(9),
            on_persist: Duration::from_millis(17),
            transaction_loop: Duration::from_millis(31),
            post_persist: Duration::from_millis(4),
            participant_precommit: Duration::from_millis(33),
            state_change_extraction: Duration::from_millis(27),
            mpt: Duration::from_millis(39),
            store_commit: Duration::from_millis(53),
            store_database_write: Duration::from_millis(59),
            store_write_batch_build: Duration::from_millis(13),
            mpt_update: Duration::from_millis(41),
            empty_blocks: EmptyBlockProfileSnapshot {
                blocks: 8,
                on_persist: Duration::from_millis(5),
                store_database_write: Duration::from_millis(4),
                mpt_node_write: Duration::from_millis(9),
                post_commit: Duration::from_millis(8),
                ..EmptyBlockProfileSnapshot::default()
            },
            ..ExecutionProfileSnapshot::default()
        };

        let delta = PersistenceProfileDeltas::between(&start, &end);
        assert_eq!(delta.state_label(), "enabled");
        assert_eq!(delta.blocks, 40);
        assert_eq!(delta.transaction_executions, 400);
        assert_eq!(delta.external_instructions, 400);
        assert_eq!(delta.snapshot_open_nanos, 4_000_000);
        assert_eq!(delta.on_persist_nanos, 10_000_000);
        assert_eq!(delta.transaction_loop_nanos, 20_000_000);
        assert_eq!(delta.post_persist_nanos, 1_000_000);
        assert_eq!(delta.participant_precommit_nanos, 20_000_000);
        assert_eq!(delta.state_change_extraction_nanos, 10_000_000);
        assert_eq!(delta.mpt_nanos, 20_000_000);
        assert_eq!(delta.store_commit_nanos, 30_000_000);
        assert_eq!(delta.store_database_write_nanos, 30_000_000);
        assert_eq!(delta.store_write_batch_build_nanos, 10_000_000);
        assert_eq!(delta.mpt_update_nanos, 10_000_000);
        assert_eq!(delta.empty_blocks.blocks, 3);
        assert_eq!(delta.empty_blocks.on_persist_nanos, 3_000_000);
        assert_eq!(delta.empty_blocks.store_database_write_nanos, 3_000_000);
        assert_eq!(delta.empty_blocks.mpt_node_write_nanos, 5_000_000);
        assert_eq!(delta.empty_blocks.post_commit_nanos, 2_000_000);
        // The empty-block subset is a subset of the aggregate, never an
        // additional overlapping stage, so it stays below the aggregate total
        // for the same stage.
        assert!(delta.empty_blocks.mpt_nanos <= delta.mpt_nanos);
        assert_eq!(nanos_to_millis(delta.store_commit_nanos), 30.0);

        let disabled = PersistenceProfileDeltas::between(
            &ExecutionProfileSnapshot::default(),
            &ExecutionProfileSnapshot::default(),
        );
        assert_eq!(disabled.state_label(), "disabled");
        assert_eq!(disabled, PersistenceProfileDeltas::default());
        assert_eq!(disabled.external_instructions, 0);

        let toggled = PersistenceProfileDeltas::between(
            &ExecutionProfileSnapshot::default(),
            &ExecutionProfileSnapshot {
                enabled: true,
                ..ExecutionProfileSnapshot::default()
            },
        );
        assert_eq!(toggled.state_label(), "toggled-mid-run");
    }
}
