//! Mempool error types

use neo_primitives::UInt256;
use thiserror::Error;

/// Mempool-related errors
#[derive(Debug, Error)]
pub enum MempoolError {
    /// Transaction already exists in pool
    #[error("Transaction already exists: {0}")]
    DuplicateTransaction(UInt256),

    /// Pool is at capacity
    #[error("Mempool is full (capacity: {0})")]
    PoolFull(usize),

    /// Transaction has insufficient fee
    #[error("Insufficient fee: required {required}, got {actual}")]
    InsufficientFee { required: i64, actual: i64 },

    /// Transaction is expired
    #[error("Transaction expired at block {0}")]
    Expired(u32),

    /// Transaction validation failed
    #[error("Transaction validation failed: {0}")]
    ValidationFailed(String),

    /// Transaction conflicts with existing transaction
    #[error("Transaction conflicts with existing: {0}")]
    Conflict(UInt256),

    /// Sender has too many pending transactions
    #[error("Too many transactions from sender: {0}")]
    TooManyFromSender(usize),

    /// Internal error
    #[error("Internal mempool error: {0}")]
    Internal(String),
}

/// Result type for mempool operations
pub type MempoolResult<T> = Result<T, MempoolError>;
