//! Per-peer bloom filter state and filtered-block matching.
//!
//! Mirrors `neo-core::network::p2p::remote_node::bloom_filter`. A peer may load
//! a bloom filter (`FilterLoad`), extend it (`FilterAdd`), or clear it
//! (`FilterClear`); when the peer then requests blocks, only transactions
//! matching the filter are included in a [`MerkleBlockPayload`].
//!
//! # Dependency substitutions
//!
//! The `neo-core` original rate-limits filter operations with the `governor`
//! crate and reports rejections through `tracing`. Neither is in this crate's
//! allowed dependency set, so:
//!
//! * the rate limiter is a hand-written fixed-window token counter over
//!   `std::time::Instant` that reproduces the "burst then reject" behaviour;
//! * every rejection path is surfaced as a [`FilterLoadOutcome`] /
//!   [`FilterAddOutcome`] value the caller can log, instead of logging here.

use super::payloads::{FilterAddPayload, FilterLoadPayload};
use neo_crypto::BloomFilter;
use neo_protocol::payloads::{Block, Transaction};
use std::time::{Duration, Instant};

/// Maximum bloom filter size in bytes (matches C# `MAX_FILTER_SIZE = 36000`).
pub const MAX_BLOOM_FILTER_SIZE: usize = 36000;
/// Maximum number of hash functions for a bloom filter.
pub const MAX_BLOOM_K: u8 = 50;
/// Maximum bloom filter operations per minute to prevent DoS attacks.
pub const MAX_FILTER_OPS_PER_MINUTE: u32 = 100;
/// Length of the fixed rate-limit window.
const FILTER_RATE_WINDOW: Duration = Duration::from_secs(60);

/// Result of a [`BloomFilterState::on_filter_load`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterLoadOutcome {
    /// A valid filter was installed.
    Loaded,
    /// The payload was empty or had `k == 0`; any existing filter was cleared.
    Cleared,
    /// The operation was rejected by the per-minute rate limit.
    RateLimited,
    /// The filter byte array exceeded [`MAX_BLOOM_FILTER_SIZE`].
    TooLarge,
    /// The hash-function count exceeded [`MAX_BLOOM_K`].
    TooManyHashFunctions,
    /// The filter could not be reconstructed from the payload bytes.
    Invalid,
}

/// Result of a [`BloomFilterState::on_filter_add`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAddOutcome {
    /// The element was added to the loaded filter.
    Added,
    /// No filter is currently loaded, so the element was ignored.
    NoFilter,
    /// The operation was rejected by the per-minute rate limit.
    RateLimited,
}

/// Fixed-window token counter standing in for `governor`'s rate limiter.
///
/// Allows up to [`MAX_FILTER_OPS_PER_MINUTE`] operations per
/// [`FILTER_RATE_WINDOW`]; once the window elapses the budget resets.
#[derive(Debug)]
struct FilterRateLimiter {
    window_start: Instant,
    used: u32,
}

impl FilterRateLimiter {
    fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            used: 0,
        }
    }

    /// Consumes one token, returning `false` when the window budget is spent.
    fn check(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_start) >= FILTER_RATE_WINDOW {
            self.window_start = now;
            self.used = 0;
        }
        if self.used >= MAX_FILTER_OPS_PER_MINUTE {
            return false;
        }
        self.used += 1;
        true
    }
}

/// Per-peer bloom filter state with DoS rate limiting.
#[derive(Debug)]
pub struct BloomFilterState {
    filter: Option<BloomFilter>,
    limiter: FilterRateLimiter,
}

impl Default for BloomFilterState {
    fn default() -> Self {
        Self {
            filter: None,
            limiter: FilterRateLimiter::new(Instant::now()),
        }
    }
}

impl BloomFilterState {
    /// Returns a reference to the loaded filter, if any (test/diagnostic hook).
    pub fn filter(&self) -> Option<&BloomFilter> {
        self.filter.as_ref()
    }

    /// Whether a filter is currently loaded.
    pub fn is_loaded(&self) -> bool {
        self.filter.is_some()
    }

    /// Clears any loaded filter (`FilterClear`).
    pub fn clear(&mut self) {
        self.filter = None;
    }

    /// Installs a filter from a `FilterLoad` payload.
    pub fn on_filter_load(&mut self, payload: &FilterLoadPayload) -> FilterLoadOutcome {
        if !self.limiter.check(Instant::now()) {
            return FilterLoadOutcome::RateLimited;
        }

        if payload.filter.is_empty() || payload.k == 0 {
            self.filter = None;
            return FilterLoadOutcome::Cleared;
        }

        if payload.filter.len() > MAX_BLOOM_FILTER_SIZE {
            self.filter = None;
            return FilterLoadOutcome::TooLarge;
        }

        if payload.k > MAX_BLOOM_K {
            self.filter = None;
            return FilterLoadOutcome::TooManyHashFunctions;
        }

        let bit_size = payload.filter.len() * 8;
        match BloomFilter::with_bits(bit_size, payload.k as usize, payload.tweak, &payload.filter) {
            Ok(filter) => {
                self.filter = Some(filter);
                FilterLoadOutcome::Loaded
            }
            Err(_) => {
                self.filter = None;
                FilterLoadOutcome::Invalid
            }
        }
    }

    /// Adds an element to the loaded filter (`FilterAdd`).
    pub fn on_filter_add(&mut self, payload: &FilterAddPayload) -> FilterAddOutcome {
        if !self.limiter.check(Instant::now()) {
            return FilterAddOutcome::RateLimited;
        }

        match self.filter.as_mut() {
            Some(filter) => {
                filter.add(&payload.data);
                FilterAddOutcome::Added
            }
            None => FilterAddOutcome::NoFilter,
        }
    }

    /// Computes per-transaction match flags for a block, or `None` when no
    /// filter is loaded (meaning "send the full block").
    pub fn flags(&self, block: &Block) -> Option<Vec<bool>> {
        let filter = self.filter.as_ref()?;
        Some(
            block
                .transactions
                .iter()
                .map(|tx| Self::matches_transaction(filter, tx))
                .collect(),
        )
    }

    /// Whether a transaction matches the filter by hash or any signer account.
    fn matches_transaction(filter: &BloomFilter, tx: &Transaction) -> bool {
        let hash = match tx.try_hash() {
            Ok(hash) => hash,
            Err(_) => return false,
        };
        if filter.check(&hash.to_array()) {
            return true;
        }

        tx.signers()
            .iter()
            .any(|signer| filter.check(&signer.account.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_matching(data: &[u8]) -> FilterLoadPayload {
        let mut filter = BloomFilter::new(256, 3, 0xCAFE_BABE).expect("filter");
        filter.add(data);
        FilterLoadPayload::create_from_bloom_filter(&filter)
    }

    #[test]
    fn filter_load_accepts_valid_filter_payload() {
        let data = b"contract-hash";
        let mut state = BloomFilterState::default();

        assert_eq!(
            state.on_filter_load(&payload_matching(data)),
            FilterLoadOutcome::Loaded
        );
        assert!(state.filter().expect("loaded filter").check(data));
    }

    #[test]
    fn filter_load_empty_or_zero_hash_functions_clears_filter() {
        let mut state = BloomFilterState::default();
        state.on_filter_load(&payload_matching(b"existing"));

        assert_eq!(
            state.on_filter_load(&FilterLoadPayload::new(Vec::new(), 3, 0)),
            FilterLoadOutcome::Cleared
        );
        assert!(!state.is_loaded());

        state.on_filter_load(&payload_matching(b"existing"));
        assert_eq!(
            state.on_filter_load(&FilterLoadPayload::new(vec![0xFF], 0, 0)),
            FilterLoadOutcome::Cleared
        );
        assert!(!state.is_loaded());
    }

    #[test]
    fn filter_load_rejects_oversized_or_excessive_hash_function_payloads() {
        let mut state = BloomFilterState::default();

        assert_eq!(
            state.on_filter_load(&FilterLoadPayload::new(
                vec![0; MAX_BLOOM_FILTER_SIZE + 1],
                3,
                0
            )),
            FilterLoadOutcome::TooLarge
        );
        assert!(!state.is_loaded());

        assert_eq!(
            state.on_filter_load(&FilterLoadPayload::new(vec![0xFF], MAX_BLOOM_K + 1, 0)),
            FilterLoadOutcome::TooManyHashFunctions
        );
        assert!(!state.is_loaded());
    }

    #[test]
    fn filter_add_updates_loaded_filter() {
        let data = b"new-element";
        let mut state = BloomFilterState::default();
        state.on_filter_load(&FilterLoadPayload::new(vec![0; 32], 3, 0));

        assert_eq!(
            state.on_filter_add(&FilterAddPayload::new(data.to_vec())),
            FilterAddOutcome::Added
        );
        assert!(state.filter().expect("loaded filter").check(data));
    }

    #[test]
    fn filter_add_without_loaded_filter_is_noop() {
        let mut state = BloomFilterState::default();

        assert_eq!(
            state.on_filter_add(&FilterAddPayload::new(b"new-element".to_vec())),
            FilterAddOutcome::NoFilter
        );
        assert!(!state.is_loaded());
    }

    #[test]
    fn filter_clear_removes_loaded_filter() {
        let mut state = BloomFilterState::default();
        state.on_filter_load(&payload_matching(b"existing"));

        state.clear();

        assert!(!state.is_loaded());
    }

    #[test]
    fn filter_rate_limit_allows_initial_burst_then_rejects() {
        let mut state = BloomFilterState::default();

        for _ in 0..MAX_FILTER_OPS_PER_MINUTE {
            assert!(state.limiter.check(state.limiter.window_start));
        }
        assert!(!state.limiter.check(state.limiter.window_start));
    }

    #[test]
    fn filter_rate_limit_resets_after_window() {
        let start = Instant::now();
        let mut limiter = FilterRateLimiter::new(start);
        for _ in 0..MAX_FILTER_OPS_PER_MINUTE {
            assert!(limiter.check(start));
        }
        assert!(!limiter.check(start));
        // After the window elapses the budget is restored.
        assert!(limiter.check(start + FILTER_RATE_WINDOW));
    }

    #[test]
    fn bloom_filter_flags_return_none_without_loaded_filter() {
        let block = Block::new();
        let state = BloomFilterState::default();

        assert!(state.flags(&block).is_none());
    }
}
