//! Shared in-memory ledger cache mirroring the behaviour of the C# `NeoSystem`.

use crate::network::p2p::payloads::{
    block::Block, extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction,
};
use crate::UInt256;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;

/// Centralised cache that tracks recently seen ledger data (blocks, headers,
/// transactions, extensible payloads) for fast access by networking
/// components. Matches the responsibilities of the C# `LedgerContext`.
#[derive(Default)]
pub struct LedgerContext {
    best_height: AtomicU32,
    best_header: AtomicU32,
    hashes_by_index: RwLock<Vec<UInt256>>,
    headers_by_index: RwLock<Vec<Option<Header>>>,
    blocks_by_hash: RwLock<HashMap<UInt256, Block>>,
    extensibles_by_hash: RwLock<HashMap<UInt256, ExtensiblePayload>>,
    transactions_by_hash: RwLock<HashMap<UInt256, Transaction>>,
}

impl LedgerContext {
    /// Returns the highest block index recorded in memory.
    pub fn current_height(&self) -> u32 {
        self.best_height.load(Ordering::Relaxed)
    }

    /// Inserts a transaction into the mempool cache and returns its hash.
    pub fn insert_transaction(&self, transaction: Transaction) -> UInt256 {
        let hash = transaction.hash();
        self.transactions_by_hash
            .write()
            .unwrap()
            .insert(hash, transaction);
        hash
    }

    /// Removes a transaction from the mempool cache if present.
    pub fn remove_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.transactions_by_hash.write().unwrap().remove(hash)
    }

    /// Attempts to fetch a transaction from the mempool cache.
    pub fn get_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.transactions_by_hash.read().unwrap().get(hash).cloned()
    }

    /// Records a block and its header for quick access by hash or index.
    pub fn insert_block(&self, mut block: Block) -> UInt256 {
        let header = block.header.clone();
        let index = header.index() as usize;
        let hash = block.hash();

        self.blocks_by_hash.write().unwrap().insert(hash, block);

        {
            let mut hashes = self.hashes_by_index.write().unwrap();
            if hashes.len() <= index {
                hashes.resize(index + 1, UInt256::zero());
            }
            hashes[index] = hash;
        }

        {
            let mut headers = self.headers_by_index.write().unwrap();
            if headers.len() <= index {
                headers.resize(index + 1, None);
            }
            headers[index] = Some(header);
        }

        self.best_height.fetch_max(index as u32, Ordering::Relaxed);
        self.best_header.fetch_max(index as u32, Ordering::Relaxed);
        hash
    }

    /// Retrieves a cached block by hash.
    pub fn get_block(&self, hash: &UInt256) -> Option<Block> {
        self.blocks_by_hash.read().unwrap().get(hash).cloned()
    }

    /// Returns the block hash at the specified index when available.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        let hashes = self.hashes_by_index.read().unwrap();
        hashes
            .get(index as usize)
            .cloned()
            .filter(|hash| *hash != UInt256::zero())
    }

    /// Stores an extensible payload in the cache.
    pub fn insert_extensible(&self, mut payload: ExtensiblePayload) -> UInt256 {
        let hash = payload.hash();
        self.extensibles_by_hash
            .write()
            .unwrap()
            .insert(hash, payload);
        hash
    }

    /// Tries to retrieve an extensible payload by hash.
    pub fn get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.extensibles_by_hash.read().unwrap().get(hash).cloned()
    }

    /// Returns block hashes following `hash_start`, limited by `count`.
    pub fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256> {
        if count == 0 {
            return Vec::new();
        }

        let hashes = self.hashes_by_index.read().unwrap();
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

    /// Indicates whether headers beyond the current block height are buffered.
    pub fn has_future_headers(&self) -> bool {
        self.highest_header_index() > self.current_height()
    }

    /// Returns headers starting at `index_start`, up to `count` entries.
    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        if count == 0 {
            return Vec::new();
        }

        let headers = self.headers_by_index.read().unwrap();
        let mut collected = Vec::new();
        let mut index = index_start as usize;

        while index < headers.len() && collected.len() < count {
            match &headers[index] {
                Some(header) => collected.push(header.clone()),
                None => break,
            }
            index += 1;
        }

        collected
    }

    /// Returns all transaction hashes currently tracked by the mempool cache.
    pub fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        self.transactions_by_hash
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn detects_future_headers() {
        let context = LedgerContext::default();
        assert!(!context.has_future_headers());

        {
            let mut headers = context.headers_by_index.write().unwrap();
            headers.resize(3, None);
            let mut header = Header::new();
            header.set_index(2);
            headers[2] = Some(header);
        }

        context.best_header.store(2, Ordering::Relaxed);
        assert!(context.has_future_headers());
    }
}
