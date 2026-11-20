//! Retention policy for the node's two in-memory block caches.
//!
//! A node keeps full blocks in memory in three places: the persisted block
//! body cache of [`LedgerContext`](crate::LedgerContext), and the verified and
//! unverified block caches of the blockchain actor. All three charge the same
//! [`BlockCacheBudget`], so the three together never retain more bytes than its
//! limit. A block held by both actor caches is charged once.
//!
//! Which entry gives way when a block does not fit is a ledger-domain decision:
//! it depends on the persisted height and on how the chain consumes blocks, not
//! on how the actor passes messages. It therefore lives here, behind
//! [`BlockCachePolicy`], and the actor only calls it.
//!
//! # Why the order below is the right one
//!
//! The caches only save a download: every block taken from them is verified
//! again before it is persisted, and a block that is not cached is fetched
//! again. Giving up a cached block therefore never changes what is valid or
//! the order in which blocks are persisted.
//!
//! When a block does not fit, room is made in this order:
//!
//! 1. cached blocks at or below the persisted height, which are of no use;
//! 2. persisted block bodies in the ledger context, oldest first, which the
//!    store can still serve;
//! 3. cached blocks at heights above the incoming block, highest first.
//!
//! Admission is all or nothing. Before anything is given up, the policy works
//! out whether the free room and everything the incoming block may displace
//! are enough for it together. If they are not, the block is refused and
//! every cache is left as it was, the blocks of step 1 included. The same
//! holds for the caps of the unverified cache and for the candidate a block
//! replaces at a height that already holds as many as it may. A block that
//! cannot be admitted therefore costs the node nothing it had cached.
//!
//! The budget never makes a block give way to one at the same or a greater
//! height. The chain needs the lowest missing height next, and blocks nobody
//! can verify yet sit above every height a verified header exists for, so
//! filling the budget with far-future blocks cannot keep a block the chain
//! needs sooner out.
//!
//! The next block to persist does not depend on admission at all: it is
//! persisted even when the budget has no room for it or it is larger than the
//! whole budget, it is just not retained, and nothing is given up for it. Any
//! other block that cannot be admitted is not kept.

use crate::block_cache_budget::{
    BlockCacheBudget, BlockCacheReservation, execution_block_cache_cost,
};
use dashmap::DashMap;
use neo_primitives::UInt256;
use neo_protocol::execution::Block;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::warn;

/// Maximum number of verified blocks to keep in the block cache.
/// Prevents unbounded memory growth from out-of-order or attacker-injected blocks.
/// Sized to accommodate the fast sync download window without backpressure.
pub const MAX_BLOCK_CACHE_SIZE: usize = 20000;

/// Maximum number of blocks retained in the unverified block cache.
/// This is a block count, not a height count: a malicious peer can otherwise
/// fill one height with arbitrarily many competing blocks.
pub const MAX_UNVERIFIED_CACHE_SIZE: usize = 20_000;

/// Maximum number of competing blocks retained for a single height.
///
/// The consensus protocol can legitimately produce a small number of fork
/// candidates at the same height. Keeping more than this is only useful to an
/// attacker trying to exhaust memory with unsolicited blocks, because none of
/// them can be persisted until their height is verified against a cached
/// header.
pub const MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT: usize = 3;

/// Hard cap on the approximate serialized size retained by the unverified
/// block cache. The entry/candidate limits above are not sufficient on their
/// own: a P2P payload may be up to 32 MiB, so 20k cached blocks could pin
/// hundreds of gigabytes of RAM. 256 MiB covers the fast-sync window while
/// bounding worst-case memory under byzantine peers.
///
/// This is a share of the block cache budget, not an addition to it: blocks
/// in the unverified cache are charged to the budget as well.
pub const MAX_UNVERIFIED_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// The persisted block bodies a cache policy may reclaim, as it needs them.
///
/// The bodies cached by the ledger context are a fast path only: the store can
/// still serve every one of them, so they are given up before any block that is
/// still waiting to be persisted.
pub trait PersistedBodyCache {
    /// Height of the persisted chain tip.
    fn persisted_height(&self) -> u32;

    /// Bytes the persisted block-body cache currently holds.
    fn cached_body_bytes(&self) -> usize;

    /// Gives up the oldest cached bodies until at least `bytes` were returned
    /// to the budget, or none are left.
    fn evict_oldest_bodies(&self, bytes: usize);
}

impl PersistedBodyCache for crate::LedgerContext {
    fn persisted_height(&self) -> u32 {
        self.current_height()
    }

    fn cached_body_bytes(&self) -> usize {
        self.cached_block_bytes()
    }

    fn evict_oldest_bodies(&self, bytes: usize) {
        self.evict_block_bodies(bytes);
    }
}

/// A block held by the verified block cache, the unverified block cache, or
/// both, together with the bytes it holds in the block cache budget.
///
/// A verified block that cannot be persisted yet sits in both caches. The two
/// entries are clones sharing one reservation, so the block is charged once
/// and its bytes are returned when the last of them is dropped.
#[derive(Clone)]
pub struct RetainedBlock {
    // Declared before the reservation so the block is released before its
    // bytes are returned to the budget.
    /// The retained block.
    pub block: Arc<Block>,
    /// Hash of [`Self::block`].
    pub hash: UInt256,
    /// Budget cost of the block, computed once when it was admitted.
    pub cost: usize,
    reservation: Arc<BlockCacheReservation>,
}

impl RetainedBlock {
    /// Charges `block` to `budget` and returns the entry, or `None` when the
    /// bytes do not fit as things stand.
    fn charge(
        block: Arc<Block>,
        hash: UInt256,
        cost: usize,
        budget: &Arc<BlockCacheBudget>,
    ) -> Option<Self> {
        let reservation = budget.try_reserve(cost)?;
        Some(Self {
            block,
            hash,
            cost,
            reservation: Arc::new(reservation),
        })
    }

    /// Identity of the reservation this entry is charged to. Two entries that
    /// report the same value share one charge.
    pub fn reservation_id(&self) -> usize {
        Arc::as_ptr(&self.reservation).addr()
    }

    /// Bytes this entry holds in the budget; always [`Self::cost`].
    pub fn reserved_bytes(&self) -> usize {
        self.reservation.bytes()
    }
}

/// Retention policy of the verified and unverified block caches.
///
/// Owns both caches, their byte accounting and every decision about which
/// entry gives way for an incoming block. Callers hand in the persisted chain
/// as [`PersistedBodyCache`] and get verbatim verdicts; they never decide
/// retention themselves.
pub struct BlockCachePolicy {
    budget: Arc<BlockCacheBudget>,
    verified: DashMap<UInt256, RetainedBlock>,
    unverified: DashMap<u32, Vec<RetainedBlock>>,
    /// Total budget cost of the blocks currently held in `unverified`.
    /// Guarded by atomics because the cache is shared through `Arc` even
    /// though the actor calls into the policy serially.
    unverified_bytes: AtomicUsize,
    /// Total number of blocks currently held in `unverified`.
    unverified_blocks: AtomicUsize,
}

impl BlockCachePolicy {
    /// Creates a policy over `budget`, the budget both caches share with the
    /// block body cache of the ledger context.
    pub fn new(budget: Arc<BlockCacheBudget>) -> Self {
        Self {
            budget,
            verified: DashMap::with_capacity(1024),
            unverified: DashMap::with_capacity(256),
            unverified_bytes: AtomicUsize::new(0),
            unverified_blocks: AtomicUsize::new(0),
        }
    }

    /// Budget both caches share with the block body cache of the ledger
    /// context.
    pub fn budget(&self) -> &Arc<BlockCacheBudget> {
        &self.budget
    }

    // ------------------------------------------------------------------
    // Verified cache
    // ------------------------------------------------------------------

    /// Number of blocks the verified cache holds.
    pub fn verified_len(&self) -> usize {
        self.verified.len()
    }

    /// Whether the verified cache holds nothing.
    pub fn verified_is_empty(&self) -> bool {
        self.verified.is_empty()
    }

    /// Whether the verified cache holds `hash`.
    pub fn contains_verified(&self, hash: &UInt256) -> bool {
        self.verified.contains_key(hash)
    }

    /// Snapshot of every block the verified cache holds.
    pub fn verified_blocks(&self) -> Vec<RetainedBlock> {
        self.verified
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Stores `entry` in the verified cache, keyed by its hash. An entry
    /// already stored under that hash is replaced, so the bytes it holds are
    /// returned unless the new entry shares its reservation.
    pub fn insert_verified(&self, entry: RetainedBlock) {
        self.verified.insert(entry.hash, entry);
    }

    /// Removes the entry for `hash`, if any, and returns it.
    pub fn remove_verified(&self, hash: &UInt256) -> Option<RetainedBlock> {
        self.verified.remove(hash).map(|(_, retained)| retained)
    }

    /// Empties the verified cache.
    pub fn clear_verified(&self) {
        self.verified.clear();
    }

    /// Finds the block cached at `index`, taking it out of the verified cache.
    ///
    /// This is a linear scan but only runs when the unverified cache misses,
    /// which is rare during normal operation.
    pub fn take_verified_at_index(&self, index: u32) -> Option<RetainedBlock> {
        // The scan is finished, and its hold on the map released, before the
        // entry is removed.
        let hash = self
            .verified
            .iter()
            .find(|entry| entry.value().block.index() == index)
            .map(|entry| *entry.key())?;
        self.remove_verified(&hash)
    }

    /// Charges `block` to the block cache budget if it fits as things stand,
    /// without making room for it.
    pub fn try_retain(&self, block: Arc<Block>, hash: UInt256) -> Option<RetainedBlock> {
        let cost = execution_block_cache_cost(block.as_ref());
        RetainedBlock::charge(block, hash, cost, &self.budget)
    }

    /// Charges `block` to the block cache budget, making room for it as
    /// described in the module documentation. Returns `None` when the block
    /// must not be retained.
    pub fn retain(
        &self,
        block: Arc<Block>,
        hash: UInt256,
        bodies: &dyn PersistedBodyCache,
    ) -> Option<RetainedBlock> {
        let cost = execution_block_cache_cost(block.as_ref());
        let reservation = self.reserve_block_bytes(block.index(), cost, bodies)?;
        Some(RetainedBlock {
            block,
            hash,
            cost,
            reservation: Arc::new(reservation),
        })
    }

    /// Reserves `cost` bytes for a block at height `index`, giving up cached
    /// blocks of lower priority when the budget is short. Nothing is given up
    /// unless that makes the block fit.
    fn reserve_block_bytes(
        &self,
        index: u32,
        cost: usize,
        bodies: &dyn PersistedBodyCache,
    ) -> Option<BlockCacheReservation> {
        if let Some(reservation) = self.budget.try_reserve(cost) {
            return Some(reservation);
        }
        if !self.can_make_room(index, cost, None, bodies) {
            return None;
        }

        self.evict_blocks_up_to(bodies.persisted_height());
        if let Some(reservation) = self.budget.try_reserve(cost) {
            return Some(reservation);
        }

        bodies.evict_oldest_bodies(cost.saturating_sub(self.budget.available()));
        if let Some(reservation) = self.budget.try_reserve(cost) {
            return Some(reservation);
        }

        for (height, hashes) in self.cached_heights_above(index).into_iter().rev() {
            self.remove_unverified_index(height);
            for hash in hashes {
                self.verified.remove(&hash);
            }
            if let Some(reservation) = self.budget.try_reserve(cost) {
                return Some(reservation);
            }
        }
        // Only reached when something outside the policy charged the budget
        // after the check above.
        None
    }

    /// Whether a block at height `index` costing `cost` bytes fits the budget
    /// once everything it may displace is given up. `replaced` is the
    /// unverified candidate at its own height whose place it would take, if
    /// any. Changes nothing.
    fn can_make_room(
        &self,
        index: u32,
        cost: usize,
        replaced: Option<UInt256>,
        bodies: &dyn PersistedBodyCache,
    ) -> bool {
        if cost > self.budget.limit() {
            // Nothing that could be given up would make this block fit.
            return false;
        }
        let room = self
            .budget
            .available()
            .saturating_add(bodies.cached_body_bytes());
        room >= cost
            || room.saturating_add(self.displaceable_bytes(index, replaced, bodies)) >= cost
    }

    /// Budget bytes that giving up every cached block a block at height
    /// `index` may displace would return: the blocks at or below the
    /// persisted height, the blocks above `index`, and the unverified
    /// candidate `replaced` at `index` itself.
    ///
    /// A block both caches hold returns its bytes only when both entries go,
    /// and is counted once.
    fn displaceable_bytes(
        &self,
        index: u32,
        replaced: Option<UInt256>,
        bodies: &dyn PersistedBodyCache,
    ) -> usize {
        let persisted_height = bodies.persisted_height();
        let gives_way = |height: u32| height <= persisted_height || height > index;

        // Bytes of each reservation, and whether every entry holding it goes.
        let mut reservations: HashMap<*const BlockCacheReservation, (usize, bool)> = HashMap::new();
        let mut note = |entry: &RetainedBlock, goes: bool| {
            let (_, all_go) = reservations
                .entry(Arc::as_ptr(&entry.reservation))
                .or_insert((entry.reservation.bytes(), true));
            *all_go &= goes;
        };
        for entry in self.verified.iter() {
            let retained = entry.value();
            note(retained, gives_way(retained.block.index()));
        }
        for entry in self.unverified.iter() {
            let height = *entry.key();
            for candidate in entry.value() {
                let is_replaced = height == index && replaced == Some(candidate.hash);
                note(candidate, is_replaced || gives_way(height));
            }
        }

        reservations
            .values()
            .filter(|(_, all_go)| *all_go)
            .map(|(bytes, _)| *bytes)
            .fold(0usize, usize::saturating_add)
    }

    /// Drops every cached block at or below `height`. Those are persisted
    /// already, or lost to the block that was, and are never looked up again.
    fn evict_blocks_up_to(&self, height: u32) {
        let stale: Vec<UInt256> = self
            .verified
            .iter()
            .filter(|entry| entry.value().block.index() <= height)
            .map(|entry| *entry.key())
            .collect();
        for hash in stale {
            self.verified.remove(&hash);
        }
        let stale: Vec<u32> = self
            .unverified
            .iter()
            .map(|entry| *entry.key())
            .filter(|index| *index <= height)
            .collect();
        for index in stale {
            self.remove_unverified_index(index);
        }
    }

    /// Heights above `index` at which either cache holds a block, each with
    /// the hashes the verified cache holds there.
    fn cached_heights_above(&self, index: u32) -> BTreeMap<u32, Vec<UInt256>> {
        let mut heights: BTreeMap<u32, Vec<UInt256>> = BTreeMap::new();
        for entry in self.unverified.iter() {
            if *entry.key() > index {
                heights.entry(*entry.key()).or_default();
            }
        }
        for entry in self.verified.iter() {
            let height = entry.value().block.index();
            if height > index {
                heights.entry(height).or_default().push(*entry.key());
            }
        }
        heights
    }

    // ------------------------------------------------------------------
    // Unverified cache
    // ------------------------------------------------------------------

    /// Budget bytes the unverified cache currently holds.
    pub fn unverified_bytes(&self) -> usize {
        self.unverified_bytes.load(Ordering::Relaxed)
    }

    /// Number of blocks the unverified cache currently holds.
    pub fn unverified_block_count(&self) -> usize {
        self.unverified_blocks.load(Ordering::Relaxed)
    }

    /// Heights at which the unverified cache holds at least one block.
    pub fn unverified_heights(&self) -> Vec<u32> {
        self.unverified.iter().map(|entry| *entry.key()).collect()
    }

    /// Whether the unverified cache holds a block at `index`.
    pub fn contains_unverified_height(&self, index: u32) -> bool {
        self.unverified.contains_key(&index)
    }

    /// Snapshot of the candidates cached at `index`, oldest first.
    pub fn unverified_candidates(&self, index: u32) -> Option<Vec<RetainedBlock>> {
        self.unverified
            .get(&index)
            .map(|entry| entry.value().clone())
    }

    /// Snapshot of every height the unverified cache holds, oldest first.
    pub fn unverified_entries(&self) -> Vec<(u32, Vec<RetainedBlock>)> {
        self.unverified
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    /// Takes `count` blocks costing `bytes` off the unverified cache counters.
    fn release_unverified(&self, bytes: usize, count: usize) {
        self.unverified_bytes
            .update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.saturating_sub(bytes)
            });
        self.unverified_blocks
            .update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.saturating_sub(count)
            });
    }

    /// Removes every unverified candidate at `index`, keeping the cache byte
    /// and block counters consistent.
    pub fn remove_unverified_index(&self, index: u32) -> Option<Vec<RetainedBlock>> {
        let removed = self.unverified.remove(&index).map(|(_, list)| list)?;
        let bytes = removed
            .iter()
            .map(|entry| entry.cost)
            .fold(0usize, usize::saturating_add);
        self.release_unverified(bytes, removed.len());
        Some(removed)
    }

    /// Pops the newest candidate cached at `index`, if any.
    pub fn pop_unverified(&self, index: u32) -> Option<RetainedBlock> {
        let (retained, empty) = {
            let mut entry = self.unverified.get_mut(&index)?;
            let retained = entry.pop()?;
            (retained, entry.is_empty())
        };
        if empty {
            self.unverified.remove(&index);
        }
        self.release_unverified(retained.cost, 1);
        Some(retained)
    }

    /// Parks `block` in the unverified cache until its height can be
    /// persisted. `retained` is the entry the verified cache holds for the
    /// block, if it was admitted there; otherwise the block is charged to the
    /// block cache budget here.
    ///
    /// Returns whether the block was kept; a block the caps or the budget
    /// cannot take is dropped with a warning.
    pub fn admit_unverified(
        &self,
        block: Arc<Block>,
        block_hash: UInt256,
        retained: Option<RetainedBlock>,
        bodies: &dyn PersistedBodyCache,
    ) -> bool {
        let index = block.index();
        let cost = retained.as_ref().map_or_else(
            || execution_block_cache_cost(block.as_ref()),
            |retained| retained.cost,
        );

        if cost > MAX_UNVERIFIED_CACHE_BYTES {
            warn!(
                target: "neo",
                index,
                cost,
                limit = MAX_UNVERIFIED_CACHE_BYTES,
                "unverified block exceeds cache byte budget, dropping block"
            );
            return false;
        }

        // Deduplicate by hash and bound the number of fork candidates kept for
        // a single height. Without these caps a peer can pin unbounded memory
        // by sending many different blocks for the same future height.
        //
        // When the per-height cap is full we evict the oldest candidate and
        // accept the new one. Silently dropping the newcomer let an attacker
        // pre-fill every future height with cheap bogus blocks and censor the
        // honest block (whose hash is then suppressed by the peer dedup cache).
        let mut replaced = None;
        if let Some(existing) = self.unverified.get(&index) {
            if existing
                .iter()
                .any(|candidate| candidate.hash == block_hash)
            {
                return true;
            }
            if existing.len() >= MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT {
                replaced = existing.first().map(|oldest| (oldest.hash, oldest.cost));
            }
        }

        // Admission is all or nothing: the candidate this block replaces and
        // the blocks above it are only given up once it is known that doing
        // so lets the block in. A block that is refused changes nothing.
        if !self.unverified_caps_can_take(index, cost, replaced.map(|(_, cost)| cost)) {
            warn!(
                target: "neo",
                index,
                cost,
                "unable to free space in unverified block cache, dropping block"
            );
            return false;
        }
        if retained.is_none()
            && !self.can_make_room(index, cost, replaced.map(|(hash, _)| hash), bodies)
        {
            warn!(
                target: "neo",
                index,
                cost,
                "block cache budget cannot hold unverified block, dropping block"
            );
            return false;
        }

        if let Some((oldest, oldest_cost)) = replaced {
            let removed = self.unverified.get_mut(&index).and_then(|mut existing| {
                let position = existing
                    .iter()
                    .position(|candidate| candidate.hash == oldest)?;
                Some(existing.remove(position))
            });
            if removed.is_some() {
                self.release_unverified(oldest_cost, 1);
            }
        }

        if !self.evict_unverified_for(index, cost) {
            warn!(
                target: "neo",
                index,
                cost,
                "unable to free space in unverified block cache, dropping block"
            );
            return false;
        }

        let Some(retained) = retained.or_else(|| self.retain(block, block_hash, bodies)) else {
            warn!(
                target: "neo",
                index,
                cost,
                "block cache budget cannot hold unverified block, dropping block"
            );
            return false;
        };

        self.unverified.entry(index).or_default().push(retained);
        self.unverified_bytes.fetch_add(cost, Ordering::Relaxed);
        self.unverified_blocks.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Whether the block-count and byte caps of the unverified cache can take
    /// a block at `index` costing `cost` once every entry above it is given
    /// up. `replaced_cost` is the cost of the candidate at its own height
    /// whose place it would take, if any. Changes nothing.
    fn unverified_caps_can_take(
        &self,
        index: u32,
        cost: usize,
        replaced_cost: Option<usize>,
    ) -> bool {
        let fits = |bytes: usize, blocks: usize| {
            let bytes = bytes.saturating_sub(replaced_cost.unwrap_or(0));
            let blocks = blocks.saturating_sub(usize::from(replaced_cost.is_some()));
            bytes.saturating_add(cost) <= MAX_UNVERIFIED_CACHE_BYTES
                && blocks < MAX_UNVERIFIED_CACHE_SIZE
        };
        if fits(self.unverified_bytes(), self.unverified_block_count()) {
            return true;
        }

        // What stays when everything above the block is given up.
        let (bytes, blocks) = self
            .unverified
            .iter()
            .filter(|entry| *entry.key() <= index)
            .fold((0usize, 0usize), |(bytes, blocks), entry| {
                let candidates = entry.value();
                (
                    candidates
                        .iter()
                        .map(|candidate| candidate.cost)
                        .fold(bytes, usize::saturating_add),
                    blocks + candidates.len(),
                )
            });
        fits(bytes, blocks)
    }

    /// Evicts unverified entries above `incoming_index`, highest first, until
    /// `incoming_cost` fits within the block-count and byte caps of the
    /// unverified cache. Returns false when the incoming block still does not
    /// fit: entries at or below its height are never evicted for it. Callers
    /// rule that out with [`Self::unverified_caps_can_take`] first.
    fn evict_unverified_for(&self, incoming_index: u32, incoming_cost: usize) -> bool {
        loop {
            let bytes = self.unverified_bytes();
            let blocks = self.unverified_block_count();
            if bytes.saturating_add(incoming_cost) <= MAX_UNVERIFIED_CACHE_BYTES
                && blocks < MAX_UNVERIFIED_CACHE_SIZE
            {
                return true;
            }

            let Some(max_index) = self
                .unverified
                .iter()
                .map(|entry| *entry.key())
                .filter(|cached_index| *cached_index > incoming_index)
                .max()
            else {
                return false;
            };

            if self.remove_unverified_index(max_index).is_none() {
                return false;
            }
        }
    }

    /// Parks a stand-in for a block of `cost` bytes at `index`, bypassing the
    /// admission caps: the block itself is small but is charged and counted as
    /// `cost` bytes. Returns false when the budget cannot take it.
    ///
    /// Meant for tests that reach a byte cap without allocating that much.
    pub fn park_unverified_costing(&self, index: u32, block: Arc<Block>, cost: usize) -> bool {
        let hash = block.hash();
        let Some(retained) = RetainedBlock::charge(block, hash, cost, &self.budget) else {
            return false;
        };
        self.unverified.entry(index).or_default().push(retained);
        self.unverified_bytes.fetch_add(cost, Ordering::Relaxed);
        self.unverified_blocks.fetch_add(1, Ordering::Relaxed);
        true
    }
}
