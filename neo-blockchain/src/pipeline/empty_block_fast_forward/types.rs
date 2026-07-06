/// Upper bound for a single empty-block fast-forward batch.
///
/// This is a memory/fairness guard, not a throughput target. Mainnet empty
/// bursts are normally short, while every fast-forwarded height still writes
/// ledger history and native state effects. Long synthetic runs are chunked into
/// bounded bursts so staged cache publication stays predictable.
pub const MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS: usize = 128;

/// Eligible contiguous empty-block interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyBlockFastForwardPlan {
    /// First block height in the interval.
    pub start: u32,
    /// Last block height in the interval.
    pub end: u32,
    /// Number of blocks in the interval.
    pub block_count: usize,
}
