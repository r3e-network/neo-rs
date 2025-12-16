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

impl TransactionEntry {
    /// Create a new transaction entry
    pub fn new(
        hash: UInt256,
        sender: UInt160,
        system_fee: i64,
        network_fee: i64,
        size: usize,
        valid_until_block: u32,
        priority: i64,
        data: Vec<u8>,
    ) -> Self {
        Self {
            hash,
            sender,
            system_fee,
            network_fee,
            size,
            valid_until_block,
            priority,
            added_at: Instant::now(),
            proposal_count: 0,
            data,
        }
    }

    /// Get total fee (system + network)
    pub fn total_fee(&self) -> i64 {
        self.system_fee.saturating_add(self.network_fee)
    }

    /// Get fee per byte
    pub fn fee_per_byte(&self) -> i64 {
        if self.size == 0 {
            0
        } else {
            self.network_fee / self.size as i64
        }
    }

    /// Check if transaction is expired at given block height
    pub fn is_expired(&self, current_height: u32) -> bool {
        self.valid_until_block <= current_height
    }

    /// Get age in seconds
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
        let high_priority = TransactionEntry::new(
            UInt256::zero(),
            UInt160::zero(),
            1_000_000,
            10_000_000,
            100,
            1000,
            100,
            vec![],
        );

        let low_priority = TransactionEntry::new(
            UInt256::from([1u8; 32]),
            UInt160::zero(),
            100_000,
            1_000_000,
            100,
            1000,
            10,
            vec![],
        );

        assert!(high_priority < low_priority); // high_priority comes first in ordering
    }

    #[test]
    fn test_expiration() {
        let entry = TransactionEntry::new(
            UInt256::zero(),
            UInt160::zero(),
            1_000_000,
            10_000_000,
            100,
            1000,
            100,
            vec![],
        );

        assert!(!entry.is_expired(999));
        assert!(entry.is_expired(1000));
        assert!(entry.is_expired(1001));
    }
}
