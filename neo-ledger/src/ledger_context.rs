//! Shared in-memory ledger cache mirroring the behaviour of the C# `NeoSystem`.
//!
//! Distributed from `neo-core::ledger::ledger_context`. Uses `neo-protocol`
//! payload types and `neo-io` result types instead of the former neo-core
//! internal types.
//!
//! # Bounded retention
//!
//! A node must never keep every block, header and extensible payload it has ever
//! seen in memory: the chain is unbounded and any peer can propose any height.
//! Every collection in this struct is therefore bounded.
//!
//! * Block hashes and headers live in a height window of
//!   [`DEFAULT_BLOCK_CACHE_CAPACITY`] entries (configurable through
//!   [`LedgerContext::with_capacities`]). The oldest heights are evicted first,
//!   and a jump to a height beyond the retained window is refused instead of
//!   being allocated: a single block claiming a far-future height can neither
//!   grow the index vector to that height nor evict the legitimate window.
//! * Block bodies form an insertion-ordered FIFO of the same capacity. They are
//!   also charged to a [`BlockCacheBudget`] shared with the blockchain actor's
//!   block caches (see [`crate::block_cache_budget`]): a body is cached only
//!   while its bytes fit, the oldest bodies make room for a newer one, and a
//!   body that cannot fit is not cached at all.
//! * Extensible payloads are an LRU of [`DEFAULT_EXTENSIBLE_CACHE_CAPACITY`]
//!   entries.
//! * The transaction cache is bounded by the memory pool that feeds it.
//!
//! Evicted heights remain available through the persistent store; these caches
//! are a fast path for recently seen data, never the source of truth.
//!
//! # Lock Ordering
//!
//! When acquiring multiple locks in this struct, follow this order to prevent deadlocks:
//! 1. `hashes_by_index`
//! 2. `headers_by_index`
//! 3. `blocks_by_hash`
//! 4. `extensibles_by_hash`
//! 5. `transactions_by_hash`
//!
//! See `docs/LOCK_ORDERING.md` for the global lock hierarchy.

use crate::block_cache_budget::{
    BlockCacheBudget, BlockCacheReservation, payload_block_cache_cost,
};
use lru::LruCache;
use neo_io::IoResult;
use neo_primitives::UInt256;
use neo_protocol::payloads::block::Block;
use neo_protocol::payloads::extensible::Extensible;
use neo_protocol::payloads::header::Header;
use neo_protocol::payloads::transaction::Transaction;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Default number of block bodies and headers retained in memory.
///
/// Sized for the fast-sync window: enough to answer peer requests for recent
/// data without pinning the entire chain.
pub const DEFAULT_BLOCK_CACHE_CAPACITY: usize = 2048;

/// Default number of extensible payloads retained in memory.
pub const DEFAULT_EXTENSIBLE_CACHE_CAPACITY: usize = 1024;

/// Outcome of writing one height into a [`HeightWindow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowInsert {
    /// The height was stored inside the retained window.
    Indexed,
    /// The height predates the retained window (already evicted); not indexed.
    Stale,
    /// The height is too far ahead of the retained window; the jump was refused
    /// instead of being allocated.
    TooFarAhead,
}

/// Bounded, height-indexed window of ledger entries.
///
/// `entries[0]` corresponds to absolute height `base`. The window holds at most
/// `capacity` heights, so its memory is constant no matter how high the chain
/// grows or what height a peer claims.
struct HeightWindow<T> {
    /// Absolute height of `entries[0]`.
    base: u32,
    entries: Vec<T>,
    capacity: usize,
    /// Value used to fill heights that were skipped inside the window.
    empty: T,
}

impl<T: Clone> HeightWindow<T> {
    fn new(capacity: usize, empty: T) -> Self {
        Self {
            base: 0,
            entries: Vec::new(),
            capacity: capacity.max(1),
            empty,
        }
    }

    /// Highest height currently addressable by the window, when any.
    fn newest(&self) -> Option<u32> {
        if self.entries.is_empty() {
            return None;
        }
        let len = u32::try_from(self.entries.len()).ok()?;
        Some(self.base.saturating_add(len - 1))
    }

    /// Returns the entry at `index`, if that height is still retained.
    fn get(&self, index: u32) -> Option<&T> {
        let offset = usize::try_from(index.checked_sub(self.base)?).ok()?;
        self.entries.get(offset)
    }

    /// Returns the vector offset of `index`, if that height is retained.
    fn offset_of(&self, index: u32) -> Option<usize> {
        let offset = usize::try_from(index.checked_sub(self.base)?).ok()?;
        (offset < self.entries.len()).then_some(offset)
    }

    /// Stores `value` at `index`, evicting the oldest heights as needed.
    fn insert(&mut self, index: u32, value: T) -> WindowInsert {
        // A height below the window base was evicted when the window slid.
        if index < self.base {
            return WindowInsert::Stale;
        }

        // Refuse a jump larger than the whole window: one block claiming a
        // far-future height must not evict every legitimate retained entry, and
        // the gap must never be allocated. Block *bodies* stay reachable by hash;
        // only the height index is refused.
        if let Some(newest) = self.newest()
            && index > newest
            && index - newest > self.capacity_span()
        {
            return WindowInsert::TooFarAhead;
        }

        // Keep `index` inside `[base, base + capacity)`. Anything below the new
        // base is the oldest retained height and is dropped first.
        let lowest_kept = index.saturating_sub(self.capacity_span());
        if lowest_kept > self.base {
            let drop_count = usize::try_from(lowest_kept - self.base).unwrap_or(usize::MAX);
            let dropped = drop_count.min(self.entries.len());
            self.entries.drain(..dropped);
            self.base = self.base.saturating_add(dropped as u32);
        }

        let offset = usize::try_from(index - self.base).unwrap_or(usize::MAX);
        if offset >= self.capacity {
            // The jump consumed the entire window: re-anchor on the new height
            // rather than allocating the gap.
            self.entries.clear();
            self.base = index;
            self.entries.push(value);
            return WindowInsert::Indexed;
        }

        while self.entries.len() <= offset {
            self.entries.push(self.empty.clone());
        }
        self.entries[offset] = value;
        WindowInsert::Indexed
    }

    /// Number of heights the window can address, minus one (the span).
    fn capacity_span(&self) -> u32 {
        u32::try_from(self.capacity.saturating_sub(1)).unwrap_or(u32::MAX)
    }
}

/// A cached block body together with the budget bytes it pays for.
struct CachedBody {
    // Declared before the reservation so the body is freed before its bytes
    // are returned to the budget.
    block: Block,
    reservation: BlockCacheReservation,
}

/// Insertion-ordered block body cache, bounded by entry count and by the
/// shared block cache budget.
struct BlockBodyCache {
    blocks: HashMap<UInt256, CachedBody>,
    order: VecDeque<UInt256>,
    capacity: usize,
    budget: Arc<BlockCacheBudget>,
    /// Sum of the reservations held by `blocks`.
    retained_bytes: usize,
}

impl BlockBodyCache {
    fn new(capacity: usize, budget: Arc<BlockCacheBudget>) -> Self {
        Self {
            blocks: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
            budget,
            retained_bytes: 0,
        }
    }

    /// Caches `block`, evicting the oldest bodies of this cache as needed.
    ///
    /// Returns false when the body was not cached because the budget cannot
    /// hold it even with every other body of this cache evicted. Bytes held by
    /// other caches are never reclaimed from here.
    fn insert(&mut self, hash: UInt256, block: Block) -> bool {
        let cost = payload_block_cache_cost(&block);
        // A body cached again under the same hash keeps its place in the
        // eviction order; its old bytes are returned first so it is charged
        // once.
        let existed = self.remove_body(&hash);

        let reservation = if cost > self.retained_bytes.saturating_add(self.budget.available()) {
            // Evicting this cache empty would still not make room.
            None
        } else {
            loop {
                if let Some(reservation) = self.budget.try_reserve(cost) {
                    break Some(reservation);
                }
                if !self.evict_oldest_except(&hash) {
                    break None;
                }
            }
        };

        let Some(reservation) = reservation else {
            if existed {
                self.order.retain(|cached| cached != &hash);
            }
            return false;
        };

        self.retained_bytes = self.retained_bytes.saturating_add(reservation.bytes());
        self.blocks.insert(hash, CachedBody { block, reservation });
        if !existed {
            self.order.push_back(hash);
        }
        // Never evict the entry that was just inserted.
        while self.order.len() > self.capacity {
            if !self.evict_oldest_except(&hash) {
                break;
            }
        }
        true
    }

    /// Drops the body cached under `hash`, leaving its eviction-order slot.
    fn remove_body(&mut self, hash: &UInt256) -> bool {
        let Some(cached) = self.blocks.remove(hash) else {
            return false;
        };
        self.retained_bytes = self
            .retained_bytes
            .saturating_sub(cached.reservation.bytes());
        true
    }

    /// Evicts the oldest body other than `keep`. Returns false when there is
    /// none.
    fn evict_oldest_except(&mut self, keep: &UInt256) -> bool {
        let Some(position) = self.order.iter().position(|cached| cached != keep) else {
            return false;
        };
        let Some(oldest) = self.order.remove(position) else {
            return false;
        };
        self.remove_body(&oldest);
        true
    }

    /// Evicts the oldest bodies until `bytes_needed` bytes were returned to
    /// the budget or the cache is empty. Returns the bytes returned.
    fn evict_bytes(&mut self, bytes_needed: usize) -> usize {
        let before = self.retained_bytes;
        while before.saturating_sub(self.retained_bytes) < bytes_needed {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.remove_body(&oldest);
        }
        before.saturating_sub(self.retained_bytes)
    }

    fn get(&self, hash: &UInt256) -> Option<&Block> {
        self.blocks.get(hash).map(|cached| &cached.block)
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }
}

/// Centralised cache that tracks recently seen ledger data (blocks, headers,
/// transactions, extensible payloads) for fast access by networking
/// components. Matches the responsibilities of the C# `LedgerContext`, with
/// bounded retention (see the module documentation).
///
/// # Thread Safety
///
/// All fields are protected by `RwLock` or atomic operations. Multiple readers
/// can access simultaneously, but writers have exclusive access.
///
/// # Lock Ordering
///
/// The locks in this structure must be acquired in the order defined at the module level.
/// Most methods only acquire a single lock, which is always safe.
pub struct LedgerContext {
    best_height: AtomicU32,
    best_header: AtomicU32,
    hashes_by_index: RwLock<HeightWindow<UInt256>>,
    headers_by_index: RwLock<HeightWindow<Option<Header>>>,
    blocks_by_hash: RwLock<BlockBodyCache>,
    extensibles_by_hash: RwLock<LruCache<UInt256, Extensible>>,
    transactions_by_hash: RwLock<HashMap<UInt256, Transaction>>,
    block_capacity: usize,
    extensible_capacity: usize,
    block_cache_budget: Arc<BlockCacheBudget>,
}

impl Default for LedgerContext {
    fn default() -> Self {
        Self::new()
    }
}

impl LedgerContext {
    /// Creates a context with the default bounded capacities.
    pub fn new() -> Self {
        Self::with_capacities(
            DEFAULT_BLOCK_CACHE_CAPACITY,
            DEFAULT_EXTENSIBLE_CACHE_CAPACITY,
        )
    }

    /// Creates a context retaining at most `block_capacity` blocks/headers and
    /// `extensible_capacity` extensible payloads.
    ///
    /// A zero capacity is raised to one so the caches stay usable.
    pub fn with_capacities(block_capacity: usize, extensible_capacity: usize) -> Self {
        Self::with_capacities_and_budget(
            block_capacity,
            extensible_capacity,
            BlockCacheBudget::with_default_limit(),
        )
    }

    /// Like [`Self::with_capacities`], charging cached block bodies to
    /// `block_cache_budget` instead of a budget of the default size.
    pub fn with_capacities_and_budget(
        block_capacity: usize,
        extensible_capacity: usize,
        block_cache_budget: Arc<BlockCacheBudget>,
    ) -> Self {
        let block_capacity = block_capacity.max(1);
        let extensible_capacity = NonZeroUsize::new(extensible_capacity.max(1))
            .expect("extensible cache capacity is non-zero");
        Self {
            best_height: AtomicU32::new(0),
            best_header: AtomicU32::new(0),
            hashes_by_index: RwLock::new(HeightWindow::new(block_capacity, UInt256::zero())),
            headers_by_index: RwLock::new(HeightWindow::<Option<Header>>::new(
                block_capacity,
                None,
            )),
            blocks_by_hash: RwLock::new(BlockBodyCache::new(
                block_capacity,
                Arc::clone(&block_cache_budget),
            )),
            extensibles_by_hash: RwLock::new(LruCache::new(extensible_capacity)),
            transactions_by_hash: RwLock::new(HashMap::new()),
            block_capacity,
            extensible_capacity: extensible_capacity.get(),
            block_cache_budget,
        }
    }

    /// Byte budget the cached block bodies are charged to.
    ///
    /// The blockchain actor charges its own block caches to the same budget,
    /// so the bytes all of them retain stay within one limit.
    pub fn block_cache_budget(&self) -> Arc<BlockCacheBudget> {
        Arc::clone(&self.block_cache_budget)
    }

    /// Number of block bodies / headers this context retains.
    pub fn block_capacity(&self) -> usize {
        self.block_capacity
    }

    /// Number of extensible payloads this context retains.
    pub fn extensible_capacity(&self) -> usize {
        self.extensible_capacity
    }

    /// Number of block bodies currently cached.
    pub fn cached_block_count(&self) -> usize {
        self.blocks_by_hash.read().len()
    }

    /// Budget bytes currently held by the cached block bodies.
    pub fn cached_block_bytes(&self) -> usize {
        self.blocks_by_hash.read().retained_bytes
    }

    /// Evicts the oldest cached block bodies until at least `bytes_needed`
    /// bytes were returned to the block cache budget, or none are left.
    /// Returns the number of bytes returned.
    ///
    /// Bodies cached here are a fast path only (see the module
    /// documentation), so the blockchain actor reclaims them before it gives
    /// up any block that is still waiting to be persisted.
    pub fn evict_block_bodies(&self, bytes_needed: usize) -> usize {
        self.blocks_by_hash.write().evict_bytes(bytes_needed)
    }

    /// Returns the highest block index recorded in memory.
    pub fn current_height(&self) -> u32 {
        self.best_height.load(Ordering::Relaxed)
    }

    /// Updates in-memory tip trackers without storing block/header bodies.
    pub fn record_tip(&self, index: u32) {
        self.best_height.fetch_max(index, Ordering::Relaxed);
        self.best_header.fetch_max(index, Ordering::Relaxed);
    }

    /// Inserts a transaction into the mempool cache and returns its hash.
    pub fn insert_transaction(&self, transaction: Transaction) -> IoResult<UInt256> {
        let hash = transaction.try_hash()?;
        self.transactions_by_hash.write().insert(hash, transaction);
        Ok(hash)
    }

    /// Removes a transaction from the mempool cache if present.
    pub fn remove_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.transactions_by_hash.write().remove(hash)
    }

    /// Attempts to fetch a transaction from the mempool cache.
    pub fn get_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.transactions_by_hash.read().get(hash).cloned()
    }

    /// Records a block and its header for quick access by hash or index.
    ///
    /// The tip trackers only advance when the height actually entered the
    /// bounded index window, so a refused far-future height cannot advertise
    /// itself as the local tip.
    ///
    /// Caching the body is best effort: when the shared block cache budget
    /// cannot hold it, the hash, header and tip are recorded all the same and
    /// the body is left to the persistent store.
    ///
    /// # Lock Ordering
    ///
    /// This method acquires locks in the correct order: hashes_by_index,
    /// headers_by_index, then blocks_by_hash.
    pub fn insert_block(&self, mut block: Block) -> IoResult<UInt256> {
        let header = block.header.clone();
        let index = header.index();
        let hash = block.try_hash()?;

        // Lock order: 1. hashes_by_index, 2. headers_by_index, 3. blocks_by_hash
        let indexed = self.hashes_by_index.write().insert(index, hash);
        self.headers_by_index.write().insert(index, Some(header));
        if !self.blocks_by_hash.write().insert(hash, block) {
            tracing::debug!(
                target: "neo::ledger",
                index,
                %hash,
                "block cache budget cannot hold this block body; not cached"
            );
        }

        match indexed {
            WindowInsert::Indexed => {
                self.best_height.fetch_max(index, Ordering::Relaxed);
                self.best_header.fetch_max(index, Ordering::Relaxed);
            }
            WindowInsert::Stale => {
                tracing::debug!(
                    target: "neo::ledger",
                    index,
                    %hash,
                    "block predates the retained ledger window; body cached by hash only"
                );
            }
            WindowInsert::TooFarAhead => {
                tracing::warn!(
                    target: "neo::ledger",
                    index,
                    %hash,
                    capacity = self.block_capacity,
                    "refusing to index a block too far ahead of the retained ledger window"
                );
            }
        }

        Ok(hash)
    }

    /// Retrieves a cached block by hash.
    pub fn get_block(&self, hash: &UInt256) -> Option<Block> {
        self.blocks_by_hash.read().get(hash).cloned()
    }

    /// Returns the block hash at the specified index when available.
    ///
    /// Heights evicted from the bounded window return `None`; read them from the
    /// persistent store instead.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        self.hashes_by_index
            .read()
            .get(index)
            .copied()
            .filter(|hash| *hash != UInt256::zero())
    }

    /// Stores an extensible payload in the cache, evicting the least recently
    /// used entry when the capacity is reached.
    pub fn insert_extensible(&self, mut payload: Extensible) -> IoResult<UInt256> {
        let hash = payload.try_hash()?;
        self.extensibles_by_hash.write().put(hash, payload);
        Ok(hash)
    }

    /// Tries to retrieve an extensible payload by hash.
    ///
    /// A hit promotes the entry, so frequently requested payloads survive
    /// eviction longer than one-off broadcasts.
    pub fn get_extensible(&self, hash: &UInt256) -> Option<Extensible> {
        self.extensibles_by_hash.write().get(hash).cloned()
    }

    /// Returns block hashes following `hash_start`, limited by `count`.
    pub fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256> {
        if count == 0 {
            return Vec::new();
        }

        let hashes = self.hashes_by_index.read();
        let Some(start_pos) = hashes.entries.iter().position(|hash| hash == hash_start) else {
            return Vec::new();
        };

        hashes
            .entries
            .iter()
            .skip(start_pos + 1)
            .filter(|hash| **hash != UInt256::zero())
            .take(count)
            .copied()
            .collect()
    }

    /// Returns the highest header index this context has observed.
    pub fn highest_header_index(&self) -> u32 {
        self.best_header.load(Ordering::Relaxed)
    }

    /// Indicates whether headers beyond the current block height are buffered.
    pub fn has_future_headers(&self) -> bool {
        self.highest_header_index() > self.current_height()
    }

    /// Returns headers starting at `index_start`, up to `count` entries.
    ///
    /// Returns an empty vector when `index_start` predates the retained window:
    /// the window only serves recent headers, the store serves history.
    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        if count == 0 {
            return Vec::new();
        }

        let headers = self.headers_by_index.read();
        let Some(offset) = headers.offset_of(index_start) else {
            return Vec::new();
        };
        let mut collected = Vec::with_capacity(count.min(headers.entries.len() - offset));

        for entry in &headers.entries[offset..] {
            if collected.len() >= count {
                break;
            }
            match entry {
                Some(header) => collected.push(header.clone()),
                None => break,
            }
        }

        collected
    }

    /// Returns all transaction hashes currently tracked by the mempool cache.
    pub fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        self.transactions_by_hash.read().keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{UInt160, WitnessScope};
    use neo_protocol::payloads::signer::Signer;
    use neo_protocol::witness::Witness;
    use neo_vm::OpCode;
    use std::sync::atomic::Ordering;

    fn transaction_with_script(script: Vec<u8>) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(1);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(script);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    fn block_with_index(index: u32) -> Block {
        let mut block = Block::new();
        let mut header = Header::new();
        header.set_index(index);
        header.set_timestamp(u64::from(index) + 1);
        block.header = header;
        block
    }

    /// A block whose serialized size is dominated by `script_len`.
    fn block_with_body(index: u32, script_len: usize) -> Block {
        let mut block = block_with_index(index);
        block.transactions = vec![transaction_with_script(vec![
            OpCode::NOP.byte();
            script_len
        ])];
        block
    }

    fn context_with_budget(block_capacity: usize, limit: usize) -> LedgerContext {
        LedgerContext::with_capacities_and_budget(block_capacity, 4, BlockCacheBudget::new(limit))
    }

    fn extensible_with_data(data: u8) -> Extensible {
        let mut payload = Extensible::new();
        payload.category = "test".to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = u32::MAX;
        payload.data = vec![data];
        payload
    }

    #[test]
    fn detects_future_headers() {
        let context = LedgerContext::default();
        assert!(!context.has_future_headers());

        {
            let mut headers = context.headers_by_index.write();
            let mut header = Header::new();
            header.set_index(2);
            headers.insert(2, Some(header));
        }

        context.best_header.store(2, Ordering::Relaxed);
        assert!(context.has_future_headers());
    }

    #[test]
    fn insert_transaction_rejects_unserializable_hash_without_zero_cache() {
        let context = LedgerContext::default();
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert!(context.insert_transaction(tx).is_err());
        assert!(context.transactions_by_hash.read().is_empty());
        assert!(context.get_transaction(&UInt256::zero()).is_none());
    }

    #[test]
    fn insert_transaction_caches_valid_hash() {
        let context = LedgerContext::default();
        let tx = transaction_with_script(vec![OpCode::PUSH1.byte()]);
        let expected = tx.try_hash().expect("hash");

        let hash = context.insert_transaction(tx).expect("insert transaction");

        assert_eq!(hash, expected);
        assert!(context.get_transaction(&expected).is_some());
    }

    #[test]
    fn block_window_evicts_oldest_heights_beyond_capacity() {
        let context = LedgerContext::with_capacities(4, 4);
        let mut hashes = Vec::new();
        for index in 0..6u32 {
            hashes.push(context.insert_block(block_with_index(index)).expect("hash"));
        }

        // Only the newest four heights stay addressable.
        assert_eq!(context.block_hash_at(5), Some(hashes[5]));
        assert_eq!(context.block_hash_at(2), Some(hashes[2]));
        assert!(context.block_hash_at(0).is_none());
        assert!(context.block_hash_at(1).is_none());

        // Bodies follow the same bound, so the cache cannot pin the whole chain.
        assert_eq!(context.cached_block_count(), 4);
        assert!(context.get_block(&hashes[0]).is_none());
        assert!(context.get_block(&hashes[5]).is_some());
        assert_eq!(context.current_height(), 5);
    }

    #[test]
    fn far_future_height_is_refused_instead_of_allocated() {
        let context = LedgerContext::with_capacities(4, 4);
        let genesis_hash = context
            .insert_block(block_with_index(0))
            .expect("genesis hash");

        // A peer claiming a height far beyond the window must not resize the
        // index vector nor evict the retained window.
        let far_future = block_with_index(1_000_000);
        let far_future_hash = context.insert_block(far_future).expect("hash");

        assert!(context.block_hash_at(1_000_000).is_none());
        assert_eq!(context.block_hash_at(0), Some(genesis_hash));
        assert_eq!(context.current_height(), 0);
        // The body is still reachable by hash, only the index entry was refused.
        assert!(context.get_block(&far_future_hash).is_some());
    }

    #[test]
    fn extensible_cache_evicts_least_recently_used_payload() {
        let context = LedgerContext::with_capacities(4, 2);
        let first = context
            .insert_extensible(extensible_with_data(1))
            .expect("hash");
        let second = context
            .insert_extensible(extensible_with_data(2))
            .expect("hash");

        // Promote `first` so `second` becomes the least recently used entry.
        assert!(context.get_extensible(&first).is_some());
        let third = context
            .insert_extensible(extensible_with_data(3))
            .expect("hash");

        assert!(context.get_extensible(&first).is_some());
        assert!(context.get_extensible(&third).is_some());
        assert!(context.get_extensible(&second).is_none());
    }

    #[test]
    fn headers_past_the_window_are_not_served() {
        let context = LedgerContext::with_capacities(4, 4);
        for index in 0..6u32 {
            context.insert_block(block_with_index(index)).expect("hash");
        }

        assert!(context.headers_from_index(0, 8).is_empty());
        let headers = context.headers_from_index(4, 8);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].index(), 4);
        assert_eq!(headers[1].index(), 5);
    }

    #[test]
    fn cached_bodies_are_charged_to_the_budget_and_count_eviction_returns_them() {
        let context = context_with_budget(2, 1 << 20);
        let budget = context.block_cache_budget();
        let costs: Vec<usize> = (0..3u32)
            .map(|index| {
                payload_block_cache_cost(&block_with_body(index, 100 * (index as usize + 1)))
            })
            .collect();

        context.insert_block(block_with_body(0, 100)).expect("hash");
        assert_eq!(budget.retained(), costs[0]);
        context.insert_block(block_with_body(1, 200)).expect("hash");
        assert_eq!(budget.retained(), costs[0] + costs[1]);

        // The entry-count bound evicts block 0; its bytes must come back.
        context.insert_block(block_with_body(2, 300)).expect("hash");
        assert_eq!(context.cached_block_count(), 2);
        assert_eq!(budget.retained(), costs[1] + costs[2]);
        assert_eq!(context.cached_block_bytes(), budget.retained());
    }

    #[test]
    fn oldest_bodies_make_room_for_a_newer_body() {
        let cost = payload_block_cache_cost(&block_with_body(0, 500));
        // Room for two bodies and a bit, far below the entry-count bound.
        let context = context_with_budget(64, cost * 2 + cost / 2);
        let budget = context.block_cache_budget();

        let mut hashes = Vec::new();
        for index in 0..5u32 {
            hashes.push(
                context
                    .insert_block(block_with_body(index, 500))
                    .expect("hash"),
            );
            assert!(budget.retained() <= budget.limit());
        }

        assert_eq!(context.cached_block_count(), 2);
        assert_eq!(budget.retained(), cost * 2);
        assert!(context.get_block(&hashes[2]).is_none());
        assert!(context.get_block(&hashes[3]).is_some());
        assert!(context.get_block(&hashes[4]).is_some());
        // Eviction is a memory decision only: every height is still indexed.
        assert_eq!(context.block_hash_at(0), Some(hashes[0]));
        assert_eq!(context.current_height(), 4);
    }

    #[test]
    fn body_larger_than_the_budget_is_indexed_but_not_cached() {
        let small = block_with_body(0, 100);
        let small_cost = payload_block_cache_cost(&small);
        let context = context_with_budget(64, small_cost * 2);
        let budget = context.block_cache_budget();
        let small_hash = context.insert_block(small).expect("hash");

        let oversize = block_with_body(1, 4_000);
        assert!(payload_block_cache_cost(&oversize) > budget.limit());
        let oversize_hash = context.insert_block(oversize).expect("hash");

        // The ledger still advanced; only the body was left out.
        assert_eq!(context.block_hash_at(1), Some(oversize_hash));
        assert_eq!(context.headers_from_index(1, 1).len(), 1);
        assert_eq!(context.current_height(), 1);
        assert!(context.get_block(&oversize_hash).is_none());
        // Nothing was evicted for a body that could never fit.
        assert!(context.get_block(&small_hash).is_some());
        assert_eq!(budget.retained(), small_cost);
    }

    #[test]
    fn body_is_not_cached_when_other_caches_hold_the_budget() {
        let cost = payload_block_cache_cost(&block_with_body(0, 300));
        let context = context_with_budget(64, cost * 3);
        let budget = context.block_cache_budget();
        let first = context.insert_block(block_with_body(0, 300)).expect("hash");

        // Another cache takes everything that is left.
        let other = budget.try_reserve(budget.available()).expect("rest");
        let second = context.insert_block(block_with_body(1, 300)).expect("hash");
        // One own body made room; the other cache was not touched.
        assert!(context.get_block(&first).is_none());
        assert!(context.get_block(&second).is_some());
        assert_eq!(budget.retained(), budget.limit());

        // A body twice the size cannot fit even with this cache emptied, so
        // the cached body is kept instead of being evicted for nothing.
        let third = context.insert_block(block_with_body(2, 700)).expect("hash");
        assert!(context.get_block(&third).is_none());
        assert!(context.get_block(&second).is_some());
        assert_eq!(context.current_height(), 2);
        assert_eq!(budget.retained(), budget.limit());

        drop(other);
        assert_eq!(budget.retained(), cost);
    }

    #[test]
    fn reinserting_a_block_charges_it_once() {
        let context = context_with_budget(64, 1 << 20);
        let budget = context.block_cache_budget();
        let cost = payload_block_cache_cost(&block_with_body(0, 300));

        let hash = context.insert_block(block_with_body(0, 300)).expect("hash");
        let again = context.insert_block(block_with_body(0, 300)).expect("hash");
        assert_eq!(hash, again);
        assert_eq!(context.cached_block_count(), 1);
        assert_eq!(budget.retained(), cost);
    }

    #[test]
    fn evict_block_bodies_returns_bytes_oldest_first() {
        let context = context_with_budget(64, 1 << 20);
        let budget = context.block_cache_budget();
        let cost = payload_block_cache_cost(&block_with_body(0, 300));
        let hashes: Vec<UInt256> = (0..4u32)
            .map(|index| {
                context
                    .insert_block(block_with_body(index, 300))
                    .expect("hash")
            })
            .collect();

        assert_eq!(context.evict_block_bodies(0), 0);
        assert_eq!(context.cached_block_count(), 4);

        // One byte more than a body: two bodies have to go.
        assert_eq!(context.evict_block_bodies(cost + 1), cost * 2);
        assert!(context.get_block(&hashes[0]).is_none());
        assert!(context.get_block(&hashes[1]).is_none());
        assert!(context.get_block(&hashes[2]).is_some());
        assert_eq!(budget.retained(), cost * 2);

        assert_eq!(context.evict_block_bodies(usize::MAX), cost * 2);
        assert_eq!(context.cached_block_count(), 0);
        assert_eq!(context.cached_block_bytes(), 0);
        assert_eq!(budget.retained(), 0);
        // The index is untouched: these heights are served from the store.
        assert_eq!(context.block_hash_at(3), Some(hashes[3]));
    }

    #[test]
    fn dropping_the_context_returns_every_byte() {
        let budget = BlockCacheBudget::new(1 << 20);
        let context = LedgerContext::with_capacities_and_budget(64, 4, Arc::clone(&budget));
        for index in 0..8u32 {
            context
                .insert_block(block_with_body(index, 200))
                .expect("hash");
        }
        assert!(budget.retained() > 0);

        drop(context);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn contexts_sharing_a_budget_stay_within_it_under_concurrent_inserts() {
        let cost = payload_block_cache_cost(&block_with_body(0, 400));
        let budget = BlockCacheBudget::new(cost * 5);
        let contexts: Vec<Arc<LedgerContext>> = (0..4)
            .map(|_| {
                Arc::new(LedgerContext::with_capacities_and_budget(
                    64,
                    4,
                    Arc::clone(&budget),
                ))
            })
            .collect();

        let workers: Vec<_> = contexts
            .iter()
            .map(|context| {
                let context = Arc::clone(context);
                let budget = Arc::clone(&budget);
                std::thread::spawn(move || {
                    for index in 0..400u32 {
                        context
                            .insert_block(block_with_body(index, 400))
                            .expect("hash");
                        assert!(budget.retained() <= budget.limit());
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("worker");
        }

        let cached: usize = contexts
            .iter()
            .map(|context| context.cached_block_bytes())
            .sum();
        assert_eq!(budget.retained(), cached);
        assert!(cached <= budget.limit());

        drop(contexts);
        assert_eq!(budget.retained(), 0);
    }
}
