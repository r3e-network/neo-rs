use neo_crypto::MerkleTree;
use neo_primitives::UInt256;
use neo_primitives::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use neo_primitives::time_provider::TimeProvider;
use neo_protocol::payloads::{Header, Transaction};
use thiserror::Error;

pub use neo_primitives::BlockLike;
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 15 * 60 * 1000;
pub const MIN_TIMESTAMP_MS: u64 = 1468595301000;
const MAX_WITNESS_SCRIPT_SIZE: usize = 1024;

#[derive(Debug, Clone, Error, PartialEq)]
pub enum BlockValidationError {
    #[error("Block size {size} exceeds maximum {max_size}")]
    BlockTooLarge { size: usize, max_size: usize },
    #[error("Transaction count {count} exceeds maximum {max_count}")]
    TooManyTransactions { count: usize, max_count: usize },
    #[error("Timestamp {timestamp} is too far in future (current: {current})")]
    TimestampTooFarInFuture { timestamp: u64, current: u64 },
    #[error("Timestamp {timestamp} is before minimum {min}")]
    TimestampTooOld { timestamp: u64, min: u64 },
    #[error("Timestamp {timestamp} must be greater than previous {prev_timestamp}")]
    TimestampNotIncreasing { timestamp: u64, prev_timestamp: u64 },
    #[error("Merkle root mismatch: expected {expected}, computed {computed}")]
    InvalidMerkleRoot {
        expected: UInt256,
        computed: UInt256,
    },
    #[error("Block contains duplicate transactions")]
    DuplicateTransactions,
    #[error("Transaction {hash} at index {index} failed verification")]
    TransactionVerificationFailed { index: usize, hash: UInt256 },
    #[error("Invalid witness script: {reason}")]
    InvalidWitnessScript { reason: String },
    #[error("Block has empty transaction list")]
    EmptyTransactionList,
    #[error("Block version {version} is not supported")]
    UnsupportedVersion { version: u32 },
    #[error("Primary index {index} exceeds maximum validator count {max}")]
    InvalidPrimaryIndex { index: u8, max: i32 },
    #[error("Header validation failed: {reason}")]
    HeaderValidationFailed { reason: String },
}

pub fn validate_block_size<B: BlockLike>(block: &B) -> Result<(), BlockValidationError> {
    validate_block_size_raw(block.size())
}

pub fn validate_block_size_raw(block_size: usize) -> Result<(), BlockValidationError> {
    if block_size > MAX_BLOCK_SIZE {
        return Err(BlockValidationError::BlockTooLarge {
            size: block_size,
            max_size: MAX_BLOCK_SIZE,
        });
    }
    Ok(())
}

pub fn validate_transaction_count<B: BlockLike>(block: &B) -> Result<(), BlockValidationError> {
    validate_transaction_count_raw(block.transaction_count())
}

pub fn validate_transaction_count_raw(tx_count: usize) -> Result<(), BlockValidationError> {
    if tx_count > MAX_TRANSACTIONS_PER_BLOCK {
        return Err(BlockValidationError::TooManyTransactions {
            count: tx_count,
            max_count: MAX_TRANSACTIONS_PER_BLOCK,
        });
    }
    Ok(())
}

pub fn validate_timestamp_bounds(timestamp: u64) -> Result<(), BlockValidationError> {
    if timestamp < MIN_TIMESTAMP_MS {
        return Err(BlockValidationError::TimestampTooOld {
            timestamp,
            min: MIN_TIMESTAMP_MS,
        });
    }
    let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
    if timestamp > current_time + MAX_TIMESTAMP_DRIFT_MS {
        return Err(BlockValidationError::TimestampTooFarInFuture {
            timestamp,
            current: current_time,
        });
    }
    Ok(())
}

pub fn validate_timestamp_progression(
    timestamp: u64,
    prev_timestamp: u64,
) -> Result<(), BlockValidationError> {
    if timestamp <= prev_timestamp {
        return Err(BlockValidationError::TimestampNotIncreasing {
            timestamp,
            prev_timestamp,
        });
    }
    Ok(())
}

pub fn validate_merkle_root(
    merkle_root: &UInt256,
    transactions: &[Transaction],
) -> Result<(), BlockValidationError> {
    if transactions.is_empty() {
        if *merkle_root != UInt256::default() {
            return Err(BlockValidationError::InvalidMerkleRoot {
                expected: *merkle_root,
                computed: UInt256::default(),
            });
        }
        return Ok(());
    }
    let tx_hashes: Vec<UInt256> = transactions
        .iter()
        .map(neo_protocol::payloads::Transaction::hash)
        .collect();
    match MerkleTree::compute_root(&tx_hashes) {
        Some(computed_root) => {
            if computed_root != *merkle_root {
                return Err(BlockValidationError::InvalidMerkleRoot {
                    expected: *merkle_root,
                    computed: computed_root,
                });
            }
            Ok(())
        }
        None => Err(BlockValidationError::InvalidMerkleRoot {
            expected: *merkle_root,
            computed: UInt256::default(),
        }),
    }
}

pub fn validate_no_duplicate_transactions(
    transactions: &[Transaction],
) -> Result<(), BlockValidationError> {
    let mut seen = std::collections::HashSet::with_capacity(transactions.len());
    for tx in transactions {
        if !seen.insert(tx.hash()) {
            return Err(BlockValidationError::DuplicateTransactions);
        }
    }
    Ok(())
}

pub fn validate_witness_scripts(header: &Header) -> Result<(), BlockValidationError> {
    let witness = &header.witness;
    if witness.invocation_script.len() > MAX_WITNESS_SCRIPT_SIZE {
        return Err(BlockValidationError::InvalidWitnessScript {
            reason: format!(
                "Invocation script size {} exceeds maximum {}",
                witness.invocation_script.len(),
                MAX_WITNESS_SCRIPT_SIZE
            ),
        });
    }
    if witness.verification_script.len() > MAX_WITNESS_SCRIPT_SIZE {
        return Err(BlockValidationError::InvalidWitnessScript {
            reason: format!(
                "Verification script size {} exceeds maximum {}",
                witness.verification_script.len(),
                MAX_WITNESS_SCRIPT_SIZE
            ),
        });
    }
    if !witness.verification_script.is_empty() {
        let first_opcode = witness.verification_script[0];
        if first_opcode == 0xFF {
            return Err(BlockValidationError::InvalidWitnessScript {
                reason: "Invalid opcode in verification script".to_string(),
            });
        }
    }
    Ok(())
}

pub fn validate_block_version(version: u32) -> Result<(), BlockValidationError> {
    if version != 0 {
        return Err(BlockValidationError::UnsupportedVersion { version });
    }
    Ok(())
}

pub fn validate_primary_index(
    primary_index: u8,
    validators_count: i32,
) -> Result<(), BlockValidationError> {
    if primary_index as i32 >= validators_count {
        return Err(BlockValidationError::InvalidPrimaryIndex {
            index: primary_index,
            max: validators_count,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_protocol::witness::Witness;

    #[test]
    fn block_validation_error_display_messages_remain_stable() {
        let hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        assert_eq!(
            BlockValidationError::BlockTooLarge {
                size: 11,
                max_size: 10
            }
            .to_string(),
            "Block size 11 exceeds maximum 10"
        );
        assert_eq!(
            BlockValidationError::TooManyTransactions {
                count: 12,
                max_count: 11
            }
            .to_string(),
            "Transaction count 12 exceeds maximum 11"
        );
        assert_eq!(
            BlockValidationError::TimestampTooFarInFuture {
                timestamp: 20,
                current: 10
            }
            .to_string(),
            "Timestamp 20 is too far in future (current: 10)"
        );
        assert_eq!(
            BlockValidationError::TimestampTooOld {
                timestamp: 1,
                min: 2
            }
            .to_string(),
            "Timestamp 1 is before minimum 2"
        );
        assert_eq!(
            BlockValidationError::TimestampNotIncreasing {
                timestamp: 7,
                prev_timestamp: 8
            }
            .to_string(),
            "Timestamp 7 must be greater than previous 8"
        );
        assert_eq!(
            BlockValidationError::InvalidMerkleRoot {
                expected: hash,
                computed: UInt256::default()
            }
            .to_string(),
            format!(
                "Merkle root mismatch: expected {}, computed {}",
                hash,
                UInt256::default()
            )
        );
        assert_eq!(
            BlockValidationError::DuplicateTransactions.to_string(),
            "Block contains duplicate transactions"
        );
        assert_eq!(
            BlockValidationError::TransactionVerificationFailed { index: 3, hash }.to_string(),
            format!("Transaction {hash} at index 3 failed verification")
        );
        assert_eq!(
            BlockValidationError::InvalidWitnessScript {
                reason: "bad opcode".to_string()
            }
            .to_string(),
            "Invalid witness script: bad opcode"
        );
        assert_eq!(
            BlockValidationError::EmptyTransactionList.to_string(),
            "Block has empty transaction list"
        );
        assert_eq!(
            BlockValidationError::UnsupportedVersion { version: 2 }.to_string(),
            "Block version 2 is not supported"
        );
        assert_eq!(
            BlockValidationError::InvalidPrimaryIndex { index: 8, max: 7 }.to_string(),
            "Primary index 8 exceeds maximum validator count 7"
        );
        assert_eq!(
            BlockValidationError::HeaderValidationFailed {
                reason: "bad header".to_string()
            }
            .to_string(),
            "Header validation failed: bad header"
        );
    }

    #[test]
    fn validate_block_version_accepts_version_0() {
        assert!(validate_block_version(0).is_ok());
    }

    #[test]
    fn validate_block_version_rejects_unsupported_versions() {
        assert_eq!(
            validate_block_version(1),
            Err(BlockValidationError::UnsupportedVersion { version: 1 })
        );
        assert_eq!(
            validate_block_version(99),
            Err(BlockValidationError::UnsupportedVersion { version: 99 })
        );
    }

    #[test]
    fn validate_block_size_raw_accepts_valid_size() {
        assert!(validate_block_size_raw(1000).is_ok());
        assert!(validate_block_size_raw(MAX_BLOCK_SIZE).is_ok());
    }

    #[test]
    fn validate_block_size_raw_rejects_oversized() {
        assert_eq!(
            validate_block_size_raw(MAX_BLOCK_SIZE + 1),
            Err(BlockValidationError::BlockTooLarge {
                size: MAX_BLOCK_SIZE + 1,
                max_size: MAX_BLOCK_SIZE
            })
        );
    }

    #[test]
    fn validate_transaction_count_raw_accepts_valid_count() {
        assert!(validate_transaction_count_raw(100).is_ok());
        assert!(validate_transaction_count_raw(MAX_TRANSACTIONS_PER_BLOCK).is_ok());
    }

    #[test]
    fn validate_transaction_count_raw_rejects_too_many() {
        assert_eq!(
            validate_transaction_count_raw(MAX_TRANSACTIONS_PER_BLOCK + 1),
            Err(BlockValidationError::TooManyTransactions {
                count: MAX_TRANSACTIONS_PER_BLOCK + 1,
                max_count: MAX_TRANSACTIONS_PER_BLOCK
            })
        );
    }

    #[test]
    fn validate_timestamp_bounds_accepts_valid_timestamp() {
        let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
        let valid_timestamp = current_time;
        assert!(validate_timestamp_bounds(valid_timestamp).is_ok());
    }

    #[test]
    fn validate_timestamp_bounds_rejects_past_timestamp() {
        let past_timestamp = MIN_TIMESTAMP_MS - 1;
        assert_eq!(
            validate_timestamp_bounds(past_timestamp),
            Err(BlockValidationError::TimestampTooOld {
                timestamp: past_timestamp,
                min: MIN_TIMESTAMP_MS
            })
        );
    }

    #[test]
    fn validate_timestamp_bounds_rejects_far_future() {
        let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
        let future_timestamp = current_time + MAX_TIMESTAMP_DRIFT_MS + 10_000;
        let result = validate_timestamp_bounds(future_timestamp);
        assert!(matches!(
            result,
            Err(BlockValidationError::TimestampTooFarInFuture { .. })
        ));
    }

    #[test]
    fn validate_timestamp_progression_accepts_increasing() {
        assert!(validate_timestamp_progression(2000, 1000).is_ok());
        assert!(validate_timestamp_progression(1001, 1000).is_ok());
    }

    #[test]
    fn validate_timestamp_progression_rejects_non_increasing() {
        assert_eq!(
            validate_timestamp_progression(1000, 1000),
            Err(BlockValidationError::TimestampNotIncreasing {
                timestamp: 1000,
                prev_timestamp: 1000
            })
        );
        assert_eq!(
            validate_timestamp_progression(999, 1000),
            Err(BlockValidationError::TimestampNotIncreasing {
                timestamp: 999,
                prev_timestamp: 1000
            })
        );
    }

    #[test]
    fn validate_merkle_root_accepts_empty_block() {
        let merkle_root = UInt256::default();
        let transactions: Vec<Transaction> = vec![];
        assert!(validate_merkle_root(&merkle_root, &transactions).is_ok());
    }

    #[test]
    fn validate_merkle_root_rejects_wrong_root_for_empty() {
        let wrong_root = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let transactions: Vec<Transaction> = vec![];
        assert!(validate_merkle_root(&wrong_root, &transactions).is_err());
    }

    #[test]
    fn validate_no_duplicate_transactions_accepts_unique() {
        let tx1 = Transaction::new();
        let mut tx2 = Transaction::new();
        tx2.set_nonce(1);
        let transactions = vec![tx1, tx2];
        assert!(validate_no_duplicate_transactions(&transactions).is_ok());
    }

    #[test]
    fn validate_primary_index_accepts_valid() {
        assert!(validate_primary_index(0, 7).is_ok());
        assert!(validate_primary_index(6, 7).is_ok());
    }

    #[test]
    fn validate_primary_index_rejects_invalid() {
        assert_eq!(
            validate_primary_index(7, 7),
            Err(BlockValidationError::InvalidPrimaryIndex { index: 7, max: 7 })
        );
        assert_eq!(
            validate_primary_index(10, 7),
            Err(BlockValidationError::InvalidPrimaryIndex { index: 10, max: 7 })
        );
    }

    #[test]
    fn validate_witness_scripts_accepts_valid() {
        let mut header = Header::new();
        header.witness = Witness::new();
        assert!(validate_witness_scripts(&header).is_ok());
    }

    #[test]
    fn validate_witness_scripts_rejects_oversized_invocation() {
        let mut header = Header::new();
        header.witness = Witness::new_with_scripts(vec![0u8; 1025], vec![]);
        assert!(validate_witness_scripts(&header).is_err());
    }

    #[test]
    fn validate_witness_scripts_rejects_oversized_verification() {
        let mut header = Header::new();
        header.witness = Witness::new_with_scripts(vec![], vec![0u8; 1025]);
        assert!(validate_witness_scripts(&header).is_err());
    }

    #[test]
    fn max_constants_are_correct() {
        assert_eq!(MAX_BLOCK_SIZE, 2_097_152);
        assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 512);
        assert_eq!(MAX_TIMESTAMP_DRIFT_MS, 900_000);
        assert_eq!(MIN_TIMESTAMP_MS, 1468595301000);
    }
}
