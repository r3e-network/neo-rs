use neo_primitives::UInt256;
use thiserror::Error;

/// Block validation error types.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum BlockValidationError {
    /// Block exceeds maximum size.
    #[error("Block size {size} exceeds maximum {max_size}")]
    BlockTooLarge {
        /// Actual serialized block size.
        size: usize,
        /// Maximum allowed serialized block size.
        max_size: usize,
    },
    /// Too many transactions in block.
    #[error("Transaction count {count} exceeds maximum {max_count}")]
    TooManyTransactions {
        /// Actual transaction count.
        count: usize,
        /// Maximum allowed transaction count.
        max_count: usize,
    },
    /// Timestamp is in the future beyond allowed drift.
    #[error("Timestamp {timestamp} is too far in future (current: {current})")]
    TimestampTooFarInFuture {
        /// Block timestamp in milliseconds.
        timestamp: u64,
        /// Current node time in milliseconds.
        current: u64,
    },
    /// Timestamp is too old (before genesis).
    #[error("Timestamp {timestamp} is before minimum {min}")]
    TimestampTooOld {
        /// Block timestamp in milliseconds.
        timestamp: u64,
        /// Minimum accepted timestamp in milliseconds.
        min: u64,
    },
    /// Timestamp is not strictly increasing from previous.
    #[error("Timestamp {timestamp} must be greater than previous {prev_timestamp}")]
    TimestampNotIncreasing {
        /// Block timestamp in milliseconds.
        timestamp: u64,
        /// Previous block timestamp in milliseconds.
        prev_timestamp: u64,
    },
    /// Merkle root does not match computed root.
    #[error("Merkle root mismatch: expected {expected}, computed {computed}")]
    InvalidMerkleRoot {
        /// Merkle root declared by the block header.
        expected: UInt256,
        /// Merkle root recomputed from transactions.
        computed: UInt256,
    },
    /// Duplicate transaction hashes found.
    #[error("Block contains duplicate transactions")]
    DuplicateTransactions,
    /// Transaction verification failed.
    #[error("Transaction {hash} at index {index} failed verification")]
    TransactionVerificationFailed {
        /// Transaction index within the block.
        index: usize,
        /// Transaction hash.
        hash: UInt256,
    },
    /// Witness script validation failed.
    #[error("Invalid witness script: {reason}")]
    InvalidWitnessScript {
        /// Validation failure reason.
        reason: String,
    },
    /// Empty block when transactions expected.
    #[error("Block has empty transaction list")]
    EmptyTransactionList,
    /// Block version not supported.
    #[error("Block version {version} is not supported")]
    UnsupportedVersion {
        /// Unsupported block version.
        version: u32,
    },
    /// Primary index out of range.
    #[error("Primary index {index} exceeds maximum validator count {max}")]
    InvalidPrimaryIndex {
        /// Primary validator index from the block header.
        index: u8,
        /// Maximum valid validator index.
        max: i32,
    },
    /// Header validation failed.
    #[error("Header validation failed: {reason}")]
    HeaderValidationFailed {
        /// Validation failure reason.
        reason: String,
    },
}
