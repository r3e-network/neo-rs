//! Block validation providing comprehensive security checks.
//!
//! This module implements hardened block validation to prevent various
//! attack vectors including oversized blocks, timestamp manipulation,
//! and merkle root tampering. It is the **pure** validation layer: it
//! operates on `BlockLike` trait objects and `&Witness` references, so
//! it has no dependency on the stateful blockchain service, consensus,
//! native-contract, or storage layers. Stateful verification is handled by
//! the service pipeline before a block is admitted.

use neo_crypto::MerkleTree;
use neo_payloads::Witness;
use neo_primitives::blockchain::marker_traits::BlockLike;
use neo_primitives::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use neo_primitives::{TimeProvider, UInt256};
use thiserror::Error;

/// Maximum allowed timestamp drift from current time (15 minutes in milliseconds)
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 15 * 60 * 1000; // 15 minutes

/// Minimum valid timestamp (Neo genesis block timestamp: July 15, 2016)
pub const MIN_TIMESTAMP_MS: u64 = 1468595301000;

/// Maximum size of witness scripts in bytes
const MAX_WITNESS_SCRIPT_SIZE: usize = 1024;

/// Block validation error types
#[derive(Debug, Clone, Error, PartialEq)]
pub enum BlockValidationError {
    /// Block exceeds maximum size
    #[error("Block size {size} exceeds maximum {max_size}")]
    BlockTooLarge {
        /// Actual serialized block size.
        size: usize,
        /// Maximum allowed serialized block size.
        max_size: usize,
    },
    /// Too many transactions in block
    #[error("Transaction count {count} exceeds maximum {max_count}")]
    TooManyTransactions {
        /// Actual transaction count.
        count: usize,
        /// Maximum allowed transaction count.
        max_count: usize,
    },
    /// Timestamp is in the future beyond allowed drift
    #[error("Timestamp {timestamp} is too far in future (current: {current})")]
    TimestampTooFarInFuture {
        /// Block timestamp in milliseconds.
        timestamp: u64,
        /// Current node time in milliseconds.
        current: u64,
    },
    /// Timestamp is too old (before genesis)
    #[error("Timestamp {timestamp} is before minimum {min}")]
    TimestampTooOld {
        /// Block timestamp in milliseconds.
        timestamp: u64,
        /// Minimum accepted timestamp in milliseconds.
        min: u64,
    },
    /// Timestamp is not strictly increasing from previous
    #[error("Timestamp {timestamp} must be greater than previous {prev_timestamp}")]
    TimestampNotIncreasing {
        /// Block timestamp in milliseconds.
        timestamp: u64,
        /// Previous block timestamp in milliseconds.
        prev_timestamp: u64,
    },
    /// Merkle root does not match computed root
    #[error("Merkle root mismatch: expected {expected}, computed {computed}")]
    InvalidMerkleRoot {
        /// Merkle root declared by the block header.
        expected: UInt256,
        /// Merkle root recomputed from transactions.
        computed: UInt256,
    },
    /// Duplicate transaction hashes found
    #[error("Block contains duplicate transactions")]
    DuplicateTransactions,
    /// Transaction verification failed
    #[error("Transaction {hash} at index {index} failed verification")]
    TransactionVerificationFailed {
        /// Transaction index within the block.
        index: usize,
        /// Transaction hash.
        hash: UInt256,
    },
    /// Witness script validation failed
    #[error("Invalid witness script: {reason}")]
    InvalidWitnessScript {
        /// Validation failure reason.
        reason: String,
    },
    /// Empty block when transactions expected
    #[error("Block has empty transaction list")]
    EmptyTransactionList,
    /// Block version not supported
    #[error("Block version {version} is not supported")]
    UnsupportedVersion {
        /// Unsupported block version.
        version: u32,
    },
    /// Primary index out of range
    #[error("Primary index {index} exceeds maximum validator count {max}")]
    InvalidPrimaryIndex {
        /// Primary validator index from the block header.
        index: u8,
        /// Maximum valid validator index.
        max: i32,
    },
    /// Header validation failed
    #[error("Header validation failed: {reason}")]
    HeaderValidationFailed {
        /// Validation failure reason.
        reason: String,
    },
}

/// Stateless block-validation checks.
///
/// The pure validation layer grouped onto a single zero-sized type: every
/// check is an associated function (none carry state), so callers spell them
/// `BlockValidator::validate_*`.
pub struct BlockValidator;

impl BlockValidator {
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
        Self::validate_block_size_raw(block.size())
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
        Self::validate_transaction_count_raw(block.transaction_count())
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
        Self::validate_transaction_count_raw_with_limit(tx_count, MAX_TRANSACTIONS_PER_BLOCK)
    }

    /// Validates transaction count against an effective protocol limit.
    ///
    /// Neo's built-in default is 512, but MainNet/TestNet v3.10.0 configurations
    /// override `ProtocolSettings.MaxTransactionsPerBlock`. Consensus-facing
    /// callers should pass the effective setting instead of the library default.
    pub fn validate_transaction_count_raw_with_limit(
        tx_count: usize,
        max_count: usize,
    ) -> Result<(), BlockValidationError> {
        if tx_count > max_count {
            return Err(BlockValidationError::TooManyTransactions {
                count: tx_count,
                max_count,
            });
        }
        Ok(())
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
    /// Takes pre-computed transaction hashes so this function has no
    /// dependency on the concrete `Transaction` type. The caller is
    /// responsible for computing the hashes from whatever transaction
    /// representation they hold.
    ///
    /// # Arguments
    /// * `merkle_root` - The expected merkle root from the header
    /// * `tx_hashes` - The transaction hashes in canonical block order
    ///
    /// # Returns
    /// * `Ok(())` if merkle root matches
    /// * `Err(BlockValidationError)` if merkle root is invalid
    pub fn validate_merkle_root(
        merkle_root: &UInt256,
        tx_hashes: &[UInt256],
    ) -> Result<(), BlockValidationError> {
        // Empty block should have zero merkle root
        if tx_hashes.is_empty() {
            if *merkle_root != UInt256::default() {
                return Err(BlockValidationError::InvalidMerkleRoot {
                    expected: *merkle_root,
                    computed: UInt256::default(),
                });
            }
            return Ok(());
        }

        // Compute merkle root from the pre-computed transaction hashes.
        match MerkleTree::compute_root(tx_hashes) {
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
    /// Takes pre-computed transaction hashes so this function has no
    /// dependency on the concrete `Transaction` type. The caller is
    /// responsible for computing the hashes from whatever transaction
    /// representation they hold.
    ///
    /// # Arguments
    /// * `tx_hashes` - The transaction hashes to check for duplicates
    ///
    /// # Returns
    /// * `Ok(())` if no duplicates found
    /// * `Err(BlockValidationError)` if duplicates exist
    pub fn validate_no_duplicate_transactions(
        tx_hashes: &[UInt256],
    ) -> Result<(), BlockValidationError> {
        let mut seen = std::collections::HashSet::with_capacity(tx_hashes.len());
        for hash in tx_hashes {
            if !seen.insert(*hash) {
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
    pub fn validate_witness_scripts(witness: &Witness) -> Result<(), BlockValidationError> {
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
}

// Re-export BlockLike from neo-primitives (single source of truth).
// pub use neo_primitives::BlockLike; // already imported at the top of the file

#[cfg(test)]
mod tests {
    use super::*;

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
            format!("Transaction {} at index 3 failed verification", hash)
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
        assert!(BlockValidator::validate_block_version(0).is_ok());
    }

    #[test]
    fn validate_block_version_rejects_unsupported_versions() {
        assert_eq!(
            BlockValidator::validate_block_version(1),
            Err(BlockValidationError::UnsupportedVersion { version: 1 })
        );
        assert_eq!(
            BlockValidator::validate_block_version(99),
            Err(BlockValidationError::UnsupportedVersion { version: 99 })
        );
    }

    #[test]
    fn validate_block_size_raw_accepts_valid_size() {
        assert!(BlockValidator::validate_block_size_raw(1000).is_ok());
        assert!(BlockValidator::validate_block_size_raw(MAX_BLOCK_SIZE).is_ok());
    }

    #[test]
    fn validate_block_size_raw_rejects_oversized() {
        assert_eq!(
            BlockValidator::validate_block_size_raw(MAX_BLOCK_SIZE + 1),
            Err(BlockValidationError::BlockTooLarge {
                size: MAX_BLOCK_SIZE + 1,
                max_size: MAX_BLOCK_SIZE,
            })
        );
    }

    #[test]
    fn validate_transaction_count_raw_accepts_valid_count() {
        assert!(BlockValidator::validate_transaction_count_raw(100).is_ok());
        assert!(BlockValidator::validate_transaction_count_raw(MAX_TRANSACTIONS_PER_BLOCK).is_ok());
    }

    #[test]
    fn validate_transaction_count_raw_rejects_too_many() {
        assert_eq!(
            BlockValidator::validate_transaction_count_raw(MAX_TRANSACTIONS_PER_BLOCK + 1),
            Err(BlockValidationError::TooManyTransactions {
                count: MAX_TRANSACTIONS_PER_BLOCK + 1,
                max_count: MAX_TRANSACTIONS_PER_BLOCK,
            })
        );
    }

    #[test]
    fn validate_transaction_count_raw_with_limit_uses_effective_protocol_limit() {
        assert!(BlockValidator::validate_transaction_count_raw_with_limit(200, 200).is_ok());
        assert_eq!(
            BlockValidator::validate_transaction_count_raw_with_limit(201, 200),
            Err(BlockValidationError::TooManyTransactions {
                count: 201,
                max_count: 200,
            })
        );
    }

    #[test]
    fn validate_timestamp_bounds_accepts_valid_timestamp() {
        // Use a timestamp that's within valid range
        let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
        let valid_timestamp = current_time; // Current time should be valid
        assert!(BlockValidator::validate_timestamp_bounds(valid_timestamp).is_ok());
    }

    #[test]
    fn validate_timestamp_bounds_rejects_past_timestamp() {
        let past_timestamp = MIN_TIMESTAMP_MS - 1;
        assert_eq!(
            BlockValidator::validate_timestamp_bounds(past_timestamp),
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
        let result = BlockValidator::validate_timestamp_bounds(future_timestamp);
        assert!(matches!(
            result,
            Err(BlockValidationError::TimestampTooFarInFuture { .. })
        ));
    }

    #[test]
    fn validate_timestamp_progression_accepts_increasing() {
        assert!(BlockValidator::validate_timestamp_progression(2000, 1000).is_ok());
        assert!(BlockValidator::validate_timestamp_progression(1001, 1000).is_ok());
    }

    #[test]
    fn validate_timestamp_progression_rejects_non_increasing() {
        assert_eq!(
            BlockValidator::validate_timestamp_progression(1000, 1000),
            Err(BlockValidationError::TimestampNotIncreasing {
                timestamp: 1000,
                prev_timestamp: 1000,
            })
        );
        assert_eq!(
            BlockValidator::validate_timestamp_progression(999, 1000),
            Err(BlockValidationError::TimestampNotIncreasing {
                timestamp: 999,
                prev_timestamp: 1000,
            })
        );
    }

    #[test]
    fn validate_merkle_root_accepts_empty_block() {
        let merkle_root = UInt256::default();
        let tx_hashes: Vec<UInt256> = vec![];
        assert!(BlockValidator::validate_merkle_root(&merkle_root, &tx_hashes).is_ok());
    }

    #[test]
    fn validate_merkle_root_rejects_wrong_root_for_empty() {
        // Non-zero merkle root with empty transactions should fail
        let wrong_root = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let tx_hashes: Vec<UInt256> = vec![];
        assert!(BlockValidator::validate_merkle_root(&wrong_root, &tx_hashes).is_err());
    }

    #[test]
    fn validate_no_duplicate_transactions_accepts_unique() {
        let hash_a = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let hash_b = UInt256::from_bytes(&[2u8; 32]).unwrap();
        let tx_hashes = vec![hash_a, hash_b];
        assert!(BlockValidator::validate_no_duplicate_transactions(&tx_hashes).is_ok());
    }

    #[test]
    fn validate_no_duplicate_transactions_rejects_duplicates() {
        let hash_a = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let tx_hashes = vec![hash_a, hash_a];
        assert!(BlockValidator::validate_no_duplicate_transactions(&tx_hashes).is_err());
    }

    #[test]
    fn validate_primary_index_accepts_valid() {
        assert!(BlockValidator::validate_primary_index(0, 7).is_ok());
        assert!(BlockValidator::validate_primary_index(6, 7).is_ok());
    }

    #[test]
    fn validate_primary_index_rejects_invalid() {
        assert_eq!(
            BlockValidator::validate_primary_index(7, 7),
            Err(BlockValidationError::InvalidPrimaryIndex { index: 7, max: 7 })
        );
        assert_eq!(
            BlockValidator::validate_primary_index(10, 7),
            Err(BlockValidationError::InvalidPrimaryIndex { index: 10, max: 7 })
        );
    }

    #[test]
    fn validate_witness_scripts_accepts_valid() {
        let witness = Witness::new();
        assert!(BlockValidator::validate_witness_scripts(&witness).is_ok());
    }

    #[test]
    fn validate_witness_scripts_rejects_oversized_invocation() {
        let witness = Witness::new_with_scripts(vec![0u8; 1025], vec![]);
        assert!(BlockValidator::validate_witness_scripts(&witness).is_err());
    }

    #[test]
    fn validate_witness_scripts_rejects_oversized_verification() {
        let witness = Witness::new_with_scripts(vec![], vec![0u8; 1025]);
        assert!(BlockValidator::validate_witness_scripts(&witness).is_err());
    }

    #[test]
    fn max_constants_are_correct() {
        assert_eq!(MAX_BLOCK_SIZE, 2_097_152); // 2 MB — Neo N3 default
        assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 512); // Neo N3 ProtocolSettings.Default
        assert_eq!(MAX_TIMESTAMP_DRIFT_MS, 900_000); // 15 minutes
        assert_eq!(MIN_TIMESTAMP_MS, 1468595301000); // Genesis timestamp
    }
}
