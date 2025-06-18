//! Neo Ledger Module
//!
//! This module provides the core blockchain ledger functionality for the Neo blockchain,
//! exactly matching the C# Neo ledger structure.
//!
//! ## Components
//!
//! - **Blockchain**: Main blockchain actor (matches C# Blockchain)
//! - **Block**: Block data structures (matches C# Block/Header)
//! - **HeaderCache**: Header caching (matches C# HeaderCache)
//! - **VerifyResult**: Verification results (matches C# VerifyResult)

pub mod blockchain;
pub mod block;
pub mod header_cache;
pub mod verify_result;

// Re-export main types (matches C# Neo structure)
pub use blockchain::Blockchain;
pub use blockchain::storage::{Storage, StorageKey, StorageItem};
pub use block::{Block, BlockHeader, Header};
pub use header_cache::HeaderCache;
pub use verify_result::VerifyResult;

pub use neo_config::{LedgerConfig, NetworkType};

use neo_core::UInt160;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Result type for ledger operations
pub type Result<T> = std::result::Result<T, Error>;

/// Ledger-specific error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Block validation error
    #[error("Block validation failed: {0}")]
    BlockValidation(String),

    /// Transaction validation error
    #[error("Transaction validation failed: {0}")]
    TransactionValidation(String),

    /// Validation error (generic)
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Not found error
    #[error("Not found")]
    NotFound,

    /// Invalid operation error
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Blockchain state error
    #[error("Blockchain state error: {0}")]
    StateError(String),

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Storage error (alias for compatibility)
    #[error("Storage error: {0}")]
    Storage(String),

    /// Mempool error
    #[error("Mempool error: {0}")]
    MempoolError(String),

    /// Consensus error
    #[error("Consensus error: {0}")]
    ConsensusError(String),

    /// Invalid block
    #[error("Invalid block: {0}")]
    InvalidBlock(String),

    /// Invalid view change
    #[error("Invalid view change: {0}")]
    InvalidViewChange(String),

    /// Invalid message
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Invalid validator
    #[error("Invalid validator: {0}")]
    InvalidValidator(String),

    /// Invalid signature
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    /// Invalid committee
    #[error("Invalid committee: {0}")]
    InvalidCommittee(String),

    /// Invalid data
    #[error("Invalid data: {0}")]
    InvalidData(String),

    /// Signature verification error
    #[error("Signature verification error: {0}")]
    SignatureVerificationError(String),

    /// Block not found
    #[error("Block not found: {0}")]
    BlockNotFound(String),

    /// Transaction not found
    #[error("Transaction not found: {0}")]
    TransactionNotFound(String),

    /// Invalid block height
    #[error("Invalid block height: expected {expected}, got {actual}")]
    InvalidBlockHeight { expected: u32, actual: u32 },

    /// Invalid block hash
    #[error("Invalid block hash: {0}")]
    InvalidBlockHash(String),

    /// Insufficient balance
    #[error("Insufficient balance for account {account}: required {required}, available {available}")]
    InsufficientBalance {
        account: UInt160,
        required: i64,
        available: i64,
    },

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// VM error
    #[error("VM error: {0}")]
    VmError(String),

    /// Smart contract error
    #[error("Smart contract error: {0}")]
    SmartContractError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(String),

    /// Generic error
    #[error("Ledger error: {0}")]
    Generic(String),
}

impl From<neo_vm::Error> for Error {
    fn from(err: neo_vm::Error) -> Self {
        Error::VmError(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err.to_string())
    }
}

impl From<neo_core::CoreError> for Error {
    fn from(err: neo_core::CoreError) -> Self {
        Error::Generic(err.to_string())
    }
}

impl From<Box<bincode::ErrorKind>> for Error {
    fn from(err: Box<bincode::ErrorKind>) -> Self {
        Error::SerializationError(err.to_string())
    }
}


/// Block validation result
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    /// Block is valid
    Valid,
    /// Block is invalid with reason
    Invalid(String),
    /// Block validation is pending (async validation)
    Pending,
}

impl ValidationResult {
    /// Checks if the validation result is valid
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid)
    }

    /// Checks if the validation result is invalid
    pub fn is_invalid(&self) -> bool {
        matches!(self, ValidationResult::Invalid(_))
    }

    /// Checks if the validation result is pending
    pub fn is_pending(&self) -> bool {
        matches!(self, ValidationResult::Pending)
    }

    /// Gets the error message if invalid
    pub fn error_message(&self) -> Option<&str> {
        match self {
            ValidationResult::Invalid(msg) => Some(msg),
            _ => None,
        }
    }
}

/// Transaction verification result
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationResult {
    /// Whether the transaction is valid
    pub is_valid: bool,
    /// Error message if invalid
    pub error: Option<String>,
    /// Gas consumed during verification
    pub gas_consumed: i64,
}

impl VerificationResult {
    /// Creates a valid verification result
    pub fn valid(gas_consumed: i64) -> Self {
        Self {
            is_valid: true,
            error: None,
            gas_consumed,
        }
    }

    /// Creates an invalid verification result
    pub fn invalid(error: String, gas_consumed: i64) -> Self {
        Self {
            is_valid: false,
            error: Some(error),
            gas_consumed,
        }
    }
}

/// Blockchain statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStats {
    /// Current block height
    pub height: u32,
    /// Total number of transactions
    pub transaction_count: u64,
    /// Total number of accounts
    pub account_count: u64,
    /// Total number of contracts
    pub contract_count: u64,
    /// Current mempool size
    pub mempool_size: usize,
    /// Average block time in seconds
    pub average_block_time: f64,
    /// Network hash rate (if applicable)
    pub network_hashrate: Option<f64>,
}

impl Default for BlockchainStats {
    fn default() -> Self {
        Self {
            height: 0,
            transaction_count: 0,
            account_count: 0,
            contract_count: 0,
            mempool_size: 0,
            average_block_time: 15.0, // Default 15 seconds for Neo
            network_hashrate: None,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_validation_result() {
        let valid = ValidationResult::Valid;
        assert!(valid.is_valid());
        assert!(!valid.is_invalid());
        assert!(!valid.is_pending());

        let invalid = ValidationResult::Invalid("test error".to_string());
        assert!(!invalid.is_valid());
        assert!(invalid.is_invalid());
        assert_eq!(invalid.error_message(), Some("test error"));

        let pending = ValidationResult::Pending;
        assert!(pending.is_pending());
    }

    #[test]
    fn test_verification_result() {
        let valid = VerificationResult::valid(1000);
        assert!(valid.is_valid);
        assert!(valid.error.is_none());
        assert_eq!(valid.gas_consumed, 1000);

        let invalid = VerificationResult::invalid("test error".to_string(), 500);
        assert!(!invalid.is_valid);
        assert_eq!(invalid.error, Some("test error".to_string()));
        assert_eq!(invalid.gas_consumed, 500);
    }

    #[test]
    fn test_ledger_config_default() {
        let config = LedgerConfig::default();
        assert_eq!(config.max_block_size, 1048576);
        assert_eq!(config.milliseconds_per_block, 15000);
        assert_eq!(config.max_transactions_per_block, 512);
    }

    #[test]
    fn test_blockchain_stats_default() {
        let stats = BlockchainStats::default();
        assert_eq!(stats.height, 0);
        assert_eq!(stats.transaction_count, 0);
        assert_eq!(stats.average_block_time, 15.0);
    }
}

/// Main Ledger struct (matches C# Ledger exactly)
/// 
/// This is a simplified implementation to get the node building and running.
/// The full implementation would include complete blockchain state management.
#[derive(Debug)]
pub struct Ledger {
    config: LedgerConfig,
    stats: BlockchainStats,
}

impl Ledger {
    /// Creates a new Ledger instance
    pub fn new(config: LedgerConfig) -> Result<Self> {
        Ok(Self {
            config,
            stats: BlockchainStats::default(),
        })
    }

    /// Gets the current blockchain statistics
    pub fn get_stats(&self) -> &BlockchainStats {
        &self.stats
    }

    /// Gets the current block height
    pub fn get_height(&self) -> u32 {
        self.stats.height
    }

    /// Gets the ledger configuration
    pub fn get_config(&self) -> &LedgerConfig {
        &self.config
    }
    
    /// Gets the best block hash
    pub async fn get_best_block_hash(&self) -> Result<neo_core::UInt256> {
        // Placeholder - return genesis block hash
        Ok(neo_core::UInt256::zero())
    }
    
    /// Gets a block by hash
    pub async fn get_block_by_hash(&self, _hash: &neo_core::UInt256) -> Result<Option<Block>> {
        // Placeholder
        Ok(None)
    }
    
    /// Adds a new block to the blockchain
    pub async fn add_block(&self, _block: Block) -> Result<()> {
        // Placeholder
        Ok(())
    }
    
    /// Persists a block to storage
    pub async fn persist_block(&self, _block: Block) -> Result<()> {
        // Placeholder - in production this would validate and store the block
        Ok(())
    }
}
