//! Verification traits for Neo blockchain.
//!
//! This module provides traits for transaction and block verification,
//! breaking the circular dependency between neo-p2p and neo-core
//! (Chain 2: Transaction → `ApplicationEngine`).
//!
//! # Design
//!
//! - `VerificationContext`: Abstracts witness verification without VM dependency
//! - `Witness`: Represents witness data (invocation + verification scripts)
//! - `BlockchainSnapshot`: Read-only blockchain state for verification
//!
//! # Example
//!
//! ```rust,ignore
//! use neo_primitives::{VerificationContext, Witness, VerificationError};
//!
//! struct MockVerifier { max_gas: i64, consumed: i64 }
//!
//! impl VerificationContext for MockVerifier {
//!     fn verify_witness(&self, hash: &UInt160, witness: &dyn Witness) -> Result<bool, VerificationError> {
//!         // Mock implementation - real one uses VM
//!         Ok(true)
//!     }
//!     fn get_gas_consumed(&self) -> i64 { self.consumed }
//!     fn get_max_gas(&self) -> i64 { self.max_gas }
//! }
//! ```

use crate::{UInt160, UInt256};
use thiserror::Error;

/// Errors that can occur during verification operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum VerificationError {
    /// Witness verification failed.
    #[error("witness verification failed: {message}")]
    VerificationFailed {
        /// Detailed error message.
        message: String,
    },

    /// Gas limit exceeded during verification.
    #[error("gas limit exceeded: consumed={consumed}, max={max}")]
    GasLimitExceeded {
        /// Gas consumed so far.
        consumed: i64,
        /// Maximum gas allowed.
        max: i64,
    },

    /// Invalid script error.
    #[error("invalid script: {message}")]
    InvalidScript {
        /// Error message describing the script issue.
        message: String,
    },

    /// Invalid signature error.
    #[error("invalid signature: {message}")]
    InvalidSignature {
        /// Error message describing the signature issue.
        message: String,
    },

    /// Missing witness error.
    #[error("missing witness for script hash: {hash}")]
    MissingWitness {
        /// The script hash that is missing a witness.
        hash: String,
    },
}

impl VerificationError {
    /// Create a verification failed error.
    pub fn verification_failed<S: Into<String>>(message: S) -> Self {
        Self::VerificationFailed {
            message: message.into(),
        }
    }

    /// Create a gas limit exceeded error.
    #[must_use]
    pub const fn gas_limit_exceeded(consumed: i64, max: i64) -> Self {
        Self::GasLimitExceeded { consumed, max }
    }

    /// Create an invalid script error.
    pub fn invalid_script<S: Into<String>>(message: S) -> Self {
        Self::InvalidScript {
            message: message.into(),
        }
    }

    /// Create an invalid signature error.
    pub fn invalid_signature<S: Into<String>>(message: S) -> Self {
        Self::InvalidSignature {
            message: message.into(),
        }
    }

    /// Create a missing witness error.
    #[must_use]
    pub fn missing_witness(hash: &UInt160) -> Self {
        Self::MissingWitness {
            hash: format!("{hash:?}"),
        }
    }
}

/// Result type for verification operations.
pub type VerificationResult<T> = Result<T, VerificationError>;

/// Trait for witness data.
///
/// A witness contains the invocation and verification scripts needed
/// to authorize a transaction or operation.
pub trait Witness: Send + Sync {
    /// Returns the invocation script (contains signatures/parameters).
    fn invocation_script(&self) -> &[u8];

    /// Returns the verification script (contains public keys/conditions).
    fn verification_script(&self) -> &[u8];
}

/// Context for verifying transactions and blocks.
///
/// This trait allows payloads (Transaction, Block) to verify themselves
/// without depending on the concrete `ApplicationEngine` implementation.
///
/// # Design Rationale
///
/// In the Neo C# implementation, `Transaction.VerifyWitnesses` directly
/// calls `ApplicationEngine.Run()`. This creates a tight coupling between
/// network payloads and VM execution.
///
/// By abstracting verification through this trait:
/// 1. `Transaction` (in neo-p2p) can verify itself via trait methods
/// 2. `ApplicationEngine` (in neo-core) implements this trait
/// 3. Tests can use mock implementations
///
/// # Performance
///
/// Trait dispatch adds ~2ns overhead per call. For verification operations
/// that take milliseconds (VM execution), this is negligible (<0.001%).
pub trait VerificationContext: Send + Sync {
    /// Verifies a witness script for the given script hash.
    ///
    /// # Arguments
    ///
    /// * `hash` - The script hash to verify (account or contract)
    /// * `witness` - The witness data containing invocation/verification scripts
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if verification succeeds
    /// - `Ok(false)` if signature is invalid
    /// - `Err(VerificationError)` if verification cannot complete
    ///
    /// # Errors
    ///
    /// Returns `VerificationError` if gas limit is exceeded, the script is invalid,
    /// the signature is invalid, or the witness is missing.
    fn verify_witness(&self, hash: &UInt160, witness: &dyn Witness) -> VerificationResult<bool>;

    /// Returns the total gas consumed during verification so far.
    fn get_gas_consumed(&self) -> i64;

    /// Returns the maximum gas allowed for verification.
    fn get_max_gas(&self) -> i64;

    /// Checks if verification should be aborted due to gas limit.
    ///
    /// Default implementation checks if consumed >= max.
    fn should_abort(&self) -> bool {
        self.get_gas_consumed() >= self.get_max_gas()
    }

    /// Returns the remaining gas available for verification.
    ///
    /// Default implementation returns max - consumed, saturated to 0.
    fn get_remaining_gas(&self) -> i64 {
        (self.get_max_gas() - self.get_gas_consumed()).max(0)
    }
}

/// Read-only snapshot of blockchain state for verification.
///
/// This trait provides access to blockchain state during verification
/// without requiring a mutable reference to the full blockchain.
///
/// # Design Rationale
///
/// During transaction verification, we need to:
/// 1. Check if referenced transactions exist
/// 2. Read contract storage
/// 3. Get current blockchain height
///
/// By abstracting through this trait, verification logic (in neo-p2p)
/// doesn't depend on concrete `DataCache` or `Blockchain` types (in neo-core).
pub trait BlockchainSnapshot: Send + Sync {
    /// Gets the current block height.
    fn height(&self) -> u32;

    /// Gets a storage value by key.
    ///
    /// # Arguments
    ///
    /// * `key` - The storage key (contract ID + key suffix)
    ///
    /// # Returns
    ///
    /// `Some(value)` if found, `None` if not present.
    fn get_storage(&self, key: &[u8]) -> Option<Vec<u8>>;

    /// Checks if a transaction exists in the blockchain.
    ///
    /// # Arguments
    ///
    /// * `hash` - The transaction hash
    ///
    /// # Returns
    ///
    /// `true` if the transaction exists, `false` otherwise.
    fn contains_transaction(&self, hash: &UInt256) -> bool;

    /// Checks if a block exists in the blockchain.
    ///
    /// # Arguments
    ///
    /// * `hash` - The block hash
    ///
    /// # Returns
    ///
    /// `true` if the block exists, `false` otherwise.
    fn contains_block(&self, hash: &UInt256) -> bool;

    /// Gets the hash of a block at the specified height.
    ///
    /// # Arguments
    ///
    /// * `height` - The block height
    ///
    /// # Returns
    ///
    /// `Some(hash)` if the block exists, `None` if height > current height.
    fn get_block_hash(&self, height: u32) -> Option<UInt256>;
}

#[cfg(test)]
mod tests;
