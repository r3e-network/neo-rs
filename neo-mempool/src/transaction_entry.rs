//! Transaction entry in the mempool

use neo_primitives::{UInt160, UInt256};
use std::time::Instant;

/// A transaction entry in the mempool
#[derive(Debug, Clone)]
pub struct TransactionEntry {
    /// Transaction hash
    pub hash: UInt256,

    /// Sender account
    pub sender: UInt160,

    /// System fee (in datoshi)
    pub system_fee: i64,

    /// Network fee (in datoshi)
    pub network_fee: i64,

    /// Transaction size in bytes
    pub size: usize,

    /// Block height when transaction expires
    pub valid_until_block: u32,

    /// Priority score (higher = more priority)
    pub priority: i64,

    /// When the transaction was added to the pool
    pub added_at: Instant,

    /// Number of times this transaction was included in a block proposal
    pub proposal_count: u32,

    /// Raw serialized transaction data
    pub data: Vec<u8>,
}

/// Parameters for creating a new transaction entry
pub struct TransactionEntryParams {
    pub hash: UInt256,
    pub sender: UInt160,
    pub system_fee: i64,
    pub network_fee: i64,
    pub size: usize,
    pub valid_until_block: u32,
    pub priority: i64,
    pub data: Vec<u8>,
}

impl TransactionEntry {
    /// Create a new transaction entry
    #[must_use] 
    pub fn new(params: TransactionEntryParams) -> Self {
        Self {
            hash: params.hash,
            sender: params.sender,
            system_fee: params.system_fee,
            network_fee: params.network_fee,
            size: params.size,
            valid_until_block: params.valid_until_block,
            priority: params.priority,
            added_at: Instant::now(),
            proposal_count: 0,
            data: params.data,
        }
    }

    /// Get total fee (system + network)
    #[must_use] 
    pub const fn total_fee(&self) -> i64 {
        self.system_fee.saturating_add(self.network_fee)
    }

    /// Get fee per byte
    #[must_use] 
    pub const fn fee_per_byte(&self) -> i64 {
        if self.size == 0 {
            0
        } else {
            self.network_fee / self.size as i64
        }
    }

    /// Check if transaction is expired at given block height
    #[must_use] 
    pub const fn is_expired(&self, current_height: u32) -> bool {
        self.valid_until_block <= current_height
    }

    /// Get age in seconds
    #[must_use] 
    pub fn age_secs(&self) -> u64 {
        self.added_at.elapsed().as_secs()
    }

    /// Increment proposal count
    pub fn increment_proposal(&mut self) {
        self.proposal_count = self.proposal_count.saturating_add(1);
    }
}

impl PartialEq for TransactionEntry {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for TransactionEntry {}

impl std::hash::Hash for TransactionEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl PartialOrd for TransactionEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TransactionEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first
        other
            .priority
            .cmp(&self.priority)
            // Then by fee per byte
            .then_with(|| other.fee_per_byte().cmp(&self.fee_per_byte()))
            // Then by age (older first)
            .then_with(|| other.added_at.cmp(&self.added_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_entry_ordering() {
        let high_priority = TransactionEntry::new(TransactionEntryParams {
            hash: UInt256::zero(),
            sender: UInt160::zero(),
            system_fee: 1_000_000,
            network_fee: 10_000_000,
            size: 100,
            valid_until_block: 1000,
            priority: 100,
            data: vec![],
        });

        let low_priority = TransactionEntry::new(TransactionEntryParams {
            hash: UInt256::from([1u8; 32]),
            sender: UInt160::zero(),
            system_fee: 100_000,
            network_fee: 1_000_000,
            size: 100,
            valid_until_block: 1000,
            priority: 10,
            data: vec![],
        });

        assert!(high_priority < low_priority); // high_priority comes first in ordering
    }

    #[test]
    fn test_expiration() {
        let entry = TransactionEntry::new(TransactionEntryParams {
            hash: UInt256::zero(),
            sender: UInt160::zero(),
            system_fee: 1_000_000,
            network_fee: 10_000_000,
            size: 100,
            valid_until_block: 1000,
            priority: 100,
            data: vec![],
        });

        assert!(!entry.is_expired(999));
        assert!(entry.is_expired(1000));
        assert!(entry.is_expired(1001));
    }
}
