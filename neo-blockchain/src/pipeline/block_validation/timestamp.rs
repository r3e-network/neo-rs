use neo_primitives::TimeProvider;

use super::{BlockValidationError, BlockValidator};

/// Maximum allowed timestamp drift from current time (15 minutes in milliseconds).
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 15 * 60 * 1000;

/// Minimum valid timestamp (Neo genesis block timestamp: July 15, 2016).
pub const MIN_TIMESTAMP_MS: u64 = 1468595301000;

impl BlockValidator {
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
}
