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
//! * Block bodies form an insertion-ordered FIFO of the same capacity.
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

/// Insertion-ordered, capacity-bounded block body cache.
struct BlockBodyCache {
    blocks: HashMap<UInt256, Block>,
    order: VecDeque<UInt256>,
    capacity: usize,
}

impl BlockBodyCache {
    fn new(capacity: usize) -> Self {
        Self {
            blocks: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    fn insert(&mut self, hash: UInt256, block: Block) {
        if self.blocks.insert(hash, block).is_none() {
            self.order.push_back(hash);
        }
        while self.order.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if oldest == hash {
                // Never evict the entry that was just inserted.
                self.order.push_back(oldest);
                break;
            }
            self.blocks.remove(&oldest);
        }
    }

    fn get(&self, hash: &UInt256) -> Option<&Block> {
        self.blocks.get(hash)
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
            blocks_by_hash: RwLock::new(BlockBodyCache::new(block_capacity)),
            extensibles_by_hash: RwLock::new(LruCache::new(extensible_capacity)),
            transactions_by_hash: RwLock::new(HashMap::new()),
            block_capacity,
            extensible_capacity: extensible_capacity.get(),
        }
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
        self.blocks_by_hash.write().insert(hash, block);

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
        let Some(start_pos) = hashes
            .entries
            .iter()
            .position(|hash| hash == hash_start)
        else {
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
}
