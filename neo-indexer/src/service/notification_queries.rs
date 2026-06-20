use std::collections::HashSet;

use neo_primitives::{UInt160, UInt256};

use crate::error::IndexerResult;
use crate::model::{AccountTransactionRecord, NotificationIndexRecord, TransactionIndexRecord};
use crate::store;

use super::IndexerService;

impl IndexerService {
    /// Returns transactions that emitted notifications from `contract_hash`.
    pub fn transactions_for_contract(
        &self,
        contract_hash: &UInt160,
        event_name: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> Vec<TransactionIndexRecord> {
        self.try_transactions_for_contract(contract_hash, event_name, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| {
                    indexer.transactions_for_contract(contract_hash, event_name, skip, limit)
                })
            })
    }

    /// Returns transactions that emitted notifications from `contract_hash`,
    /// surfacing service-store read errors.
    pub fn try_transactions_for_contract(
        &self,
        contract_hash: &UInt160,
        event_name: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<TransactionIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                let mut seen = HashSet::new();
                let mut skipped = 0usize;
                let mut records = Vec::new();
                for notification in store::read_record_prefix_filtered(
                    snapshot,
                    &store::notification_by_contract_prefix(contract_hash),
                    |record: &NotificationIndexRecord| match event_name {
                        Some(event) => record.event_name == event,
                        None => true,
                    },
                )? {
                    let Some(tx_hash) = notification.tx_hash else {
                        continue;
                    };
                    if !seen.insert(tx_hash) {
                        continue;
                    }
                    if skipped < skip {
                        skipped += 1;
                        continue;
                    }
                    if records.len() >= limit {
                        break;
                    }
                    if let Some(transaction) =
                        store::get_record(snapshot, store::transaction_by_hash_key(&tx_hash))?
                    {
                        records.push(transaction);
                    }
                }
                Ok(records)
            },
            |indexer| indexer.transactions_for_contract(contract_hash, event_name, skip, limit),
        )
    }

    /// Returns smart-contract notifications for a block in execution order.
    pub fn notifications_for_block(
        &self,
        block_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        self.try_notifications_for_block(block_hash, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| {
                    indexer.notifications_for_block(block_hash, skip, limit)
                })
            })
    }

    /// Returns smart-contract notifications for a block, surfacing errors.
    pub fn try_notifications_for_block(
        &self,
        block_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<NotificationIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                store::read_record_page(
                    snapshot,
                    &store::notification_by_block_prefix(block_hash),
                    skip,
                    limit,
                )
            },
            |indexer| indexer.notifications_for_block(block_hash, skip, limit),
        )
    }

    /// Returns smart-contract notifications for a transaction in execution
    /// order.
    pub fn notifications_for_transaction(
        &self,
        tx_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        self.try_notifications_for_transaction(tx_hash, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| {
                    indexer.notifications_for_transaction(tx_hash, skip, limit)
                })
            })
    }

    /// Returns smart-contract notifications for a transaction, surfacing errors.
    pub fn try_notifications_for_transaction(
        &self,
        tx_hash: &UInt256,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<NotificationIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                store::read_record_page(
                    snapshot,
                    &store::notification_by_transaction_prefix(tx_hash),
                    skip,
                    limit,
                )
            },
            |indexer| indexer.notifications_for_transaction(tx_hash, skip, limit),
        )
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
        self.try_notifications_for_contract(contract_hash, event_name, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| {
                    indexer.notifications_for_contract(contract_hash, event_name, skip, limit)
                })
            })
    }

    /// Returns smart-contract notifications emitted by a contract, surfacing
    /// service-store read errors.
    pub fn try_notifications_for_contract(
        &self,
        contract_hash: &UInt160,
        event_name: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<NotificationIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                store::read_record_page_filtered(
                    snapshot,
                    &store::notification_by_contract_prefix(contract_hash),
                    |record: &NotificationIndexRecord| match event_name {
                        Some(event) => record.event_name == event,
                        None => true,
                    },
                    skip,
                    limit,
                )
            },
            |indexer| indexer.notifications_for_contract(contract_hash, event_name, skip, limit),
        )
    }

    /// Returns Transfer notifications involving `account` in ascending chain
    /// order.
    pub fn notifications_for_account(
        &self,
        account: &UInt160,
        skip: usize,
        limit: usize,
    ) -> Vec<NotificationIndexRecord> {
        self.try_notifications_for_account(account, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| indexer.notifications_for_account(account, skip, limit))
            })
    }

    /// Returns Transfer notifications involving `account`, surfacing errors.
    pub fn try_notifications_for_account(
        &self,
        account: &UInt160,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<NotificationIndexRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                store::read_record_page(
                    snapshot,
                    &store::notification_by_account_prefix(account),
                    skip,
                    limit,
                )
            },
            |indexer| indexer.notifications_for_account(account, skip, limit),
        )
    }

    /// Returns account-related transactions in ascending chain order.
    pub fn transactions_for_account(
        &self,
        account: &UInt160,
        skip: usize,
        limit: usize,
    ) -> Vec<AccountTransactionRecord> {
        self.try_transactions_for_account(account, skip, limit)
            .unwrap_or_else(|_| {
                self.read_indexer(|indexer| indexer.transactions_for_account(account, skip, limit))
            })
    }

    /// Returns account-related transactions, surfacing service-store errors.
    pub fn try_transactions_for_account(
        &self,
        account: &UInt160,
        skip: usize,
        limit: usize,
    ) -> IndexerResult<Vec<AccountTransactionRecord>> {
        self.read_store_or_indexer(
            |snapshot| {
                store::read_record_page(
                    snapshot,
                    &store::account_transaction_prefix(account),
                    skip,
                    limit,
                )
            },
            |indexer| indexer.transactions_for_account(account, skip, limit),
        )
    }
}
