use std::sync::Arc;

use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_storage::persistence::{Store, providers::RuntimeStore};

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

    /// Returns whether a live block hash-links to the exact next height after
    /// the current contiguous in-memory projection tip.
    ///
    /// This intentionally reads the service's synchronized projection instead
    /// of scanning the durable store. Commit hooks use it on the hot path to
    /// avoid creating an ahead-of-stage gap while historical indexing runs.
    pub fn can_append_contiguous_block(&self, block: &Block) -> bool {
        self.read_indexer(|indexer| {
            let status = indexer.status();
            let Some(indexed_height) = status.indexed_height else {
                return block.index() == 0 && *block.prev_hash() == UInt256::zero();
            };
            let expected_blocks = u64::from(indexed_height).saturating_add(1);
            let contiguous =
                u64::try_from(status.indexed_blocks).unwrap_or(u64::MAX) == expected_blocks;
            contiguous
                && indexed_height
                    .checked_add(1)
                    .is_some_and(|next| block.index() == next)
                && status.indexed_hash == Some(*block.prev_hash())
        })
    }

    /// Returns aggregate indexer status.
    pub fn status(&self) -> IndexerStatus {
        self.projection_checkpoint()
    }

    /// Returns the synchronized in-memory projection checkpoint in O(1).
    ///
    /// Every persistent mutation updates the durable backend before returning
    /// and rolls this projection back on failure. The node's Index stage uses
    /// this view for per-block control flow, then calls `flush_durable` before
    /// treating a newly advanced checkpoint as durable. The same synchronized
    /// counters serve operator status without scanning every persisted row.
    pub fn projection_checkpoint(&self) -> IndexerStatus {
        self.read_indexer(Indexer::status)
    }

    /// Returns aggregate indexer status, surfacing service-store read errors.
    pub fn try_status(&self) -> IndexerResult<IndexerStatus> {
        let Some(store) = self.store_backend() else {
            return Ok(self.projection_checkpoint());
        };

        // Writers take this lock before changing the store and synchronized
        // projection. Read both under the same lock so a concurrent commit
        // cannot turn a valid checkpoint into a transient mismatch.
        let _persist_guard = self.persist_lock.lock();
        let status = self.projection_checkpoint();
        let (Some(height), Some(expected_hash)) = (status.indexed_height, status.indexed_hash)
        else {
            return Ok(status);
        };
        let snapshot = store.snapshot();
        let record = store::get_record::<BlockIndexRecord>(
            snapshot.as_ref(),
            store::block_by_height_key(height),
        )?
        .ok_or(crate::IndexerError::MissingCheckpointBlock { height })?;
        if record.hash != expected_hash {
            return Err(crate::IndexerError::CheckpointBlockMismatch {
                height,
                expected: expected_hash,
                actual: record.hash,
            });
        }
        Ok(status)
    }

    pub(super) fn read_indexer<T>(&self, read: impl FnOnce(&Indexer) -> T) -> T {
        let indexer = self.inner.read();
        read(&indexer)
    }

    fn store_backend(&self) -> Option<Arc<RuntimeStore>> {
        self.persistence
            .as_deref()
            .and_then(PersistenceBackend::store_backend)
    }

    pub(super) fn read_store_or_indexer<T>(
        &self,
        read_store: impl FnOnce(&<RuntimeStore as Store>::Snapshot) -> IndexerResult<T>,
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
