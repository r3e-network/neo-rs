use super::{BlockValidationError, BlockValidator};

impl BlockValidator {
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
