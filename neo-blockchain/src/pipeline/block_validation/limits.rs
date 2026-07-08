use neo_primitives::blockchain::marker_traits::BlockLike;
use neo_primitives::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};

use super::{BlockValidationError, BlockValidator};

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
    /// Neo's built-in default is 512, but MainNet/TestNet v3.10.1 configurations
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
}
