//! Block validation logic

use crate::{ChainError, ChainResult, ChainState, MAX_TIME_DRIFT_SECS};
use neo_primitives::UInt256;
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of block validation
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// Block is valid
    Valid,

    /// Block is invalid with reason
    Invalid(String),

    /// Block validation is incomplete (needs more data)
    Incomplete(String),
}

impl ValidationResult {
    /// Check if validation passed
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid)
    }

    /// Convert to Result
    pub fn into_result(self) -> ChainResult<()> {
        match self {
            ValidationResult::Valid => Ok(()),
            ValidationResult::Invalid(reason) => Err(ChainError::ValidationError(reason)),
            ValidationResult::Incomplete(reason) => Err(ChainError::ValidationError(format!(
                "Incomplete validation: {}",
                reason
            ))),
        }
    }
}

/// Block data required for validation
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Block hash
    pub hash: UInt256,

    /// Previous block hash
    pub prev_hash: UInt256,

    /// Block height
    pub height: u32,

    /// Block timestamp (milliseconds since Unix epoch)
    pub timestamp: u64,

    /// Merkle root of transactions
    pub merkle_root: UInt256,

    /// Next consensus address
    pub next_consensus: UInt256,

    /// Number of transactions
    pub tx_count: usize,

    /// Block size in bytes
    pub size: usize,

    /// Witness data present
    pub has_witness: bool,
}

/// Block validator
pub struct BlockValidator {
    /// Maximum allowed block size
    max_block_size: usize,

    /// Maximum transactions per block
    max_transactions_per_block: usize,

    /// Expected block time (milliseconds)
    #[allow(dead_code)]
    expected_block_time_ms: u64,
}

impl BlockValidator {
    /// Create a new block validator with default settings
    pub fn new() -> Self {
        Self {
            max_block_size: 5 * 1024 * 1024, // 5 MB
            max_transactions_per_block: 512,
            expected_block_time_ms: 15_000, // 15 seconds
        }
    }

    /// Create with custom settings
    pub fn with_settings(
        max_block_size: usize,
        max_transactions_per_block: usize,
        expected_block_time_ms: u64,
    ) -> Self {
        Self {
            max_block_size,
            max_transactions_per_block,
            expected_block_time_ms,
        }
    }

    /// Validate a block against chain state
    pub fn validate(&self, block: &BlockData, chain_state: &ChainState) -> ValidationResult {
        // Check basic validity first (doesn't need chain state)
        let basic_result = self.validate_basic(block);
        if !basic_result.is_valid() {
            return basic_result;
        }

        // Check contextual validity (needs chain state)
        self.validate_contextual(block, chain_state)
    }

    /// Perform basic validation (stateless)
    pub fn validate_basic(&self, block: &BlockData) -> ValidationResult {
        // Check block size
        if block.size > self.max_block_size {
            return ValidationResult::Invalid(format!(
                "Block size {} exceeds maximum {}",
                block.size, self.max_block_size
            ));
        }

        // Check transaction count
        if block.tx_count > self.max_transactions_per_block {
            return ValidationResult::Invalid(format!(
                "Transaction count {} exceeds maximum {}",
                block.tx_count, self.max_transactions_per_block
            ));
        }

        // Check timestamp is not too far in the future
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let max_future_ms = MAX_TIME_DRIFT_SECS * 1000;
        if block.timestamp > now_ms + max_future_ms {
            return ValidationResult::Invalid(format!(
                "Block timestamp {} is too far in the future (max allowed: {})",
                block.timestamp,
                now_ms + max_future_ms
            ));
        }

        // Check hash is not zero
        if block.hash == UInt256::zero() {
            return ValidationResult::Invalid("Block hash cannot be zero".to_string());
        }

        // Check merkle root is not zero (unless empty block)
        if block.tx_count > 0 && block.merkle_root == UInt256::zero() {
            return ValidationResult::Invalid(
                "Merkle root cannot be zero for non-empty block".to_string(),
            );
        }

        // Check witness is present
        if !block.has_witness {
            return ValidationResult::Invalid("Block must have witness data".to_string());
        }

        ValidationResult::Valid
    }

    /// Perform contextual validation (requires chain state)
    pub fn validate_contextual(
        &self,
        block: &BlockData,
        chain_state: &ChainState,
    ) -> ValidationResult {
        // Check chain is initialized
        if !chain_state.is_initialized() {
            return ValidationResult::Incomplete("Chain not initialized".to_string());
        }

        // Check parent exists
        if block.height > 0 {
            match chain_state.get_block(&block.prev_hash) {
                Some(parent) => {
                    // Validate height continuity
                    if block.height != parent.height + 1 {
                        return ValidationResult::Invalid(format!(
                            "Invalid height: expected {}, got {}",
                            parent.height + 1,
                            block.height
                        ));
                    }

                    // Validate timestamp is after parent
                    if block.timestamp <= parent.timestamp {
                        return ValidationResult::Invalid(format!(
                            "Block timestamp {} must be after parent timestamp {}",
                            block.timestamp, parent.timestamp
                        ));
                    }
                }
                None => {
                    return ValidationResult::Incomplete(format!(
                        "Parent block {} not found",
                        block.prev_hash
                    ));
                }
            }
        } else {
            // Genesis block validation
            if block.prev_hash != UInt256::zero() {
                return ValidationResult::Invalid(
                    "Genesis block must have zero prev_hash".to_string(),
                );
            }
        }

        ValidationResult::Valid
    }

    /// Validate genesis block
    pub fn validate_genesis(&self, block: &BlockData) -> ValidationResult {
        // Genesis must be height 0
        if block.height != 0 {
            return ValidationResult::Invalid(format!(
                "Genesis block must have height 0, got {}",
                block.height
            ));
        }

        // Genesis must have zero prev_hash
        if block.prev_hash != UInt256::zero() {
            return ValidationResult::Invalid(
                "Genesis block must have zero prev_hash".to_string(),
            );
        }

        // Basic validation
        self.validate_basic(block)
    }
}

impl Default for BlockValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_valid_block(height: u32) -> BlockData {
        let mut hash = [0u8; 32];
        hash[0] = height as u8 + 1;

        let mut prev = [0u8; 32];
        if height > 0 {
            prev[0] = height as u8;
        }

        let mut merkle = [0u8; 32];
        merkle[0] = 0xFF;

        BlockData {
            hash: UInt256::from(hash),
            prev_hash: UInt256::from(prev),
            height,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            merkle_root: UInt256::from(merkle),
            next_consensus: UInt256::from([0xABu8; 32]),
            tx_count: 1,
            size: 1000,
            has_witness: true,
        }
    }

    #[test]
    fn test_valid_block() {
        let validator = BlockValidator::new();
        let block = create_valid_block(1);

        let result = validator.validate_basic(&block);
        assert!(result.is_valid());
    }

    #[test]
    fn test_oversized_block() {
        let validator = BlockValidator::new();
        let mut block = create_valid_block(1);
        block.size = 10 * 1024 * 1024; // 10 MB

        let result = validator.validate_basic(&block);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_too_many_transactions() {
        let validator = BlockValidator::new();
        let mut block = create_valid_block(1);
        block.tx_count = 1000;

        let result = validator.validate_basic(&block);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_future_timestamp() {
        let validator = BlockValidator::new();
        let mut block = create_valid_block(1);
        // Set timestamp 1 hour in the future
        block.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 3600_000;

        let result = validator.validate_basic(&block);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_valid_genesis() {
        let validator = BlockValidator::new();
        let mut block = create_valid_block(0);
        block.prev_hash = UInt256::zero();

        let result = validator.validate_genesis(&block);
        assert!(result.is_valid());
    }

    #[test]
    fn test_invalid_genesis_height() {
        let validator = BlockValidator::new();
        let mut block = create_valid_block(1);
        block.prev_hash = UInt256::zero();

        let result = validator.validate_genesis(&block);
        assert!(!result.is_valid());
    }
}
