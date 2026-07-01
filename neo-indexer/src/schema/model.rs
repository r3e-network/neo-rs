//! Public index records exposed by the Neo indexer service.

use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version tag for persisted indexer snapshots.
///
/// Version 2 adds notification payload JSON plus Transfer participant account
/// indexes. Version 1 snapshots are still accepted and migrated on load when
/// their notification payload JSON is available.
pub const INDEXER_SNAPSHOT_VERSION: u32 = 2;

/// Canonical block index entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockIndexRecord {
    /// Block hash.
    pub hash: UInt256,
    /// Block height.
    pub height: u32,
    /// Block timestamp in milliseconds since Unix epoch, matching Neo headers.
    pub timestamp: u64,
    /// Number of transactions in the block.
    pub transaction_count: u32,
}

/// Canonical transaction index entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionIndexRecord {
    /// Transaction hash.
    pub hash: UInt256,
    /// Containing block hash.
    pub block_hash: UInt256,
    /// Containing block height.
    pub block_height: u32,
    /// Transaction position inside the block.
    pub transaction_index: u32,
    /// Signer accounts declared by the transaction.
    pub signers: Vec<UInt160>,
}

/// Account-to-transaction index entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountTransactionRecord {
    /// Account script hash.
    pub account: UInt160,
    /// Transaction hash.
    pub tx_hash: UInt256,
    /// Containing block hash.
    pub block_hash: UInt256,
    /// Containing block height.
    pub block_height: u32,
    /// Transaction position inside the block.
    pub transaction_index: u32,
}

/// Smart-contract notification index entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationIndexRecord {
    /// Containing block hash.
    pub block_hash: UInt256,
    /// Containing block height.
    pub block_height: u32,
    /// Transaction hash that emitted the notification, if the notification came
    /// from a transaction execution.
    pub tx_hash: Option<UInt256>,
    /// Execution position inside the block's `ApplicationExecuted` list.
    pub execution_index: u32,
    /// Notification position inside the execution's notification list.
    pub notification_index: u32,
    /// Contract script hash that emitted the notification.
    pub contract_hash: UInt160,
    /// Notification event name.
    pub event_name: String,
    /// Execution trigger name (`Application`, `OnPersist`, `PostPersist`, ...).
    pub trigger: String,
    /// Number of stack items carried by the notification state payload.
    pub state_item_count: u32,
    /// Notification state payload rendered as Neo JSON-RPC stack item envelopes.
    #[serde(default)]
    pub state: Vec<Value>,
    /// Transfer participant accounts extracted from the notification payload.
    ///
    /// Populated for NEP-style `Transfer` events whose first two state items
    /// are `from`/`to` account byte strings or `null`.
    #[serde(default)]
    pub accounts: Vec<UInt160>,
}

/// Summary of the current indexer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexerStatus {
    /// Highest indexed block height, if any block has been indexed.
    pub indexed_height: Option<u32>,
    /// Hash of the highest indexed block, if any block has been indexed.
    pub indexed_hash: Option<UInt256>,
    /// Number of indexed canonical blocks.
    pub indexed_blocks: usize,
    /// Number of indexed transactions.
    pub indexed_transactions: usize,
    /// Number of accounts with at least one indexed transaction.
    pub indexed_accounts: usize,
    /// Number of indexed smart-contract notifications.
    pub indexed_notifications: usize,
    /// Number of accounts with at least one indexed notification.
    pub indexed_notification_accounts: usize,
}

impl IndexerStatus {
    /// Returns the number of ledger blocks not yet covered by the indexer.
    ///
    /// If the indexer is ahead of the supplied ledger height, the lag is
    /// reported as zero; callers should use [`Self::is_synced_with`] to
    /// distinguish an exact match from an ahead-of-ledger store mismatch.
    pub fn blocks_behind(&self, ledger_height: Option<u32>) -> Option<u32> {
        ledger_height.map(|height| match self.indexed_height {
            Some(indexed) => height.saturating_sub(indexed),
            None => height.saturating_add(1),
        })
    }

    /// Returns true only when the indexed tip exactly matches the ledger tip.
    pub fn is_synced_with(&self, ledger_height: Option<u32>) -> bool {
        match (ledger_height, self.indexed_height) {
            (Some(ledger), Some(indexed)) => indexed == ledger,
            _ => false,
        }
    }
}

/// Portable point-in-time indexer snapshot.
///
/// The snapshot persists canonical block, transaction, and notification
/// records. Derived lookup tables, such as account-to-transaction records, are
/// rebuilt on load so the on-disk format stays compact and easy to validate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexerSnapshot {
    /// Snapshot schema version.
    pub version: u32,
    /// Canonical blocks in ascending height order.
    pub blocks: Vec<BlockIndexRecord>,
    /// Canonical transactions in ascending `(block_height, transaction_index)`
    /// order.
    pub transactions: Vec<TransactionIndexRecord>,
    /// Contract notifications in ascending `(block_height, execution_index,
    /// notification_index)` order.
    #[serde(default)]
    pub notifications: Vec<NotificationIndexRecord>,
}

impl IndexerSnapshot {
    /// Creates a versioned snapshot from block and transaction records.
    pub fn new(blocks: Vec<BlockIndexRecord>, transactions: Vec<TransactionIndexRecord>) -> Self {
        Self::with_notifications(blocks, transactions, Vec::new())
    }

    /// Creates a versioned snapshot from block, transaction, and notification
    /// records.
    pub fn with_notifications(
        blocks: Vec<BlockIndexRecord>,
        transactions: Vec<TransactionIndexRecord>,
        notifications: Vec<NotificationIndexRecord>,
    ) -> Self {
        Self {
            version: INDEXER_SNAPSHOT_VERSION,
            blocks,
            transactions,
            notifications,
        }
    }
}

#[cfg(test)]
#[path = "../tests/schema/model.rs"]
mod tests;
