//! Canonical-chain removal and reorg cleanup for the in-memory indexer.

use neo_primitives::{UInt160, UInt256};

use super::Indexer;
use crate::model::BlockIndexRecord;

impl Indexer {
    /// Removes an indexed block by hash and drops all derived transaction and
    /// account records.
    pub fn remove_block_by_hash(&mut self, hash: &UInt256) -> Option<BlockIndexRecord> {
        let block = self.blocks_by_hash.remove(hash)?;
        self.block_hash_by_height.remove(&block.height);

        if let Some(tx_hashes) = self.tx_hashes_by_block.remove(hash) {
            let mut touched_accounts = Vec::new();
            for tx_hash in tx_hashes {
                if let Some(transaction) = self.transactions_by_hash.remove(&tx_hash) {
                    touched_accounts.extend(transaction.signers);
                }
            }

            touched_accounts.sort_by_key(UInt160::to_array);
            touched_accounts.dedup();
            for account in touched_accounts {
                if let Some(records) = self.account_transactions.get_mut(&account) {
                    records.retain(|record| record.block_hash != *hash);
                    if records.is_empty() {
                        self.account_transactions.remove(&account);
                    }
                }
            }
        }

        let removed_notifications = self
            .notifications
            .iter()
            .filter(|record| record.block_hash == *hash)
            .cloned()
            .collect::<Vec<_>>();
        self.notifications
            .retain(|record| record.block_hash != *hash);
        let mut touched_notification_accounts = removed_notifications
            .iter()
            .flat_map(|record| record.accounts.iter().copied())
            .collect::<Vec<_>>();
        touched_notification_accounts.sort_by_key(UInt160::to_array);
        touched_notification_accounts.dedup();
        for account in touched_notification_accounts {
            if let Some(records) = self.account_notifications.get_mut(&account) {
                records.retain(|record| record.block_hash != *hash);
                if records.is_empty() {
                    self.account_notifications.remove(&account);
                }
            }
        }

        Some(block)
    }

    /// Removes an indexed block by height.
    pub fn remove_block_at_height(&mut self, height: u32) -> Option<BlockIndexRecord> {
        let hash = self.block_hash_by_height.get(&height).copied()?;
        self.remove_block_by_hash(&hash)
    }

    /// Removes all indexed blocks above `height`.
    pub fn revert_to_height(&mut self, height: u32) -> Vec<BlockIndexRecord> {
        if height == u32::MAX {
            return Vec::new();
        }
        let from_height = height.saturating_add(1);
        let hashes = self
            .block_hash_by_height
            .range(from_height..)
            .map(|(_, hash)| *hash)
            .collect::<Vec<_>>();

        hashes
            .iter()
            .filter_map(|hash| self.remove_block_by_hash(hash))
            .collect()
    }
}
