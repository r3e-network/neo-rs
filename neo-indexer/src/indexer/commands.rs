//! Public block and notification indexing commands.

use std::collections::HashSet;

use neo_payloads::{ApplicationExecuted, Block};

use super::Indexer;
use super::block::prepare_block;
use super::notifications::{normalize_notification_records, prepare_notifications};
use crate::error::IndexerResult;
use crate::model::{BlockIndexRecord, NotificationIndexRecord};

impl Indexer {
    /// Indexes a canonical block, replacing any previous block at the same
    /// height. Replacing by height lets the service handle local reorg repair
    /// without leaking stale transaction or account records.
    pub fn index_block(&mut self, block: &Block) -> IndexerResult<BlockIndexRecord> {
        let prepared = prepare_block(block)?;
        Ok(self.apply_prepared_block(prepared))
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
        let prepared = prepare_block(block)?;
        let block_transactions = prepared
            .transactions
            .iter()
            .map(|transaction| transaction.hash)
            .collect::<HashSet<_>>();
        let notifications =
            prepare_notifications(&prepared.block, &block_transactions, executions)?;
        let block_record = self.apply_prepared_block(prepared);
        for notification in notifications {
            self.index_notification_accounts(&notification);
            self.notifications.push(notification);
        }
        Ok(block_record)
    }

    /// Indexes a canonical block with already materialized notification
    /// records.
    ///
    /// This is used by daemon backfill paths that can recover historical
    /// notifications from durable plugin data but no longer have the original
    /// `ApplicationExecuted` values in memory.
    pub fn index_block_with_notification_records(
        &mut self,
        block: &Block,
        notifications: Vec<NotificationIndexRecord>,
    ) -> IndexerResult<BlockIndexRecord> {
        let prepared = prepare_block(block)?;
        let notifications =
            normalize_notification_records(&prepared.block, &prepared.transactions, notifications)?;
        let block_record = self.apply_prepared_block(prepared);
        for notification in notifications {
            self.index_notification_accounts(&notification);
            self.notifications.push(notification);
        }
        Ok(block_record)
    }
}
