//! Empty-block write-back planning for the offline `.acc` importer.
//!
//! The importer decodes archive records one at a time and can persist a run of
//! consecutive transaction-free blocks through a single coalesced store
//! transaction. Everything that decides *when* such a run must be persisted is
//! pure arithmetic over heights and counters, so it lives here instead of inside
//! the import loop: the decisions are the correctness-critical part, and they are
//! unit-testable without a node.
//!
//! Two invariants shape the planning:
//!
//! - A coalesced run must never delay a checkpoint, a stop height, an archive
//!   end, or a progress emission. Every one of those boundaries ends the run at
//!   exactly the height a single-block import would have reached it.
//! - A run is only ever persisted as one unit when it is a contiguous,
//!   predecessor-linked chain of transaction-free blocks starting at the current
//!   tip. Anything else is rejected before the persistence layer can start
//!   speculative execution.

use neo_primitives::UInt256;

/// Largest run the persistence layer accepts for one empty-block write-back.
pub const MAX_EMPTY_BATCH_SIZE: usize = neo_system::NeoSystem::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS;

/// Numbers that decide when decoded transaction-free blocks are persisted.
///
/// `empty_batch_cap` mirrors `NEO_IMPORT_EMPTY_BATCH_SIZE`: `0` (the default)
/// keeps the historical one-block-per-call path, and only a cap of two or more
/// enables coalescing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportDrainPolicy {
    /// Blocks per empty-block write-back batch; `0` or `1` disables batching.
    pub empty_batch_cap: usize,
    /// Checkpoint interval in accepted blocks. A boundary is never crossed.
    pub flush_interval: u64,
    /// Progress interval in accepted blocks.
    pub progress_interval: u64,
    /// Forces a progress emission while blocks are slow.
    pub progress_heartbeat_secs: u64,
}

impl ImportDrainPolicy {
    /// Builds a policy from already parsed configuration values.
    pub fn new(
        empty_batch_cap: usize,
        flush_interval: u64,
        progress_interval: u64,
        progress_heartbeat_secs: u64,
    ) -> Self {
        Self {
            empty_batch_cap,
            flush_interval,
            progress_interval,
            progress_heartbeat_secs,
        }
    }

    /// Returns whether multi-block write-back is enabled.
    ///
    /// A cap of one describes a batch of one block, which must take the
    /// single-block path, so it is reported as disabled rather than as a
    /// degenerate batch.
    pub fn batching_enabled(&self) -> bool {
        self.empty_batch_cap >= 2
    }
}

impl Default for ImportDrainPolicy {
    fn default() -> Self {
        Self {
            empty_batch_cap: 0,
            flush_interval: 10_000,
            progress_interval: 1_000,
            progress_heartbeat_secs: 60,
        }
    }
}

/// Clamps a configured batch size to what the persistence layer accepts.
///
/// Returns the usable size and whether the configured value was reduced, so the
/// importer can report a silently narrowed batch instead of ignoring it.
pub fn clamp_empty_batch_size(configured: usize) -> (usize, bool) {
    if configured > MAX_EMPTY_BATCH_SIZE {
        (MAX_EMPTY_BATCH_SIZE, true)
    } else {
        (configured, false)
    }
}

/// Why a decoded run of transaction-free blocks is being persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainTrigger {
    /// Multi-block write-back is off, so each decoded block is persisted alone.
    BatchingDisabled,
    /// The configured batch cap was reached.
    BatchCap,
    /// The run ends exactly on a checkpoint boundary.
    CheckpointBoundary,
    /// The run reaches the configured stop height.
    StopHeight,
    /// The next record carries transactions, so it ends the empty run.
    NonEmptySuccessor,
    /// The archive ended.
    EndOfArchive,
    /// The next record could not be read or decoded.
    ReadFailure,
    /// A progress emission is due and must include this run.
    ProgressWindow,
}

impl DrainTrigger {
    /// Stable low-cardinality label for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BatchingDisabled => "batching-disabled",
            Self::BatchCap => "batch-cap",
            Self::CheckpointBoundary => "checkpoint-boundary",
            Self::StopHeight => "stop-height",
            Self::NonEmptySuccessor => "non-empty-successor",
            Self::EndOfArchive => "end-of-archive",
            Self::ReadFailure => "read-failure",
            Self::ProgressWindow => "progress-window",
        }
    }
}

/// Returns whether `count` lands exactly on an `interval` boundary.
///
/// A zero interval means "never": without this guard a disabled interval would
/// panic on the remainder instead of simply never matching.
fn on_boundary(count: u64, interval: u64) -> bool {
    interval > 0 && count.is_multiple_of(interval)
}

/// Decides whether the pending run of transaction-free blocks must be persisted.
///
/// `pending_len` counts decoded but not yet persisted blocks, `imported` counts
/// blocks already accepted by this run, and `last_pending_height` is the height
/// of the newest decoded block. The function is pure: it never reads clocks, so
/// the caller decides the heartbeat and every boundary is testable.
pub fn empty_run_drain_trigger(
    pending_len: usize,
    imported: u64,
    last_pending_height: u32,
    stop_height: Option<u32>,
    progress_heartbeat_due: bool,
    policy: ImportDrainPolicy,
) -> Option<DrainTrigger> {
    if pending_len == 0 {
        return None;
    }
    if !policy.batching_enabled() {
        return Some(DrainTrigger::BatchingDisabled);
    }
    if stop_height.is_some_and(|limit| last_pending_height >= limit) {
        return Some(DrainTrigger::StopHeight);
    }
    if pending_len >= policy.empty_batch_cap {
        return Some(DrainTrigger::BatchCap);
    }

    let accepted_after = imported.saturating_add(u64::try_from(pending_len).unwrap_or(u64::MAX));
    if on_boundary(accepted_after, policy.flush_interval) {
        return Some(DrainTrigger::CheckpointBoundary);
    }
    if on_boundary(accepted_after, policy.progress_interval) || progress_heartbeat_due {
        return Some(DrainTrigger::ProgressWindow);
    }
    None
}

/// Chain-shape metadata for one decoded archive record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMeta {
    /// Height the record claims.
    pub index: u32,
    /// Hash of this block, computed while the payload was still owned here.
    pub hash: UInt256,
    /// Predecessor hash the record names.
    pub prev_hash: UInt256,
    /// Transactions in this block.
    pub transaction_count: usize,
}

/// Rejects a run that is not a contiguous, predecessor-linked chain.
///
/// `parent` is the hash of the last accepted block. `None` means the run's first
/// block names a tip this process cannot read back, so only the links inside the
/// run are checked and the header validator remains responsible for the first
/// block, exactly as it is on the single-block path.
pub fn validate_contiguous_run(
    parent: Option<UInt256>,
    blocks: &[BlockMeta],
) -> Result<(), String> {
    let Some(first) = blocks.first() else {
        return Err("empty-block run contains no block".to_string());
    };
    if let Some(parent) = parent
        && first.prev_hash != parent
    {
        return Err(format!(
            "block {} does not extend the accepted tip: prev_hash {} does not match {}",
            first.index, first.prev_hash, parent
        ));
    }

    let mut expected_index = first.index;
    let mut expected_prev = None;
    for block in blocks {
        if block.index != expected_index {
            return Err(format!(
                "run is not contiguous: expected block {expected_index}, got {}",
                block.index
            ));
        }
        if let Some(expected_prev) = expected_prev
            && block.prev_hash != expected_prev
        {
            return Err(format!(
                "block {} does not extend its predecessor: prev_hash {} does not match {}",
                block.index, block.prev_hash, expected_prev
            ));
        }
        expected_index = expected_index
            .checked_add(1)
            .ok_or_else(|| format!("run height overflow after block {}", block.index))?;
        expected_prev = Some(block.hash);
    }
    Ok(())
}

/// Rejects a run that is not a contiguous chain of transaction-free blocks.
pub fn validate_empty_run(parent: Option<UInt256>, blocks: &[BlockMeta]) -> Result<(), String> {
    if let Some(bearing) = blocks.iter().find(|block| block.transaction_count != 0) {
        return Err(format!(
            "empty-block run rejected block {} with {} transactions",
            bearing.index, bearing.transaction_count
        ));
    }
    validate_contiguous_run(parent, blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(byte: u8) -> UInt256 {
        UInt256::from_bytes(&[byte; 32]).expect("32-byte hash")
    }

    /// Builds a linked run of empty blocks starting at `first_index`.
    fn empty_run(first_index: u32, len: usize) -> Vec<BlockMeta> {
        let mut blocks = Vec::with_capacity(len);
        let mut prev_hash = hash_of(0xAB);
        for offset in 0..len {
            let index = first_index + u32::try_from(offset).expect("test height");
            let hash = hash_of(u8::try_from(offset).expect("test offset") + 1);
            blocks.push(BlockMeta {
                index,
                hash,
                prev_hash,
                transaction_count: 0,
            });
            prev_hash = hash;
        }
        blocks
    }

    fn test_policy(cap: usize, flush: u64, progress: u64) -> ImportDrainPolicy {
        ImportDrainPolicy::new(cap, flush, progress, 60)
    }

    #[test]
    fn disabled_batching_persists_every_block_alone() {
        let policy = test_policy(0, 10_000, 1_000);
        assert!(!policy.batching_enabled());
        assert_eq!(
            empty_run_drain_trigger(1, 0, 1, None, false, policy),
            Some(DrainTrigger::BatchingDisabled)
        );
        // A cap of one is not a batch either.
        let tiny = test_policy(1, 10_000, 1_000);
        assert!(!tiny.batching_enabled());
        assert_eq!(
            empty_run_drain_trigger(1, 0, 1, None, false, tiny),
            Some(DrainTrigger::BatchingDisabled)
        );
        // Nothing pending never triggers anything.
        assert_eq!(empty_run_drain_trigger(0, 0, 0, None, true, policy), None);
    }

    #[test]
    fn cap_is_reached_exactly_and_the_extra_block_opens_a_new_group() {
        let policy = test_policy(4, 10_000, 100_000);

        for pending_len in 1..4 {
            assert_eq!(
                empty_run_drain_trigger(pending_len, 0, pending_len as u32, None, false, policy),
                None,
                "a run below the cap must keep accumulating at {pending_len}"
            );
        }
        assert_eq!(
            empty_run_drain_trigger(4, 0, 4, None, false, policy),
            Some(DrainTrigger::BatchCap),
            "exactly at the cap the run is persisted"
        );

        // cap + 1 blocks therefore split into two groups: [4, 1].
        let mut groups = Vec::new();
        let mut imported = 0u64;
        let mut pending = 0usize;
        for height in 1..=5u32 {
            pending += 1;
            if let Some(trigger) =
                empty_run_drain_trigger(pending, imported, height, None, false, policy)
            {
                assert_eq!(trigger, DrainTrigger::BatchCap);
                groups.push(pending);
                imported += pending as u64;
                pending = 0;
            }
        }
        if pending > 0 {
            groups.push(pending);
        }
        assert_eq!(groups, vec![4, 1]);
        assert_eq!(imported, 4);
    }

    #[test]
    fn checkpoint_boundary_truncates_the_run() {
        let policy = test_policy(4, 10, 1_000);
        // Eight blocks accepted, two more complete the tenth block checkpoint.
        assert_eq!(
            empty_run_drain_trigger(1, 8, 9, None, false, policy),
            None,
            "nine accepted blocks are not a checkpoint"
        );
        assert_eq!(
            empty_run_drain_trigger(2, 8, 10, None, false, policy),
            Some(DrainTrigger::CheckpointBoundary),
            "the run must end exactly on the checkpoint height"
        );
    }

    #[test]
    fn stop_height_truncates_the_run() {
        let policy = test_policy(4, 10_000, 100_000);
        assert_eq!(
            empty_run_drain_trigger(2, 0, 12, Some(15), false, policy),
            None,
            "a run below the stop height keeps accumulating"
        );
        assert_eq!(
            empty_run_drain_trigger(3, 0, 13, Some(13), false, policy),
            Some(DrainTrigger::StopHeight),
            "the block that reaches the stop height ends the run"
        );
    }

    #[test]
    fn progress_window_and_heartbeat_truncate_the_run() {
        let policy = test_policy(4, 10_000, 5);
        assert_eq!(
            empty_run_drain_trigger(2, 8, 10, None, false, policy),
            Some(DrainTrigger::ProgressWindow),
            "a progress boundary must be reported on the block that completes it"
        );
        assert_eq!(
            empty_run_drain_trigger(1, 1, 2, None, true, policy),
            Some(DrainTrigger::ProgressWindow),
            "a due heartbeat must not wait for the next boundary"
        );
    }

    #[test]
    fn zero_intervals_never_look_like_a_boundary() {
        let policy = test_policy(8, 0, 0);
        assert_eq!(
            empty_run_drain_trigger(3, 0, 3, None, false, policy),
            None,
            "a disabled interval must not panic on a remainder"
        );
    }

    #[test]
    fn run_validation_accepts_a_linked_chain_with_a_known_and_unknown_parent() {
        let run = empty_run(5, 3);
        validate_empty_run(Some(hash_of(0xAB)), &run).expect("linked chain with known parent");
        validate_empty_run(None, &run).expect("linked chain without an observable tip");
    }

    #[test]
    fn run_validation_rejects_a_broken_predecessor_link() {
        let mut run = empty_run(5, 3);
        run[2].prev_hash = hash_of(0xEE);

        let error = validate_empty_run(Some(hash_of(0xAB)), &run).expect_err("broken link");
        assert!(error.contains("does not extend its predecessor"), "{error}");
        assert!(error.contains("block 7"), "{error}");
    }

    #[test]
    fn run_validation_rejects_a_first_block_that_does_not_extend_the_tip() {
        let run = empty_run(5, 3);
        let error =
            validate_empty_run(Some(hash_of(0xAB)), &[run[1], run[2]]).expect_err("wrong tip");
        assert!(
            error.contains("does not extend the accepted tip"),
            "{error}"
        );
    }

    #[test]
    fn run_validation_rejects_gaps_and_transaction_bearing_blocks() {
        let mut run = empty_run(5, 3);
        run[1].index = 9;
        let error = validate_empty_run(None, &run).expect_err("gap");
        assert!(error.contains("not contiguous"), "{error}");

        let mut run = empty_run(5, 2);
        run[1].transaction_count = 1;
        let error = validate_empty_run(None, &run).expect_err("transactions");
        assert!(error.contains("empty-block run rejected"), "{error}");
        assert!(
            validate_empty_run(None, &[])
                .expect_err("empty run")
                .contains("no block")
        );
    }

    #[test]
    fn configured_batch_size_is_clamped_to_what_persistence_accepts() {
        assert_eq!(clamp_empty_batch_size(0), (0, false));
        assert_eq!(clamp_empty_batch_size(256), (256, false));
        assert_eq!(
            clamp_empty_batch_size(MAX_EMPTY_BATCH_SIZE + 1),
            (MAX_EMPTY_BATCH_SIZE, true)
        );
    }

    /// End-to-end planning of a small archive: every group stays within the cap,
    /// groups are contiguous, and each checkpoint multiple ends a group.
    #[test]
    fn planned_groups_never_cross_a_checkpoint_and_stay_within_the_cap() {
        let policy = test_policy(4, 10, 1_000);
        let mut groups = Vec::new();
        let mut imported = 0u64;
        let mut pending = 0usize;

        for height in 1..=25u32 {
            pending += 1;
            if let Some(trigger) =
                empty_run_drain_trigger(pending, imported, height, None, false, policy)
            {
                groups.push((imported, pending, trigger));
                imported += pending as u64;
                pending = 0;
            }
        }
        if pending > 0 {
            groups.push((imported, pending, DrainTrigger::EndOfArchive));
            imported += pending as u64;
        }

        assert_eq!(imported, 25);
        let mut expected_start = 0u64;
        for (start, len, trigger) in &groups {
            let end = start + *len as u64;
            assert_eq!(*start, expected_start, "groups must stay contiguous");
            assert!(
                *len <= policy.empty_batch_cap,
                "group {start}..{end} exceeds the cap"
            );
            assert!(
                !end.is_multiple_of(policy.flush_interval)
                    || *trigger == DrainTrigger::CheckpointBoundary,
                "group {start}..{end} crossed a checkpoint without the matching trigger"
            );
            expected_start = end;
        }
        assert_eq!(expected_start, 25);
        assert_eq!(groups.iter().map(|(_, len, _)| *len).sum::<usize>(), 25);
        assert!(
            groups
                .iter()
                .any(|(_, _, trigger)| *trigger == DrainTrigger::CheckpointBoundary),
            "the simulation must reach at least one checkpoint boundary"
        );
    }
}
