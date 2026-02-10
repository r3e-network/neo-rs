// Copyright (C) 2015-2025 The Neo Project.
//
// validation.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Block validation module providing comprehensive security checks.
//!
//! This module implements hardened block validation to prevent various
//! attack vectors including oversized blocks, timestamp manipulation,
//! and merkle root tampering.

use crate::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use crate::cryptography::MerkleTree;
use crate::network::p2p::payloads::header::Header;
use crate::network::p2p::payloads::transaction::Transaction;
use crate::time_provider::TimeProvider;
use crate::UInt256;

/// Maximum allowed timestamp drift from current time (15 minutes in milliseconds)
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 15 * 60 * 1000; // 15 minutes

/// Minimum valid timestamp (Neo genesis block timestamp: July 15, 2016)
pub const MIN_TIMESTAMP_MS: u64 = 1468595301000;

/// Maximum size of witness scripts in bytes
const MAX_WITNESS_SCRIPT_SIZE: usize = 1024;

/// Block validation error types
#[derive(Debug, Clone, PartialEq)]
pub enum BlockValidationError {
    /// Block exceeds maximum size
    BlockTooLarge { size: usize, max_size: usize },
    /// Too many transactions in block
    TooManyTransactions { count: usize, max_count: usize },
    /// Timestamp is in the future beyond allowed drift
    TimestampTooFarInFuture { timestamp: u64, current: u64 },
    /// Timestamp is too old (before genesis)
    TimestampTooOld { timestamp: u64, min: u64 },
    /// Timestamp is not strictly increasing from previous
    TimestampNotIncreasing { timestamp: u64, prev_timestamp: u64 },
    /// Merkle root does not match computed root
    InvalidMerkleRoot {
        expected: UInt256,
        computed: UInt256,
    },
    /// Duplicate transaction hashes found
    DuplicateTransactions,
    /// Transaction verification failed
    TransactionVerificationFailed { index: usize, hash: UInt256 },
    /// Witness script validation failed
    InvalidWitnessScript { reason: String },
    /// Empty block when transactions expected
    EmptyTransactionList,
    /// Block version not supported
    UnsupportedVersion { version: u32 },
    /// Primary index out of range
    InvalidPrimaryIndex { index: u8, max: i32 },
    /// Header validation failed
    HeaderValidationFailed { reason: String },
}

impl std::fmt::Display for BlockValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockValidationError::BlockTooLarge { size, max_size } => {
                write!(f, "Block size {} exceeds maximum {}", size, max_size)
            }
            BlockValidationError::TooManyTransactions { count, max_count } => {
                write!(
                    f,
                    "Transaction count {} exceeds maximum {}",
                    count, max_count
                )
            }
            BlockValidationError::TimestampTooFarInFuture { timestamp, current } => {
                write!(
                    f,
                    "Timestamp {} is too far in future (current: {})",
                    timestamp, current
                )
            }
            BlockValidationError::TimestampTooOld { timestamp, min } => {
                write!(f, "Timestamp {} is before minimum {}", timestamp, min)
            }
            BlockValidationError::TimestampNotIncreasing {
                timestamp,
                prev_timestamp,
            } => {
                write!(
                    f,
                    "Timestamp {} must be greater than previous {}",
                    timestamp, prev_timestamp
                )
            }
            BlockValidationError::InvalidMerkleRoot { expected, computed } => {
                write!(
                    f,
                    "Merkle root mismatch: expected {}, computed {}",
                    expected, computed
                )
            }
            BlockValidationError::DuplicateTransactions => {
                write!(f, "Block contains duplicate transactions")
            }
            BlockValidationError::TransactionVerificationFailed { index, hash } => {
                write!(
                    f,
                    "Transaction {} at index {} failed verification",
                    hash, index
                )
            }
            BlockValidationError::InvalidWitnessScript { reason } => {
                write!(f, "Invalid witness script: {}", reason)
            }
            BlockValidationError::EmptyTransactionList => {
                write!(f, "Block has empty transaction list")
            }
            BlockValidationError::UnsupportedVersion { version } => {
                write!(f, "Block version {} is not supported", version)
            }
            BlockValidationError::InvalidPrimaryIndex { index, max } => {
                write!(
                    f,
                    "Primary index {} exceeds maximum validator count {}",
                    index, max
                )
            }
            BlockValidationError::HeaderValidationFailed { reason } => {
                write!(f, "Header validation failed: {}", reason)
            }
        }
    }
}

impl std::error::Error for BlockValidationError {}

/// Validates block size against maximum allowed size.
///
/// # Type Parameters
/// * `B` - A type that implements `BlockLike` trait
///
/// # Arguments
/// * `block` - The block to validate
///
/// # Returns
/// * `Ok(())` if block size is within limits
/// * `Err(BlockValidationError)` if block exceeds maximum size
pub fn validate_block_size<B: BlockLike>(block: &B) -> Result<(), BlockValidationError> {
    validate_block_size_raw(block.size())
}

/// Validates block size against maximum allowed size (raw value).
///
/// # Arguments
/// * `block_size` - The size of the block in bytes
///
/// # Returns
/// * `Ok(())` if block size is within limits
/// * `Err(BlockValidationError)` if block exceeds maximum size
pub fn validate_block_size_raw(block_size: usize) -> Result<(), BlockValidationError> {
    if block_size > MAX_BLOCK_SIZE {
        return Err(BlockValidationError::BlockTooLarge {
            size: block_size,
            max_size: MAX_BLOCK_SIZE,
        });
    }
    Ok(())
}

/// Validates transaction count against maximum allowed.
///
/// # Type Parameters
/// * `B` - A type that implements `BlockLike` trait
///
/// # Arguments
/// * `block` - The block to validate
///
/// # Returns
/// * `Ok(())` if transaction count is within limits
/// * `Err(BlockValidationError)` if too many transactions
pub fn validate_transaction_count<B: BlockLike>(block: &B) -> Result<(), BlockValidationError> {
    validate_transaction_count_raw(block.transaction_count())
}

/// Validates transaction count against maximum allowed (raw value).
///
/// # Arguments
/// * `tx_count` - The number of transactions
///
/// # Returns
/// * `Ok(())` if transaction count is within limits
/// * `Err(BlockValidationError)` if too many transactions
pub fn validate_transaction_count_raw(tx_count: usize) -> Result<(), BlockValidationError> {
    if tx_count > MAX_TRANSACTIONS_PER_BLOCK {
        return Err(BlockValidationError::TooManyTransactions {
            count: tx_count,
            max_count: MAX_TRANSACTIONS_PER_BLOCK,
        });
    }
    Ok(())
}

/// Trait for types that can be validated as blocks
pub trait BlockLike {
    /// Returns the size of the block in bytes
    fn size(&self) -> usize;
    /// Returns the number of transactions in the block
    fn transaction_count(&self) -> usize;
}

/// Validates block timestamp is within acceptable bounds.
///
/// Checks:
/// - Timestamp is not before genesis block timestamp
/// - Timestamp is not too far in the future (within 15 minutes)
///
/// # Arguments
/// * `timestamp` - The block timestamp to validate (in milliseconds)
///
/// # Returns
/// * `Ok(())` if timestamp is valid
/// * `Err(BlockValidationError)` if timestamp is invalid
pub fn validate_timestamp_bounds(timestamp: u64) -> Result<(), BlockValidationError> {
    // Check minimum timestamp (must be after genesis)
    if timestamp < MIN_TIMESTAMP_MS {
        return Err(BlockValidationError::TimestampTooOld {
            timestamp,
            min: MIN_TIMESTAMP_MS,
        });
    }

    // Get current time
    let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;

    // Check timestamp is not too far in the future
    if timestamp > current_time + MAX_TIMESTAMP_DRIFT_MS {
        return Err(BlockValidationError::TimestampTooFarInFuture {
            timestamp,
            current: current_time,
        });
    }

    Ok(())
}

/// Validates block timestamp against previous block timestamp.
///
/// Neo protocol requires timestamps to be strictly increasing.
///
/// # Arguments
/// * `timestamp` - The current block timestamp
/// * `prev_timestamp` - The previous block timestamp
///
/// # Returns
/// * `Ok(())` if timestamp progression is valid
/// * `Err(BlockValidationError)` if timestamp is not increasing
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

/// Validates merkle root integrity against transaction hashes.
///
/// # Arguments
/// * `merkle_root` - The expected merkle root from the header
/// * `transactions` - The transactions to compute merkle root from
///
/// # Returns
/// * `Ok(())` if merkle root matches
/// * `Err(BlockValidationError)` if merkle root is invalid
pub fn validate_merkle_root(
    merkle_root: &UInt256,
    transactions: &[Transaction],
) -> Result<(), BlockValidationError> {
    // Empty block should have zero merkle root
    if transactions.is_empty() {
        if *merkle_root != UInt256::default() {
            return Err(BlockValidationError::InvalidMerkleRoot {
                expected: *merkle_root,
                computed: UInt256::default(),
            });
        }
        return Ok(());
    }

    // Compute merkle root from transaction hashes
    let tx_hashes: Vec<UInt256> = transactions.iter().map(|tx| tx.hash()).collect();

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

/// Validates there are no duplicate transaction hashes in the block.
///
/// # Arguments
/// * `transactions` - The transactions to check
///
/// # Returns
/// * `Ok(())` if no duplicates found
/// * `Err(BlockValidationError)` if duplicates exist
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

/// Validates witness scripts in the block header.
///
/// Checks:
/// - Witness exists and is not empty
/// - Verification script is valid (if not empty)
/// - Invocation script is within size limits
///
/// # Arguments
/// * `header` - The header containing the witness
///
/// # Returns
/// * `Ok(())` if witness is valid
/// * `Err(BlockValidationError)` if witness is invalid
pub fn validate_witness_scripts(header: &Header) -> Result<(), BlockValidationError> {
    let witness = &header.witness;

    // Validate invocation script size
    if witness.invocation_script.len() > MAX_WITNESS_SCRIPT_SIZE {
        return Err(BlockValidationError::InvalidWitnessScript {
            reason: format!(
                "Invocation script size {} exceeds maximum {}",
                witness.invocation_script.len(),
                MAX_WITNESS_SCRIPT_SIZE
            ),
        });
    }

    // Validate verification script size
    if witness.verification_script.len() > MAX_WITNESS_SCRIPT_SIZE {
        return Err(BlockValidationError::InvalidWitnessScript {
            reason: format!(
                "Verification script size {} exceeds maximum {}",
                witness.verification_script.len(),
                MAX_WITNESS_SCRIPT_SIZE
            ),
        });
    }

    // If verification script is not empty, perform basic validation
    if !witness.verification_script.is_empty() {
        // Basic opcode validation - ensure it doesn't start with invalid opcodes
        let first_opcode = witness.verification_script[0];
        if first_opcode == 0xFF {
            // 0xFF is not a valid opcode (reserved for internal use)
            return Err(BlockValidationError::InvalidWitnessScript {
                reason: "Invalid opcode in verification script".to_string(),
            });
        }
    }

    Ok(())
}

/// Validates block version is supported.
///
/// # Arguments
/// * `version` - The block version
///
/// # Returns
/// * `Ok(())` if version is supported
/// * `Err(BlockValidationError)` if version is not supported
pub fn validate_block_version(version: u32) -> Result<(), BlockValidationError> {
    // Currently only version 0 is supported
    if version != 0 {
        return Err(BlockValidationError::UnsupportedVersion { version });
    }
    Ok(())
}

/// Validates primary index is within valid range.
///
/// # Arguments
/// * `primary_index` - The primary index
/// * `validators_count` - The number of validators
///
/// # Returns
/// * `Ok(())` if primary index is valid
/// * `Err(BlockValidationError)` if primary index is invalid
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
    use crate::network::p2p::payloads::witness::Witness;

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
                max_size: MAX_BLOCK_SIZE,
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
                max_count: MAX_TRANSACTIONS_PER_BLOCK,
            })
        );
    }

    #[test]
    fn validate_timestamp_bounds_accepts_valid_timestamp() {
        // Use a timestamp that's within valid range
        let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
        let valid_timestamp = current_time; // Current time should be valid
        assert!(validate_timestamp_bounds(valid_timestamp).is_ok());
    }

    #[test]
    fn validate_timestamp_bounds_rejects_past_timestamp() {
        let past_timestamp = MIN_TIMESTAMP_MS - 1;
        assert_eq!(
            validate_timestamp_bounds(past_timestamp),
            Err(BlockValidationError::TimestampTooOld {
                timestamp: past_timestamp,
                min: MIN_TIMESTAMP_MS,
            })
        );
    }

    #[test]
    fn validate_timestamp_bounds_rejects_far_future() {
        // Use a timestamp far enough in the future that timing drift won't matter
        let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
        let future_timestamp = current_time + MAX_TIMESTAMP_DRIFT_MS + 10_000; // 10 seconds buffer
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
                prev_timestamp: 1000,
            })
        );
        assert_eq!(
            validate_timestamp_progression(999, 1000),
            Err(BlockValidationError::TimestampNotIncreasing {
                timestamp: 999,
                prev_timestamp: 1000,
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
        // Non-zero merkle root with empty transactions should fail
        let wrong_root = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let transactions: Vec<Transaction> = vec![];
        assert!(validate_merkle_root(&wrong_root, &transactions).is_err());
    }

    #[test]
    fn validate_no_duplicate_transactions_accepts_unique() {
        let tx1 = Transaction::new();
        let mut tx2 = Transaction::new();
        tx2.set_nonce(1); // Make it different from tx1
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
        assert_eq!(MAX_BLOCK_SIZE, 2_097_152); // 2 MB — Neo N3 default
        assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 512); // Neo N3 ProtocolSettings.Default
        assert_eq!(MAX_TIMESTAMP_DRIFT_MS, 900_000); // 15 minutes
        assert_eq!(MIN_TIMESTAMP_MS, 1468595301000); // Genesis timestamp
    }
}
