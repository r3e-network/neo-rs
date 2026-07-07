//! Public indexing and revert commands for `IndexerService`.

use neo_payloads::{ApplicationExecuted, Block};
use neo_primitives::UInt256;

use super::IndexerService;
use crate::error::IndexerResult;
use crate::model::{BlockIndexRecord, NotificationIndexRecord};

impl IndexerService {
    /// Indexes a canonical block.
    pub fn index_block(&self, block: &Block) -> IndexerResult<BlockIndexRecord> {
        self.mutate_indexer(|indexer| {
            let record = indexer.index_block(block)?;
            Ok((record, true))
        })
    }

    /// Indexes a canonical block and its emitted smart-contract notifications.
    pub fn index_block_with_application_executions(
        &self,
        block: &Block,
        executions: &[ApplicationExecuted],
    ) -> IndexerResult<BlockIndexRecord> {
        self.mutate_indexer(|indexer| {
            let record = indexer.index_block_with_application_executions(block, executions)?;
            Ok((record, true))
        })
    }

    /// Indexes a canonical block with notification records recovered from
    /// durable plugin data.
    pub fn index_block_with_notification_records(
        &self,
        block: &Block,
        notifications: Vec<NotificationIndexRecord>,
    ) -> IndexerResult<BlockIndexRecord> {
        self.mutate_indexer(|indexer| {
            let record = indexer.index_block_with_notification_records(block, notifications)?;
            Ok((record, true))
        })
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
