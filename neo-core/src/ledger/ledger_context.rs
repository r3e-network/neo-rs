//! Shared in-memory ledger cache mirroring the behaviour of the C# `NeoSystem`.

use crate::network::p2p::payloads::{
    block::Block, extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction,
};
use crate::{CoreResult, UInt256};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

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

    /// Updates in-memory tip trackers without storing block/header bodies.
    pub fn record_tip(&self, index: u32) {
        self.best_height.fetch_max(index, Ordering::Relaxed);
        self.best_header.fetch_max(index, Ordering::Relaxed);
    }

    /// Inserts a transaction into the mempool cache and returns its hash.
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

    /// Records a block and its header for quick access by hash or index.
    pub fn insert_block(&self, mut block: Block) -> CoreResult<UInt256> {
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

    /// Returns the block hash at the specified index when available.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        let hashes = self.hashes_by_index.read();
        hashes
            .get(index as usize)
            .cloned()
            .filter(|hash| *hash != UInt256::zero())
    }

    /// Stores an extensible payload in the cache.
    pub fn insert_extensible(&self, mut payload: ExtensiblePayload) -> crate::CoreResult<UInt256> {
        let hash = payload.try_hash()?;
        self.extensibles_by_hash.write().insert(hash, payload);
        Ok(hash)
    }

    /// Tries to retrieve an extensible payload by hash.
    pub fn get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.extensibles_by_hash.read().get(hash).cloned()
    }

    /// Returns block hashes following `hash_start`, limited by `count`.
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

    /// Indicates whether headers beyond the current block height are buffered.
    pub fn has_future_headers(&self) -> bool {
        self.highest_header_index() > self.current_height()
    }

    /// Returns headers starting at `index_start`, up to `count` entries.
    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        if count == 0 {
            return Vec::new();
        }

        let headers = self.headers_by_index.read();
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

    /// Returns all transaction hashes currently tracked by the mempool cache.
    pub fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        self.transactions_by_hash.read().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::signer::Signer;
    use crate::network::p2p::payloads::witness::Witness;
    use crate::{UInt160, WitnessScope};
    use neo_vm_rs::OpCode;
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

    #[test]
    fn detects_future_headers() {
        let context = LedgerContext::default();
        assert!(!context.has_future_headers());

        {
            let mut headers = context.headers_by_index.write();
            headers.resize(3, None);
            let mut header = Header::new();
            header.set_index(2);
            headers[2] = Some(header);
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
}
