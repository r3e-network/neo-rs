//! Public indexing and revert commands for `IndexerService`.

use neo_payloads::{ApplicationExecuted, Block};
use neo_primitives::UInt256;

use super::IndexerService;
use crate::error::IndexerResult;
use crate::indexer::{Indexer, PreparedIndexBatch};
use crate::model::{BlockIndexRecord, IndexBlockBatchEntry, NotificationIndexRecord};

impl IndexerService {
    /// Clears the canonical projection through the configured persistence
    /// backend.
    ///
    /// Stage recovery uses this before rebuilding an invalid checkpoint so no
    /// stale rows beyond the next durable batch remain queryable.
    pub fn clear(&self) -> IndexerResult<()> {
        self.mutate_indexer(|indexer| {
            *indexer = crate::indexer::Indexer::new();
            Ok(((), true))
        })
    }

    /// Indexes a canonical block batch in one persistence transaction.
    pub fn index_block_batch<'a>(
        &self,
        entries: impl IntoIterator<Item = IndexBlockBatchEntry<'a>>,
    ) -> IndexerResult<Vec<BlockIndexRecord>> {
        let prepared = Indexer::prepare_index_batch(entries)?;
        self.commit_prepared_batch(prepared)
    }

    /// Indexes a canonical block.
    pub fn index_block(&self, block: &Block) -> IndexerResult<BlockIndexRecord> {
        let prepared = PreparedIndexBatch::single(Indexer::prepare_block_entry(block)?);
        single_block_result(self.commit_prepared_batch(prepared)?)
    }

    /// Indexes a canonical block and its emitted smart-contract notifications.
    pub fn index_block_with_application_executions(
        &self,
        block: &Block,
        executions: &[ApplicationExecuted],
    ) -> IndexerResult<BlockIndexRecord> {
        let prepared =
            PreparedIndexBatch::single(Indexer::prepare_application_entry(block, executions)?);
        single_block_result(self.commit_prepared_batch(prepared)?)
    }

    /// Indexes a canonical block with notification records recovered from
    /// durable plugin data.
    pub fn index_block_with_notification_records(
        &self,
        block: &Block,
        notifications: Vec<NotificationIndexRecord>,
    ) -> IndexerResult<BlockIndexRecord> {
        let prepared =
            PreparedIndexBatch::single(Indexer::prepare_notification_entry(block, notifications)?);
        single_block_result(self.commit_prepared_batch(prepared)?)
    }

    /// Removes an indexed block by hash.
    pub fn remove_block_by_hash(&self, hash: &UInt256) -> IndexerResult<Option<BlockIndexRecord>> {
        self.mutate_indexer(|indexer| {
            let removed = indexer.remove_block_by_hash(hash);
            let should_persist = removed.is_some();
            Ok((removed, should_persist))
        })
    }

    /// Removes an indexed block by height.
    pub fn remove_block_at_height(&self, height: u32) -> IndexerResult<Option<BlockIndexRecord>> {
        self.mutate_indexer(|indexer| {
            let removed = indexer.remove_block_at_height(height);
            let should_persist = removed.is_some();
            Ok((removed, should_persist))
        })
    }

    /// Removes all indexed blocks above `height`.
    pub fn revert_to_height(&self, height: u32) -> IndexerResult<Vec<BlockIndexRecord>> {
        self.mutate_indexer(|indexer| {
            let removed = indexer.revert_to_height(height);
            let should_persist = !removed.is_empty();
            Ok((removed, should_persist))
        })
    }
}

fn single_block_result(mut records: Vec<BlockIndexRecord>) -> IndexerResult<BlockIndexRecord> {
    records
        .pop()
        .ok_or(crate::IndexerError::MissingPreparedBlockResult)
}
