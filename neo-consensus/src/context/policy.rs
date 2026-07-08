//! dBFT context defaults and bounded-cache policy.
//!
//! These constants are protocol/service defaults used while building or
//! restoring a [`ConsensusContext`](super::ConsensusContext). Runtime policy
//! updates still flow through the service and context accessors.

/// Default block time in milliseconds (15 seconds for Neo N3).
/// Post-Echidna, `MillisecondsPerBlock` is a committee-configurable policy
/// setting. Use this only as a fallback when no policy value is available.
pub const DEFAULT_BLOCK_TIME_MS: u64 = 15_000;

/// Backwards-compatible alias (deprecated - prefer `DEFAULT_BLOCK_TIME_MS`).
pub const BLOCK_TIME_MS: u64 = DEFAULT_BLOCK_TIME_MS;

/// Maximum validators in dBFT.
pub const MAX_VALIDATORS: usize = 21;

/// C# DBFTPlugin `DbftSettings.MaxBlockSize` — the block-size policy a backup
/// enforces in `CheckPrepareResponse` before sending its `PrepareResponse`.
/// The DBFTPlugin ships `MaxBlockSize = 2097152` (2 MiB) in `DBFTPlugin.json`,
/// which matches `neo_primitives::constants::MAX_BLOCK_SIZE` and the value the
/// primary already enforces during `EnsureMaxBlockLimitation`.
pub const DEFAULT_MAX_BLOCK_SIZE: u32 = 2_097_152;

/// C# DBFTPlugin `DbftSettings.MaxBlockSystemFee` default (150000000000, i.e.
/// 1500 GAS). The block-system-fee policy a backup enforces in
/// `CheckPrepareResponse`, identical to the limit the primary applies in
/// `EnsureMaxBlockLimitation`.
pub const DEFAULT_MAX_BLOCK_SYSTEM_FEE: i64 = 150_000_000_000;

/// Maximum size of message hash cache (LRU limit for memory protection).
/// Matches C# `DBFTPlugin`'s message caching behavior.
pub const MAX_MESSAGE_CACHE_SIZE: usize = 10_000;

pub(super) fn effective_block_time(block_time_ms: Option<u64>) -> u64 {
    match block_time_ms {
        Some(t) if t > 0 => t,
        _ => DEFAULT_BLOCK_TIME_MS,
    }
}
