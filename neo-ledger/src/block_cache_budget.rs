//! Shared retained-bytes budget for the node's in-memory block caches.
//!
//! A node keeps full blocks in memory in three places: the persisted block
//! body cache of [`LedgerContext`](crate::LedgerContext), and the verified and
//! unverified block caches of the blockchain actor. Each used to be bounded on
//! its own, mostly by entry count, so their combined footprint was only
//! limited by the block size a peer can send. They now charge one
//! [`BlockCacheBudget`], and the bytes they retain together never exceed its
//! limit.
//!
//! # Metric
//!
//! Every cache charges the same cost for a block: its wire-serialized size
//! plus [`BLOCK_CACHE_ENTRY_OVERHEAD`]. The cost is computed once when the
//! block is admitted and travels with the entry. It approximates, and for
//! blocks made of many tiny transactions underestimates, the heap footprint.
//!
//! # Accounting
//!
//! Bytes enter the budget only through [`BlockCacheBudget::try_reserve`] and
//! leave it only when the returned [`BlockCacheReservation`] is dropped. A
//! cache stores the reservation next to the block it pays for, so every way an
//! entry can leave a cache (eviction, removal after persistence, an error
//! path, dropping the cache) returns its bytes.
//!
//! The budget never evicts anything itself. Which entry gives way under
//! pressure is decided by the caches; see `neo-system/src/blockchain`.

use neo_io::Serializable;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Default limit shared by all block caches of one node.
///
/// Twice the 256 MiB cap the unverified block cache has always had: that cap
/// still bounds what unverified blocks may take, and the other half is left
/// for verified blocks awaiting persistence and for persisted block bodies.
/// It holds the full 2048-entry body cache for blocks averaging up to about
/// 125 KiB.
pub const DEFAULT_BLOCK_CACHE_BUDGET_BYTES: usize = 512 * 1024 * 1024;

/// Per-entry bookkeeping charged on top of the serialized size, so that very
/// small blocks are not accounted as free.
pub const BLOCK_CACHE_ENTRY_OVERHEAD: usize = 64;

/// Budget cost of retaining a block whose wire encoding is `serialized_size`
/// bytes.
pub const fn block_cache_cost(serialized_size: usize) -> usize {
    serialized_size.saturating_add(BLOCK_CACHE_ENTRY_OVERHEAD)
}

/// Budget cost of retaining a payload block.
pub fn payload_block_cache_cost(block: &neo_protocol::payloads::block::Block) -> usize {
    block_cache_cost(Serializable::size(block))
}

/// Budget cost of retaining an execution block.
///
/// Equal to [`payload_block_cache_cost`] of its payload projection for every
/// block with exactly one header witness, without cloning the block.
pub fn execution_block_cache_cost(block: &neo_protocol::execution::Block) -> usize {
    block_cache_cost(block.size())
}

/// Byte budget shared by the block caches of one node.
#[derive(Debug)]
pub struct BlockCacheBudget {
    limit: usize,
    retained: AtomicUsize,
}

impl BlockCacheBudget {
    /// Creates a budget that allows at most `limit` retained bytes.
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            retained: AtomicUsize::new(0),
        })
    }

    /// Creates a budget with [`DEFAULT_BLOCK_CACHE_BUDGET_BYTES`].
    pub fn with_default_limit() -> Arc<Self> {
        Self::new(DEFAULT_BLOCK_CACHE_BUDGET_BYTES)
    }

    /// Maximum number of bytes the caches may retain together.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Bytes currently reserved.
    pub fn retained(&self) -> usize {
        self.retained.load(Ordering::Acquire)
    }

    /// Bytes that can still be reserved.
    pub fn available(&self) -> usize {
        self.limit.saturating_sub(self.retained())
    }

    /// Reserves `bytes`, or returns `None` when they do not fit.
    ///
    /// The addition is checked: a request that would overflow `usize` or pass
    /// the limit is refused and leaves the budget untouched, so the retained
    /// total can never wrap around to a small value. A request larger than
    /// the whole limit is therefore always refused.
    pub fn try_reserve(self: &Arc<Self>, bytes: usize) -> Option<BlockCacheReservation> {
        let mut current = self.retained.load(Ordering::Acquire);
        loop {
            let total = current
                .checked_add(bytes)
                .filter(|total| *total <= self.limit)?;
            match self.retained.compare_exchange_weak(
                current,
                total,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(BlockCacheReservation {
                        budget: Arc::clone(self),
                        bytes,
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, bytes: usize) {
        // Only a reservation releases, and it releases exactly what it
        // reserved, so this cannot underflow. Saturating keeps `Drop` free of
        // panics and of wrap-around should that invariant ever be broken.
        let mut current = self.retained.load(Ordering::Acquire);
        loop {
            match self.retained.compare_exchange_weak(
                current,
                current.saturating_sub(bytes),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

/// Bytes reserved from a [`BlockCacheBudget`], returned to it on drop.
#[derive(Debug)]
pub struct BlockCacheReservation {
    budget: Arc<BlockCacheBudget>,
    bytes: usize,
}

impl BlockCacheReservation {
    /// Number of bytes this reservation holds.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for BlockCacheReservation {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn reservation_charges_and_drop_returns_bytes() {
        let budget = BlockCacheBudget::new(100);
        let first = budget.try_reserve(60).expect("fits");
        assert_eq!(first.bytes(), 60);
        assert_eq!(budget.retained(), 60);
        assert_eq!(budget.available(), 40);

        assert!(budget.try_reserve(41).is_none(), "would exceed the limit");
        assert_eq!(budget.retained(), 60, "a refused request charges nothing");

        let second = budget.try_reserve(40).expect("fits exactly");
        assert_eq!(budget.retained(), 100);
        assert_eq!(budget.available(), 0);

        drop(first);
        assert_eq!(budget.retained(), 40);
        drop(second);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn request_larger_than_limit_is_refused_without_overflow() {
        let budget = BlockCacheBudget::new(100);
        assert!(budget.try_reserve(101).is_none());
        assert!(budget.try_reserve(usize::MAX).is_none());

        // `retained + bytes` overflows `usize` here; it must be refused, not
        // wrapped into a total that looks small.
        let held = budget.try_reserve(1).expect("fits");
        assert!(budget.try_reserve(usize::MAX).is_none());
        assert_eq!(budget.retained(), 1);
        drop(held);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn unlimited_budget_refuses_overflowing_total() {
        let budget = BlockCacheBudget::new(usize::MAX);
        let held = budget.try_reserve(usize::MAX - 1).expect("fits");
        assert!(budget.try_reserve(2).is_none());
        let last = budget.try_reserve(1).expect("fits exactly");
        assert_eq!(budget.retained(), usize::MAX);
        drop(held);
        drop(last);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn zero_limit_budget_only_admits_empty_reservations() {
        let budget = BlockCacheBudget::new(0);
        assert!(budget.try_reserve(1).is_none());
        let empty = budget.try_reserve(0).expect("zero bytes always fit");
        assert_eq!(budget.retained(), 0);
        drop(empty);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn block_cost_saturates_instead_of_overflowing() {
        assert_eq!(block_cache_cost(0), BLOCK_CACHE_ENTRY_OVERHEAD);
        assert_eq!(block_cache_cost(usize::MAX), usize::MAX);
    }

    #[test]
    fn payload_and_execution_blocks_cost_the_same() {
        let mut payload = neo_protocol::payloads::block::Block::new();
        payload.header.set_index(7);
        let mut transaction = neo_protocol::payloads::transaction::Transaction::new();
        transaction.set_script(vec![0x21; 300]);
        payload.transactions = vec![transaction];
        let execution = neo_protocol::execution::Block::from(&payload);

        assert_eq!(
            payload_block_cache_cost(&payload),
            execution_block_cache_cost(&execution)
        );
        assert!(payload_block_cache_cost(&payload) > 300 + BLOCK_CACHE_ENTRY_OVERHEAD);
    }

    #[test]
    fn concurrent_reservations_never_exceed_limit_and_all_return() {
        const LIMIT: usize = 1_000;
        const THREADS: usize = 8;
        const ROUNDS: usize = 20_000;

        let budget = BlockCacheBudget::new(LIMIT);
        let stop = Arc::new(AtomicBool::new(false));

        let observer = {
            let budget = Arc::clone(&budget);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut peak = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    peak = peak.max(budget.retained());
                }
                peak
            })
        };

        let workers: Vec<_> = (0..THREADS)
            .map(|worker| {
                let budget = Arc::clone(&budget);
                std::thread::spawn(move || {
                    let mut held = Vec::new();
                    let mut granted = 0usize;
                    for round in 0..ROUNDS {
                        // Sizes from 1 to 400: several workers always compete
                        // for the last bytes of the budget.
                        let bytes = 1 + (worker * 131 + round * 17) % 400;
                        if let Some(reservation) = budget.try_reserve(bytes) {
                            granted += 1;
                            held.push(reservation);
                        }
                        assert!(budget.retained() <= LIMIT);
                        if held.len() > 2 || round % 3 == 0 {
                            held.pop();
                        }
                    }
                    granted
                })
            })
            .collect();

        let granted: usize = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker"))
            .sum();
        stop.store(true, Ordering::Relaxed);
        let peak = observer.join().expect("observer");

        assert!(granted > 0, "the workers must have exercised the budget");
        assert!(peak <= LIMIT, "observed {peak} retained bytes");
        assert_eq!(budget.retained(), 0, "every reservation must be returned");
    }
}
