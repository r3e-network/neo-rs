//! # neo-indexer::indexer
//!
//! Indexer workers and projection logic for chain-derived data.
//!
//! ## Boundary
//!
//! This module belongs to `neo-indexer`. This service crate owns projections
//! over committed chain data and must not decide block validity or consensus
//! outcomes.
//!
//! ## Contents
//!
//! - `notifications`: notification projection and query logic.
//! - `query`: query APIs for indexed data.
//! - `reorg`: reorg-aware index update helpers.
//! - `snapshot`: Read snapshot view for the surrounding store backend.
//! - `tests`: Module-local tests and regression coverage.

mod notifications;
mod query;
mod reorg;
mod snapshot;

use std::collections::{BTreeMap, HashMap, HashSet};

use neo_payloads::{ApplicationExecuted, Block};
use neo_primitives::{UInt160, UInt256};

use crate::error::{IndexerError, IndexerResult};
use crate::model::{
    AccountTransactionRecord, BlockIndexRecord, NotificationIndexRecord, TransactionIndexRecord,
};

use notifications::{normalize_notification_records, prepare_notifications};

#[derive(Debug)]
struct PreparedBlock {
    block: BlockIndexRecord,
    transactions: Vec<TransactionIndexRecord>,
}

/// Mutable in-memory index over canonical blocks and transactions.
#[derive(Debug, Default)]
pub struct Indexer {
    blocks_by_hash: HashMap<UInt256, BlockIndexRecord>,
    block_hash_by_height: BTreeMap<u32, UInt256>,
    transactions_by_hash: HashMap<UInt256, TransactionIndexRecord>,
    tx_hashes_by_block: HashMap<UInt256, Vec<UInt256>>,
    account_transactions: HashMap<UInt160, Vec<AccountTransactionRecord>>,
    account_notifications: HashMap<UInt160, Vec<NotificationIndexRecord>>,
    notifications: Vec<NotificationIndexRecord>,
}

impl Indexer {
    /// Creates an empty index.
    pub fn new() -> Self {
        Self::default()
    }

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

    fn index_notification_accounts(&mut self, notification: &NotificationIndexRecord) {
        for account in &notification.accounts {
            self.account_notifications
                .entry(*account)
                .or_default()
                .push(notification.clone());
        }
    }

    fn apply_prepared_block(&mut self, prepared: PreparedBlock) -> BlockIndexRecord {
        let block_hash = prepared.block.hash;
        let block_height = prepared.block.height;

        if let Some(existing_hash) = self.block_hash_by_height.get(&block_height).copied() {
            self.remove_block_by_hash(&existing_hash);
        }
        if self.blocks_by_hash.contains_key(&block_hash) {
            self.remove_block_by_hash(&block_hash);
        }

        let tx_hashes = prepared
            .transactions
            .iter()
            .map(|transaction| transaction.hash)
            .collect::<Vec<_>>();

        self.block_hash_by_height.insert(block_height, block_hash);
        self.tx_hashes_by_block.insert(block_hash, tx_hashes);

        for transaction in prepared.transactions {
            for account in &transaction.signers {
                self.account_transactions.entry(*account).or_default().push(
                    AccountTransactionRecord {
                        account: *account,
                        tx_hash: transaction.hash,
                        block_hash,
                        block_height,
                        transaction_index: transaction.transaction_index,
                    },
                );
            }
            self.transactions_by_hash
                .insert(transaction.hash, transaction);
        }

        self.blocks_by_hash
            .insert(block_hash, prepared.block.clone());
        prepared.block
    }
}

fn prepare_block(block: &Block) -> IndexerResult<PreparedBlock> {
    let block_hash = block
        .try_hash()
        .map_err(|source| IndexerError::BlockHash { source })?;
    let transaction_count =
        u32::try_from(block.transactions.len()).map_err(|_| IndexerError::TooManyTransactions {
            count: block.transactions.len(),
        })?;

    let mut transactions = Vec::with_capacity(block.transactions.len());
    let mut seen_transactions = HashSet::with_capacity(block.transactions.len());
    for (position, transaction) in block.transactions.iter().enumerate() {
        let transaction_index =
            u32::try_from(position).map_err(|_| IndexerError::TooManyTransactions {
                count: block.transactions.len(),
            })?;
        let hash = transaction
            .try_hash()
            .map_err(|source| IndexerError::TransactionHash {
                index: transaction_index,
                source,
            })?;
        if !seen_transactions.insert(hash) {
            return Err(IndexerError::DuplicateTransaction { hash });
        }

        let mut seen_accounts = HashSet::new();
        let mut signers = Vec::new();
        for signer in transaction.signers() {
            if seen_accounts.insert(signer.account) {
                signers.push(signer.account);
            }
        }

        transactions.push(TransactionIndexRecord {
            hash,
            block_hash,
            block_height: block.index(),
            transaction_index,
            signers,
        });
    }

    Ok(PreparedBlock {
        block: BlockIndexRecord {
            hash: block_hash,
            height: block.index(),
            timestamp: block.timestamp(),
            transaction_count,
        },
        transactions,
    })
}

#[cfg(test)]
#[path = "../tests/indexer/mod.rs"]
mod tests;
