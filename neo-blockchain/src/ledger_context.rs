//! In-memory ledger cache used by the blockchain service.
//!
//! The service is the single owner of the canonical tip, so this cache lives
//! on the service state rather than in a process-wide singleton.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU32, Ordering};

use lru::LruCache;
use neo_error::CoreResult;
use neo_payloads::{
    block::Block, extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction,
};
use neo_primitives::UInt256;
use parking_lot::{Mutex, RwLock};

/// Number of recent full block bodies / headers retained in the in-memory
/// ledger cache. Cold reads beyond this window fall back to the durable
/// `LedgerContract` store (see the `GetBlock`/`GetBlockByHeight` command arms
/// in `service.rs`). The cheap height->hash index is kept in full because it
/// is only 32 bytes/entry and is needed to translate a height to a hash for
/// the durable fallback.
pub const DEFAULT_BLOCK_CACHE_CAPACITY: usize = 1024;

/// Centralised cache that tracks recently seen ledger data (blocks,
/// headers, transactions, extensible payloads) for fast access by
/// networking components. Matches the responsibilities of the C#
/// `LedgerContext`.
pub struct LedgerContext {
    best_height: AtomicU32,
    best_header: AtomicU32,
    /// Full height->hash index. Cheap (32 bytes/entry); kept complete so
    /// `block_hash_at` and `get_block_by_height` always resolve a height.
    hashes_by_index: RwLock<Vec<UInt256>>,
    /// LRU of the most-recent block headers, keyed by index.
    headers_by_index: Mutex<LruCache<u32, Header>>,
    /// LRU of the most-recent full block bodies, keyed by hash. Cold reads
    /// beyond the window are served from the durable store by the service.
    blocks_by_hash: Mutex<LruCache<UInt256, Block>>,
    extensibles_by_hash: RwLock<HashMap<UInt256, ExtensiblePayload>>,
    transactions_by_hash: RwLock<HashMap<UInt256, Transaction>>,
}

impl Default for LedgerContext {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_BLOCK_CACHE_CAPACITY)
    }
}

impl LedgerContext {
    /// Construct a context that retains `capacity` recent block bodies and
    /// headers in memory (minimum 1).
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).expect("capacity.max(1) is non-zero");
        Self {
            best_height: AtomicU32::new(0),
            best_header: AtomicU32::new(0),
            hashes_by_index: RwLock::new(Vec::new()),
            headers_by_index: Mutex::new(LruCache::new(cap)),
            blocks_by_hash: Mutex::new(LruCache::new(cap)),
            extensibles_by_hash: RwLock::new(HashMap::new()),
            transactions_by_hash: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the highest block index recorded in memory.
    pub fn current_height(&self) -> u32 {
        self.best_height.load(Ordering::Relaxed)
    }

    /// Updates in-memory tip trackers without storing block/header
    /// bodies.
    pub fn record_tip(&self, index: u32) {
        self.best_height.fetch_max(index, Ordering::Relaxed);
        self.best_header.fetch_max(index, Ordering::Relaxed);
    }

    /// Inserts a transaction into the mempool cache and returns its
    /// hash.
    pub fn insert_transaction(&self, transaction: Transaction) -> CoreResult<UInt256> {
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

    /// Records a block and its header for quick access by hash or
    /// index.
    pub fn insert_block(&self, block: Block) -> CoreResult<UInt256> {
        let header = block.header.clone();
        let index = header.index();
        let hash = block.try_hash()?;

        self.blocks_by_hash.lock().put(hash, block);
        self.headers_by_index.lock().put(index, header);

        {
            let mut hashes = self.hashes_by_index.write();
            let idx = index as usize;
            if hashes.len() <= idx {
                hashes.resize(idx + 1, UInt256::zero());
            }
            hashes[idx] = hash;
        }

        self.best_height.fetch_max(index, Ordering::Relaxed);
        self.best_header.fetch_max(index, Ordering::Relaxed);
        Ok(hash)
    }

    /// Retrieves a cached block by hash.
    pub fn get_block(&self, hash: &UInt256) -> Option<Block> {
        self.blocks_by_hash.lock().get(hash).cloned()
    }

    /// Look up a block by its canonical height, when known.
    pub fn get_block_by_height(&self, height: u32) -> Option<Block> {
        let hash = self.block_hash_at(height)?;
        self.get_block(&hash)
    }

    /// Returns the block hash at the specified index when available.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        let hashes = self.hashes_by_index.read();
        hashes
            .get(index as usize)
            .cloned()
            .filter(|hash| *hash != UInt256::zero())
    }

    /// Stores an extensible payload in the cache.
    pub fn insert_extensible(&self, mut payload: ExtensiblePayload) -> CoreResult<UInt256> {
        let hash = payload.try_hash()?;
        self.extensibles_by_hash.write().insert(hash, payload);
        Ok(hash)
    }

    /// Tries to retrieve an extensible payload by hash.
    pub fn get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.extensibles_by_hash.read().get(hash).cloned()
    }

    /// Returns block hashes following `hash_start`, limited by
    /// `count`.
    pub fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256> {
        if count == 0 {
            return Vec::new();
        }

        let hashes = self.hashes_by_index.read();
        let Some(start_pos) = hashes.iter().position(|hash| hash == hash_start) else {
            return Vec::new();
        };

        hashes
            .iter()
            .skip(start_pos + 1)
            .filter(|hash| **hash != UInt256::zero())
            .take(count)
            .cloned()
            .collect()
    }

    /// Returns the highest header index this context has observed.
    pub fn highest_header_index(&self) -> u32 {
        self.best_header.load(Ordering::Relaxed)
    }

    /// Indicates whether headers beyond the current block height are
    /// buffered.
    pub fn has_future_headers(&self) -> bool {
        self.highest_header_index() > self.current_height()
    }

    /// Returns headers starting at `index_start`, up to `count`
    /// entries.
    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        if count == 0 {
            return Vec::new();
        }

        let mut headers = self.headers_by_index.lock();
        let mut collected = Vec::with_capacity(count);
        let mut index = index_start;

        while collected.len() < count {
            match headers.get(&index) {
                Some(header) => collected.push(header.clone()),
                None => break,
            }
            index = index.wrapping_add(1);
        }

        collected
    }

    /// Returns all transaction hashes currently tracked by the
    /// mempool cache.
    pub fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        self.transactions_by_hash.read().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_payloads::signer::Signer;
    use neo_payloads::witness::Witness;
    use neo_primitives::{UInt160, WitnessScope};

    fn make_signed_transaction() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_valid_until_block(10);
        tx.add_signer(Signer::new(
            UInt160::default(),
            WitnessScope::CALLED_BY_ENTRY,
        ));
        tx.add_witness(Witness::new());
        tx
    }

    #[test]
    fn record_tip_tracks_highest_index() {
        let ledger = LedgerContext::default();
        ledger.record_tip(7);
        ledger.record_tip(5);
        ledger.record_tip(12);
        assert_eq!(ledger.current_height(), 12);
    }

    #[test]
    fn insert_and_get_transaction() {
        let ledger = LedgerContext::default();
        let tx = make_signed_transaction();
        let hash = tx.hash();
        ledger.insert_transaction(tx).expect("insert");
        assert!(ledger.get_transaction(&hash).is_some());
    }

    #[test]
    fn block_hash_at_unknown_index_returns_none() {
        let ledger = LedgerContext::default();
        assert!(ledger.block_hash_at(0).is_none());
        assert!(ledger.block_hash_at(123).is_none());
    }

    #[test]
    fn block_body_cache_is_bounded_and_evicts_oldest() {
        // Capacity of 2: inserting 3 blocks must evict the first body, but the
        // height->hash index must still record all three (it is kept full).
        let ledger = LedgerContext::with_capacity(2);

        let mut hashes = Vec::new();
        for i in 0..3u32 {
            let mut header = Header::new();
            header.set_index(i);
            // distinct nonce keeps each header hash unique
            header.set_nonce(1000 + i as u64);
            let block = Block::from_parts(header, vec![]);
            let hash = ledger.insert_block(block).expect("insert");
            hashes.push(hash);
        }

        // Oldest body (index 0) was evicted from the bounded in-memory cache...
        assert!(
            ledger.get_block(&hashes[0]).is_none(),
            "block body cache must evict beyond capacity"
        );
        // ...but the two most-recent bodies are still resident.
        assert!(ledger.get_block(&hashes[1]).is_some());
        assert!(ledger.get_block(&hashes[2]).is_some());

        // The cheap height->hash index is NOT evicted: every height resolves.
        assert_eq!(ledger.block_hash_at(0), Some(hashes[0]));
        assert_eq!(ledger.block_hash_at(1), Some(hashes[1]));
        assert_eq!(ledger.block_hash_at(2), Some(hashes[2]));
        assert_eq!(ledger.current_height(), 2);
    }
}
