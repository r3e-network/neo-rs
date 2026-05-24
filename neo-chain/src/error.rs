//! Chain error types

use neo_primitives::UInt256;
use thiserror::Error;

/// Chain-related errors
#[derive(Debug, Error)]
pub enum ChainError {
    /// Block not found
    #[error("Block not found: {0}")]
    BlockNotFound(UInt256),

    /// Invalid block
    #[error("Invalid block: {0}")]
    InvalidBlock(String),

    /// Invalid block height
    #[error("Invalid block height: expected {expected}, got {actual}")]
    InvalidHeight { expected: u32, actual: u32 },

    /// Invalid previous hash
    #[error("Invalid previous hash: expected {expected}, got {actual}")]
    InvalidPrevHash { expected: UInt256, actual: UInt256 },

    /// Invalid merkle root
    #[error("Invalid merkle root")]
    InvalidMerkleRoot,

    /// Invalid timestamp
    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    /// Block already exists
    #[error("Block already exists: {0}")]
    BlockExists(UInt256),

    /// Orphan block (parent not found)
    #[error("Orphan block: parent {0} not found")]
    OrphanBlock(UInt256),

    /// Chain reorganization failed
    #[error("Chain reorganization failed: {0}")]
    ReorgFailed(String),

    /// State error
    #[error("State error: {0}")]
    StateError(String),

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Genesis block mismatch
    #[error("Genesis block mismatch")]
    GenesisMismatch,

    /// Chain not initialized
    #[error("Chain not initialized")]
    NotInitialized,
}

/// Result type for chain operations
pub type ChainResult<T> = Result<T, ChainError>;
