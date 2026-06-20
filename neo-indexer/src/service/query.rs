use std::sync::Arc;

use neo_primitives::UInt256;
use neo_storage::persistence::{Store, StoreSnapshot};

use crate::error::IndexerResult;
use crate::indexer::Indexer;
use crate::model::{BlockIndexRecord, IndexerStatus, TransactionIndexRecord};
use crate::store;

use super::{IndexerService, PersistenceBackend};

impl IndexerService {
    /// Returns a block record by hash.
    pub fn block_by_hash(&self, hash: &UInt256) -> Option<BlockIndexRecord> {
        self.try_block_by_hash(hash)
            .unwrap_or_else(|_| self.read_indexer(|indexer| indexer.block_by_hash(hash)))
    }

    /// Returns a block record by hash, surfacing service-store read errors.
    pub fn try_block_by_hash(&self, hash: &UInt256) -> IndexerResult<Option<BlockIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| store::get_record(snapshot, store::block_by_hash_key(hash)),
            |indexer| indexer.block_by_hash(hash),
        )
    }

    /// Returns a block record by height.
    pub fn block_by_height(&self, height: u32) -> Option<BlockIndexRecord> {
        self.try_block_by_height(height)
            .unwrap_or_else(|_| self.read_indexer(|indexer| indexer.block_by_height(height)))
    }

    /// Returns a block record by height, surfacing service-store read errors.
    pub fn try_block_by_height(&self, height: u32) -> IndexerResult<Option<BlockIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| store::get_record(snapshot, store::block_by_height_key(height)),
            |indexer| indexer.block_by_height(height),
        )
    }

    /// Returns indexed blocks in ascending height order.
    pub fn blocks(&self, skip: usize, limit: usize) -> Vec<BlockIndexRecord> {
        self.try_blocks(skip, limit)
            .unwrap_or_else(|_| self.read_indexer(|indexer| indexer.blocks(skip, limit)))
    }

    /// Returns indexed blocks in ascending height order, surfacing errors.
    pub fn try_blocks(&self, skip: usize, limit: usize) -> IndexerResult<Vec<BlockIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                store::read_record_page(snapshot, store::BLOCK_BY_HEIGHT_PREFIX, skip, limit)
            },
            |indexer| indexer.blocks(skip, limit),
        )
    }

    /// Returns a transaction record by hash.
    pub fn transaction(&self, hash: &UInt256) -> Option<TransactionIndexRecord> {
        self.try_transaction(hash)
            .unwrap_or_else(|_| self.read_indexer(|indexer| indexer.transaction(hash)))
    }

    /// Returns a transaction record by hash, surfacing service-store errors.
    pub fn try_transaction(&self, hash: &UInt256) -> IndexerResult<Option<TransactionIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| store::get_record(snapshot, store::transaction_by_hash_key(hash)),
            |indexer| indexer.transaction(hash),
        )
    }

    /// Returns transactions in a block in canonical transaction-index order.
    pub fn transactions_for_block(
        &self,
        block_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> Vec<TransactionIndexRecord> {
        self.try_transactions_for_block(block_hash, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| indexer.transactions_for_block(block_hash, skip, limit))
            })
    }

    /// Returns transactions in a block, surfacing service-store read errors.
    pub fn try_transactions_for_block(
        &self,
        block_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<TransactionIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                let Some(block) = store::get_record::<BlockIndexRecord>(
                    snapshot,
                    store::block_by_hash_key(block_hash),
                )?
                else {
                    return Ok(Vec::new());
                };
                store::read_record_page_filtered(
                    snapshot,
                    &store::transaction_by_block_prefix(block.height),
                    |record: &TransactionIndexRecord| record.block_hash == *block_hash,
                    skip,
                    limit,
                )
            },
            |indexer| indexer.transactions_for_block(block_hash, skip, limit),
        )
    }

    /// Returns aggregate indexer status.
    pub fn status(&self) -> IndexerStatus {
        self.try_status()
            .unwrap_or_else(|_| self.read_indexer(Indexer::status))
    }

    /// Returns aggregate indexer status, surfacing service-store read errors.
    pub fn try_status(&self) -> IndexerResult<IndexerStatus> {
        self.read_store_or_indexer(store::status, Indexer::status)
    }

    pub(super) fn read_indexer<T>(&self, read: impl FnOnce(&Indexer) -> T) -> T {
        let indexer = self.inner.read();
        read(&indexer)
    }

    fn store_backend(&self) -> Option<Arc<dyn Store>> {
        match self.persistence.as_deref() {
            Some(PersistenceBackend::Store { store, .. }) => Some(Arc::clone(store)),
            _ => None,
        }
    }

    pub(super) fn read_store_or_indexer<T>(
        &self,
        read_store: impl FnOnce(&dyn StoreSnapshot) -> IndexerResult<T>,
        read_memory: impl FnOnce(&Indexer) -> T,
    ) -> IndexerResult<T> {
        if let Some(store) = self.store_backend() {
            let _persist_guard = self.persist_lock.lock();
            let snapshot = store.snapshot();
            return read_store(snapshot.as_ref());
        }
        Ok(self.read_indexer(read_memory))
    }
}
