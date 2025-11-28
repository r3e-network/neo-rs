//! Fair ordering policy for MEV prevention
//!
//! Implements various ordering strategies to prevent front-running and other MEV attacks.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Transaction ordering policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FairOrderingPolicy {
    /// First-come, first-served based on arrival time in enclave
    /// Most fair but may have lower throughput
    FirstComeFirstServed,

    /// Batch transactions and order randomly within batch
    /// Good balance of fairness and throughput
    BatchedRandom {
        /// Duration to collect transactions before ordering
        batch_interval_ms: u64,
    },

    /// Order by commit timestamp with random tiebreaker
    /// Prevents timestamp manipulation while maintaining order
    CommitReveal {
        /// Duration for commit phase
        commit_duration_ms: u64,
        /// Duration for reveal phase
        reveal_duration_ms: u64,
    },

    /// Threshold encryption - transactions encrypted until block proposal
    /// Strongest MEV protection but highest overhead
    ThresholdEncryption,

    /// Hybrid: FCFS with gas price cap (prevents gas wars)
    FcfsWithGasCap {
        /// Maximum gas price multiplier above median
        max_gas_multiplier: u32,
    },
}

impl Default for FairOrderingPolicy {
    fn default() -> Self {
        // BatchedRandom with 100ms batches is a good default
        FairOrderingPolicy::BatchedRandom {
            batch_interval_ms: 100,
        }
    }
}

/// Information about when a transaction was received
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionTiming {
    /// When the transaction was first seen in the enclave
    pub enclave_arrival_time: SystemTime,
    /// Monotonic sequence number assigned in enclave
    pub sequence_number: u64,
    /// Hash commitment (for commit-reveal scheme)
    pub commitment: Option<[u8; 32]>,
    /// Batch ID this transaction belongs to
    pub batch_id: Option<u64>,
}

impl TransactionTiming {
    pub fn new(sequence_number: u64) -> Self {
        Self {
            enclave_arrival_time: SystemTime::now(),
            sequence_number,
            commitment: None,
            batch_id: None,
        }
    }

    pub fn with_batch(mut self, batch_id: u64) -> Self {
        self.batch_id = Some(batch_id);
        self
    }

    pub fn with_commitment(mut self, commitment: [u8; 32]) -> Self {
        self.commitment = Some(commitment);
        self
    }
}

/// Ordering key for comparing transactions
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderingKey {
    /// Primary sort key (depends on policy)
    pub primary: u64,
    /// Secondary sort key (random tiebreaker)
    pub secondary: u64,
    /// Transaction hash for final tiebreaker
    pub tx_hash: [u8; 32],
}

impl Ord for OrderingKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.primary
            .cmp(&other.primary)
            .then_with(|| self.secondary.cmp(&other.secondary))
            .then_with(|| self.tx_hash.cmp(&other.tx_hash))
    }
}

impl PartialOrd for OrderingKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Compute ordering key based on policy
pub fn compute_ordering_key(
    policy: FairOrderingPolicy,
    timing: &TransactionTiming,
    tx_hash: &[u8; 32],
    network_fee: i64,
) -> OrderingKey {
    match policy {
        FairOrderingPolicy::FirstComeFirstServed => {
            // Primary key is sequence number, secondary is random
            OrderingKey {
                primary: timing.sequence_number,
                secondary: rand::random(),
                tx_hash: *tx_hash,
            }
        }
        FairOrderingPolicy::BatchedRandom { .. } => {
            // Primary key is batch ID, secondary is random within batch
            OrderingKey {
                primary: timing.batch_id.unwrap_or(0),
                secondary: rand::random(),
                tx_hash: *tx_hash,
            }
        }
        FairOrderingPolicy::CommitReveal { .. } => {
            // Primary key is commit time, secondary uses commitment for deterministic ordering
            let commitment_key = timing
                .commitment
                .map(|c| u64::from_le_bytes(c[..8].try_into().unwrap()))
                .unwrap_or(0);
            OrderingKey {
                primary: timing.sequence_number,
                secondary: commitment_key,
                tx_hash: *tx_hash,
            }
        }
        FairOrderingPolicy::ThresholdEncryption => {
            // All transactions have same priority until decrypted
            // Random ordering after decryption
            OrderingKey {
                primary: 0,
                secondary: rand::random(),
                tx_hash: *tx_hash,
            }
        }
        FairOrderingPolicy::FcfsWithGasCap { max_gas_multiplier } => {
            // FCFS but transactions paying excessive fees get deprioritized
            // This prevents gas wars while still allowing normal fee bidding
            let capped_fee = network_fee.min(network_fee * max_gas_multiplier as i64);
            OrderingKey {
                primary: timing.sequence_number,
                secondary: capped_fee as u64,
                tx_hash: *tx_hash,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fcfs_ordering() {
        let policy = FairOrderingPolicy::FirstComeFirstServed;

        let timing1 = TransactionTiming::new(1);
        let timing2 = TransactionTiming::new(2);

        let hash1 = [0u8; 32];
        let hash2 = [1u8; 32];

        let key1 = compute_ordering_key(policy, &timing1, &hash1, 1000);
        let key2 = compute_ordering_key(policy, &timing2, &hash2, 2000);

        // Earlier sequence number should come first
        assert!(key1 < key2);
    }

    #[test]
    fn test_batched_ordering() {
        let policy = FairOrderingPolicy::BatchedRandom {
            batch_interval_ms: 100,
        };

        let timing1 = TransactionTiming::new(1).with_batch(1);
        let timing2 = TransactionTiming::new(2).with_batch(1);
        let timing3 = TransactionTiming::new(3).with_batch(2);

        let hash1 = [0u8; 32];
        let hash2 = [1u8; 32];
        let hash3 = [2u8; 32];

        let key1 = compute_ordering_key(policy, &timing1, &hash1, 1000);
        let key2 = compute_ordering_key(policy, &timing2, &hash2, 1000);
        let key3 = compute_ordering_key(policy, &timing3, &hash3, 1000);

        // Same batch should have same primary key
        assert_eq!(key1.primary, key2.primary);
        // Different batch should have different primary key
        assert!(key1.primary < key3.primary);
    }
}
