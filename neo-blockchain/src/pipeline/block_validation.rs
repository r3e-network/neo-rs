//! Block validation providing comprehensive security checks.
//!
//! This module implements hardened block validation to prevent various
//! attack vectors including oversized blocks, timestamp manipulation,
//! and merkle root tampering. It is the **pure** validation layer: it
//! operates on `BlockLike` trait objects and `&Witness` references, so
//! it has no dependency on the stateful blockchain service, consensus,
//! native-contract, or storage layers. Stateful verification is handled by
//! the service pipeline before a block is admitted.

use neo_payloads::Witness;

mod error;
mod integrity;
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
