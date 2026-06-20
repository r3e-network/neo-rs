use std::collections::HashSet;

use neo_primitives::{UInt160, UInt256};

use crate::model::{
    AccountTransactionRecord, BlockIndexRecord, IndexerStatus, NotificationIndexRecord,
    TransactionIndexRecord,
};

use super::Indexer;
use super::notifications::sort_notifications;

impl Indexer {
    /// Returns a block record by hash.
    pub fn block_by_hash(&self, hash: &UInt256) -> Option<BlockIndexRecord> {
        self.blocks_by_hash.get(hash).cloned()
    }

    /// Returns a block record by height.
    pub fn block_by_height(&self, height: u32) -> Option<BlockIndexRecord> {
        let hash = self.block_hash_by_height.get(&height)?;
        self.block_by_hash(hash)
    }

    /// Returns indexed blocks in ascending height order.
    pub fn blocks(&self, skip: usize, limit: usize) -> Vec<BlockIndexRecord> {
        collect_page(
            self.block_hash_by_height
                .values()
                .filter_map(|hash| self.blocks_by_hash.get(hash))
                .cloned(),
            skip,
            limit,
        )
    }

    /// Returns a transaction index record by hash.
    pub fn transaction(&self, hash: &UInt256) -> Option<TransactionIndexRecord> {
        self.transactions_by_hash.get(hash).cloned()
    }

    /// Returns transactions in a block in canonical transaction-index order.
    pub fn transactions_for_block(
        &self,
        block_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> Vec<TransactionIndexRecord> {
        let Some(tx_hashes) = self.tx_hashes_by_block.get(block_hash) else {
            return Vec::new();
        };
        collect_page(
            tx_hashes
                .iter()
                .filter_map(|hash| self.transactions_by_hash.get(hash))
                .cloned(),
            skip,
            limit,
        )
    }

    /// Returns transactions that emitted notifications from `contract_hash`.
    pub fn transactions_for_contract(
        &self,
        contract_hash: &UInt160,
        event_name: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> Vec<TransactionIndexRecord> {
        let mut seen = HashSet::new();
        let mut records = self
            .notifications
            .iter()
            .filter(|record| {
                record.contract_hash == *contract_hash
                    && match event_name {
                        Some(event) => record.event_name == event,
                        None => true,
                    }
            })
            .filter_map(|record| record.tx_hash)
            .filter(|tx_hash| seen.insert(*tx_hash))
            .filter_map(|tx_hash| self.transactions_by_hash.get(&tx_hash))
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by_key(|record| (record.block_height, record.transaction_index));
        collect_page(records, skip, limit)
    }

    /// Returns smart-contract notifications for a block in execution order.
    pub fn notifications_for_block(
        &self,
        block_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        let mut records = self
            .notifications
            .iter()
            .filter(|record| record.block_hash == *block_hash)
            .cloned()
            .collect::<Vec<_>>();
        sort_notifications(&mut records);
        collect_page(records, skip, limit)
    }

    /// Returns smart-contract notifications for a transaction in execution
    /// order.
    pub fn notifications_for_transaction(
        &self,
        tx_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        let mut records = self
            .notifications
            .iter()
            .filter(|record| record.tx_hash == Some(*tx_hash))
            .cloned()
            .collect::<Vec<_>>();
        sort_notifications(&mut records);
        collect_page(records, skip, limit)
    }

    /// Returns smart-contract notifications emitted by a contract, optionally
    /// filtered by event name.
    pub fn notifications_for_contract(
        &self,
        contract_hash: &UInt160,
        event_name: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        let mut records = self
            .notifications
            .iter()
            .filter(|record| {
                record.contract_hash == *contract_hash
                    && match event_name {
                        Some(event) => record.event_name == event,
                        None => true,
                    }
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_notifications(&mut records);
        collect_page(records, skip, limit)
    }

    /// Returns Transfer notifications involving `account` in ascending chain
    /// order.
    pub fn notifications_for_account(
        &self,
        account: &UInt160,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        let Some(records) = self.account_notifications.get(account) else {
            return Vec::new();
        };
        let mut records = records.clone();
        sort_notifications(&mut records);
        collect_page(records, skip, limit)
    }

    /// Returns account-related transactions in ascending chain order.
    pub fn transactions_for_account(
        &self,
        account: &UInt160,
        skip: usize,
        limit: usize,
    ) -> Vec<AccountTransactionRecord> {
        let Some(records) = self.account_transactions.get(account) else {
            return Vec::new();
        };
        let mut records = records.clone();
        records.sort_by_key(|record| (record.block_height, record.transaction_index));
        collect_page(records, skip, limit)
    }

    /// Returns aggregate indexer status.
    pub fn status(&self) -> IndexerStatus {
        let indexed_tip = self
            .block_hash_by_height
            .iter()
            .next_back()
            .map(|(height, hash)| (*height, *hash));
        IndexerStatus {
            indexed_height: indexed_tip.map(|(height, _)| height),
            indexed_hash: indexed_tip.map(|(_, hash)| hash),
            indexed_blocks: self.blocks_by_hash.len(),
            indexed_transactions: self.transactions_by_hash.len(),
            indexed_accounts: self.account_transactions.len(),
            indexed_notifications: self.notifications.len(),
            indexed_notification_accounts: self.account_notifications.len(),
        }
    }
}

fn collect_page<T>(records: impl IntoIterator<Item = T>, skip: usize, limit: usize) -> Vec<T> {
    records.into_iter().skip(skip).take(limit).collect()
}
