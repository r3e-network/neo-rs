use neo_payloads::Witness;

use super::{BlockValidationError, BlockValidator};

/// Maximum size of witness scripts in bytes.
const MAX_WITNESS_SCRIPT_SIZE: usize = 1024;

impl BlockValidator {
    /// Validates witness scripts in the block header.
    ///
    /// Checks:
    /// - Invocation script is within size limits
    /// - Verification script is within size limits
    /// - Verification script does not start with a reserved opcode when present
    ///
    /// # Arguments
    /// * `witness` - The header witness to validate
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
}
