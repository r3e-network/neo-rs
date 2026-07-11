//! Public block and notification indexing commands.

use std::collections::HashSet;

use neo_payloads::{ApplicationExecuted, Block};

use super::Indexer;
use super::block::prepare_block;
use super::notifications::{normalize_notification_records, prepare_notifications};
use crate::error::IndexerResult;
use crate::model::{BlockIndexRecord, IndexBlockBatchEntry, NotificationIndexRecord};

pub(crate) struct PreparedBatchEntry {
    pub(crate) block: super::block::PreparedBlock,
    pub(crate) notifications: Vec<NotificationIndexRecord>,
}

pub(crate) struct PreparedIndexBatch {
    pub(crate) entries: Vec<PreparedBatchEntry>,
}

impl PreparedIndexBatch {
    pub(crate) fn single(entry: PreparedBatchEntry) -> Self {
        Self {
            entries: vec![entry],
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Indexer {
    /// Atomically indexes a prevalidated batch of canonical blocks.
    ///
    /// Every block and notification record is materialized before any index
    /// map is changed, so a malformed later entry cannot leave a partial batch.
    pub fn index_block_batch<'a>(
        &mut self,
        entries: impl IntoIterator<Item = IndexBlockBatchEntry<'a>>,
    ) -> IndexerResult<Vec<BlockIndexRecord>> {
        let prepared = Self::prepare_index_batch(entries)?;
        self.validate_prepared_entries(&prepared.entries)?;
        Ok(self.apply_prepared_batch(prepared))
    }

    pub(crate) fn prepare_index_batch<'a>(
        entries: impl IntoIterator<Item = IndexBlockBatchEntry<'a>>,
    ) -> IndexerResult<PreparedIndexBatch> {
        let entries = entries
            .into_iter()
            .map(|entry| {
                Self::prepare_notification_entry(
                    entry.block,
                    entry.notifications.unwrap_or_default(),
                )
            })
            .collect::<IndexerResult<Vec<_>>>()?;
        validate_prepared_batch(&entries)?;
        Ok(PreparedIndexBatch { entries })
    }

    pub(crate) fn prepare_block_entry(block: &Block) -> IndexerResult<PreparedBatchEntry> {
        Ok(PreparedBatchEntry {
            block: prepare_block(block)?,
            notifications: Vec::new(),
        })
    }

    pub(crate) fn prepare_application_entry(
        block: &Block,
        executions: &[ApplicationExecuted],
    ) -> IndexerResult<PreparedBatchEntry> {
        let block = prepare_block(block)?;
        let block_transactions = block
            .transactions
            .iter()
            .map(|transaction| transaction.hash)
            .collect::<HashSet<_>>();
        let notifications = prepare_notifications(&block.block, &block_transactions, executions)?;
        Ok(PreparedBatchEntry {
            block,
            notifications,
        })
    }

    pub(crate) fn prepare_notification_entry(
        block: &Block,
        notifications: Vec<NotificationIndexRecord>,
    ) -> IndexerResult<PreparedBatchEntry> {
        let block = prepare_block(block)?;
        let notifications =
            normalize_notification_records(&block.block, &block.transactions, notifications)?;
        Ok(PreparedBatchEntry {
            block,
            notifications,
        })
    }

    pub(crate) fn apply_prepared_batch(
        &mut self,
        prepared: PreparedIndexBatch,
    ) -> Vec<BlockIndexRecord> {
        let mut records = Vec::with_capacity(prepared.entries.len());
        for entry in prepared.entries {
            records.push(self.apply_prepared_entry(entry));
        }
        records
    }

    pub(crate) fn apply_prepared_entry(&mut self, entry: PreparedBatchEntry) -> BlockIndexRecord {
        let block = self.apply_prepared_block(entry.block);
        for notification in entry.notifications {
            self.index_notification_accounts(&notification);
            self.notifications.push(notification);
        }
        block
    }

    /// Indexes a canonical block, replacing any previous block at the same
    /// height. Replacing by height lets the service handle local reorg repair
    /// without leaking stale transaction or account records.
    pub fn index_block(&mut self, block: &Block) -> IndexerResult<BlockIndexRecord> {
        let prepared = Self::prepare_block_entry(block)?;
        self.validate_prepared_entries(std::slice::from_ref(&prepared))?;
        Ok(self.apply_prepared_entry(prepared))
    }

    /// Indexes a canonical block and its emitted smart-contract notifications.
    ///
    /// Re-indexing the same height or hash replaces the previous block,
    /// transaction, account, and notification records.
    pub fn index_block_with_application_executions(
        &mut self,
        block: &Block,
        executions: &[ApplicationExecuted],
    ) -> IndexerResult<BlockIndexRecord> {
        let prepared = Self::prepare_application_entry(block, executions)?;
        self.validate_prepared_entries(std::slice::from_ref(&prepared))?;
        Ok(self.apply_prepared_entry(prepared))
    }

    /// Indexes a canonical block with already materialized notification
    /// records.
    ///
    /// The durable Index stage uses this during catch-up when plugin records
    /// remain available but the original `ApplicationExecuted` values are no
    /// longer in memory.
    pub fn index_block_with_notification_records(
        &mut self,
        block: &Block,
        notifications: Vec<NotificationIndexRecord>,
    ) -> IndexerResult<BlockIndexRecord> {
        let prepared = Self::prepare_notification_entry(block, notifications)?;
        self.validate_prepared_entries(std::slice::from_ref(&prepared))?;
        Ok(self.apply_prepared_entry(prepared))
    }
}

fn validate_prepared_batch(entries: &[PreparedBatchEntry]) -> IndexerResult<()> {
    let mut heights = HashSet::with_capacity(entries.len());
    let mut block_hashes = HashSet::with_capacity(entries.len());
    let transaction_capacity = entries
        .iter()
        .map(|entry| entry.block.transactions.len())
        .sum();
    let mut transaction_hashes = HashSet::with_capacity(transaction_capacity);

    for entry in entries {
        if !heights.insert(entry.block.block.height) {
            return Err(crate::IndexerError::DuplicateBlockHeight {
                height: entry.block.block.height,
            });
        }
        if !block_hashes.insert(entry.block.block.hash) {
            return Err(crate::IndexerError::DuplicateBlockHash {
                hash: entry.block.block.hash,
            });
        }
        for transaction in &entry.block.transactions {
            if !transaction_hashes.insert(transaction.hash) {
                return Err(crate::IndexerError::DuplicateTransaction {
                    hash: transaction.hash,
                });
            }
        }
    }
    Ok(())
}
