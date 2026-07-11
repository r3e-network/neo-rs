//! Snapshot hydration and export for the in-memory indexer.

use std::collections::{HashMap, HashSet};

use neo_primitives::UInt256;

use super::Indexer;
use super::notifications::{normalize_notification_payload_metadata, sort_notifications};
use crate::error::{IndexerError, IndexerResult};
use crate::model::{AccountTransactionRecord, INDEXER_SNAPSHOT_VERSION, IndexerSnapshot};

impl Indexer {
    /// Rebuilds the in-memory lookup tables from a persisted snapshot.
    pub fn from_snapshot(snapshot: IndexerSnapshot) -> IndexerResult<Self> {
        if snapshot.version != INDEXER_SNAPSHOT_VERSION {
            return Err(IndexerError::UnsupportedSnapshotVersion {
                version: snapshot.version,
            });
        }
        let mut indexer = Self::new();
        for block in snapshot.blocks {
            if indexer.blocks_by_hash.contains_key(&block.hash) {
                return Err(IndexerError::DuplicateBlockHash { hash: block.hash });
            }
            if indexer.block_hash_by_height.contains_key(&block.height) {
                return Err(IndexerError::DuplicateBlockHeight {
                    height: block.height,
                });
            }

            indexer
                .block_hash_by_height
                .insert(block.height, block.hash);
            indexer.tx_hashes_by_block.entry(block.hash).or_default();
            indexer.blocks_by_hash.insert(block.hash, block);
        }

        let mut seen_transaction_positions: HashMap<UInt256, HashSet<u32>> = HashMap::new();
        for transaction in snapshot.transactions {
            let Some(block) = indexer.blocks_by_hash.get(&transaction.block_hash) else {
                return Err(IndexerError::MissingTransactionBlock {
                    hash: transaction.hash,
                    block_hash: transaction.block_hash,
                });
            };
            if block.height != transaction.block_height {
                return Err(IndexerError::TransactionBlockHeightMismatch {
                    hash: transaction.hash,
                    block_hash: transaction.block_hash,
                    transaction_height: transaction.block_height,
                    block_height: block.height,
                });
            }
            if transaction.transaction_index >= block.transaction_count {
                return Err(IndexerError::TransactionIndexOutOfBounds {
                    hash: transaction.hash,
                    block_hash: transaction.block_hash,
                    transaction_index: transaction.transaction_index,
                    transaction_count: block.transaction_count,
                });
            }
            if !seen_transaction_positions
                .entry(transaction.block_hash)
                .or_default()
                .insert(transaction.transaction_index)
            {
                return Err(IndexerError::DuplicateTransactionPosition {
                    block_hash: transaction.block_hash,
                    block_height: transaction.block_height,
                    transaction_index: transaction.transaction_index,
                });
            }
            if indexer.transactions_by_hash.contains_key(&transaction.hash) {
                return Err(IndexerError::DuplicateTransaction {
                    hash: transaction.hash,
                });
            }

            for account in &transaction.signers {
                indexer
                    .account_transactions
                    .entry(*account)
                    .or_default()
                    .push(AccountTransactionRecord {
                        account: *account,
                        tx_hash: transaction.hash,
                        block_hash: transaction.block_hash,
                        block_height: transaction.block_height,
                        transaction_index: transaction.transaction_index,
                    });
            }
            indexer
                .tx_hashes_by_block
                .entry(transaction.block_hash)
                .or_default()
                .push(transaction.hash);
            indexer
                .transactions_by_hash
                .insert(transaction.hash, transaction);
        }
        for (height, block_hash) in &indexer.block_hash_by_height {
            let Some(block) = indexer.blocks_by_hash.get(block_hash) else {
                return Err(IndexerError::MissingHeightIndexBlock {
                    height: *height,
                    block_hash: *block_hash,
                });
            };
            let tx_hashes = indexer.tx_hashes_by_block.entry(*block_hash).or_default();
            if tx_hashes.len() != block.transaction_count as usize {
                return Err(IndexerError::TransactionCountMismatch {
                    block_hash: *block_hash,
                    block_height: block.height,
                    expected: block.transaction_count,
                    actual: tx_hashes.len(),
                });
            }
            for tx_hash in tx_hashes.iter() {
                if !indexer.transactions_by_hash.contains_key(tx_hash) {
                    return Err(IndexerError::MissingBlockTransactionRecord {
                        tx_hash: *tx_hash,
                        block_hash: *block_hash,
                        block_height: block.height,
                    });
                }
            }
            tx_hashes.sort_by_key(|tx_hash| {
                indexer
                    .transactions_by_hash
                    .get(tx_hash)
                    .map_or(u32::MAX, |record| record.transaction_index)
            });
        }

        let mut seen_notifications = HashSet::with_capacity(snapshot.notifications.len());
        for mut notification in snapshot.notifications {
            let coordinate = (
                notification.block_hash,
                notification.execution_index,
                notification.notification_index,
            );
            if !seen_notifications.insert(coordinate) {
                return Err(IndexerError::DuplicateNotification {
                    block_hash: notification.block_hash,
                    execution_index: notification.execution_index,
                    notification_index: notification.notification_index,
                });
            }

            let Some(block) = indexer.blocks_by_hash.get(&notification.block_hash) else {
                return Err(IndexerError::MissingNotificationBlock {
                    block_hash: notification.block_hash,
                    block_height: notification.block_height,
                    execution_index: notification.execution_index,
                    notification_index: notification.notification_index,
                });
            };
            if block.height != notification.block_height {
                return Err(IndexerError::NotificationBlockHeightMismatch {
                    block_hash: notification.block_hash,
                    notification_height: notification.block_height,
                    block_height: block.height,
                    execution_index: notification.execution_index,
                    notification_index: notification.notification_index,
                });
            }

            if let Some(tx_hash) = notification.tx_hash {
                let Some(transaction) = indexer.transactions_by_hash.get(&tx_hash) else {
                    return Err(IndexerError::MissingNotificationTransaction {
                        tx_hash,
                        block_hash: notification.block_hash,
                        execution_index: notification.execution_index,
                        notification_index: notification.notification_index,
                    });
                };
                if transaction.block_hash != notification.block_hash {
                    return Err(IndexerError::NotificationTransactionBlockMismatch {
                        tx_hash,
                        transaction_block_hash: transaction.block_hash,
                        block_hash: notification.block_hash,
                        execution_index: notification.execution_index,
                        notification_index: notification.notification_index,
                    });
                }
            }

            normalize_notification_payload_metadata(&mut notification)?;
            indexer.index_notification_accounts(&notification);
            indexer.notifications.push(notification);
        }

        Ok(indexer)
    }

    /// Returns a portable snapshot of the current index.
    pub fn snapshot(&self) -> IndexerSnapshot {
        let blocks = self
            .block_hash_by_height
            .values()
            .filter_map(|hash| self.blocks_by_hash.get(hash))
            .cloned()
            .collect::<Vec<_>>();

        let transactions = self
            .block_hash_by_height
            .values()
            .filter_map(|block_hash| self.tx_hashes_by_block.get(block_hash))
            .flat_map(|tx_hashes| tx_hashes.iter())
            .filter_map(|tx_hash| self.transactions_by_hash.get(tx_hash))
            .cloned()
            .collect::<Vec<_>>();

        let mut notifications = self.notifications.clone();
        sort_notifications(&mut notifications);

        IndexerSnapshot::with_notifications(blocks, transactions, notifications)
    }
}
