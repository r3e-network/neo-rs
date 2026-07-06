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
use neo_payloads::{Block, Witness};
use neo_primitives::UInt256;

mod error;
mod limits;
mod timestamp;

pub use error::BlockValidationError;
pub use timestamp::{MAX_TIMESTAMP_DRIFT_MS, MIN_TIMESTAMP_MS};

/// Maximum size of witness scripts in bytes
const MAX_WITNESS_SCRIPT_SIZE: usize = 1024;

/// Stateless block-validation checks.
///
/// The pure validation layer grouped onto a single zero-sized type: every
/// check is an associated function (none carry state), so callers spell them
/// `BlockValidator::validate_*`.
pub struct BlockValidator;

impl BlockValidator {
    /// Validates the stateless block-integrity checks shared by live inventory
    /// import and the reusable [`neo_runtime::BlockImport::check`] boundary.
    ///
    /// This intentionally mirrors the structural subset of C# `Block.Verify`
    /// used by `Blockchain.OnNewBlock`: block version, transaction merkle root,
    /// and duplicate transaction hashes. It does **not** enforce
    /// `MaxTransactionsPerBlock`; Neo C# treats that as a dBFT block-production
    /// limit rather than a peer block validity rule.
    pub fn validate_import_integrity(block: &Block) -> Result<(), BlockValidationError> {
        Self::validate_block_version(block.version())?;
        let tx_hashes = block.transaction_hashes().map_err(|err| {
            BlockValidationError::HeaderValidationFailed {
                reason: format!("failed to hash block transactions: {err}"),
            }
        })?;
        Self::validate_merkle_root(block.header.merkle_root(), &tx_hashes)?;
        Self::validate_no_duplicate_transactions(&tx_hashes)
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

#[cfg(test)]
#[path = "../tests/pipeline/block_validation.rs"]
mod tests;
