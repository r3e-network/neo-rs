//! Prepared block and notification application into the in-memory indexes.

use super::Indexer;
use super::block::PreparedBlock;
use crate::model::{AccountTransactionRecord, BlockIndexRecord, NotificationIndexRecord};

impl Indexer {
    pub(super) fn index_notification_accounts(&mut self, notification: &NotificationIndexRecord) {
        for account in &notification.accounts {
            self.account_notifications
                .entry(*account)
                .or_default()
                .push(notification.clone());
        }
    }

    pub(super) fn apply_prepared_block(&mut self, prepared: PreparedBlock) -> BlockIndexRecord {
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
