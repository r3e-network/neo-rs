//! In-memory ledger cache used by the blockchain service.
//!
//! Moved verbatim from `neo-core::ledger::ledger_context` in
//! Stage 4 of the kill-neo-core refactor. The service is the
//! single owner of the canonical tip, so this cache lives on the
//! service struct rather than in a global `NeoSystem` singleton.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use neo_error::CoreResult;
use neo_payloads::{
    block::Block, extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction,
};
use neo_primitives::UInt256;
use parking_lot::RwLock;

/// Centralised cache that tracks recently seen ledger data (blocks,
/// headers, transactions, extensible payloads) for fast access by
/// networking components. Matches the responsibilities of the C#
/// `LedgerContext`.
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
        let index = header.index() as usize;
        let hash = block.try_hash()?;

        self.blocks_by_hash.write().insert(hash, block);

        {
            let mut hashes = self.hashes_by_index.write();
            if hashes.len() <= index {
                hashes.resize(index + 1, UInt256::zero());
            }
            hashes[index] = hash;
        }

        {
            let mut headers = self.headers_by_index.write();
            if headers.len() <= index {
                headers.resize(index + 1, None);
            }
            headers[index] = Some(header);
        }

        self.best_height.fetch_max(index as u32, Ordering::Relaxed);
        self.best_header.fetch_max(index as u32, Ordering::Relaxed);
        Ok(hash)
    }

    /// Retrieves a cached block by hash.
    pub fn get_block(&self, hash: &UInt256) -> Option<Block> {
        self.blocks_by_hash.read().get(hash).cloned()
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

        let headers = self.headers_by_index.write();
        let mut collected = Vec::with_capacity(count);
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
}
