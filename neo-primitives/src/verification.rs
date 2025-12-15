//! Verification traits for Neo blockchain.
//!
//! This module provides traits for transaction and block verification,
//! breaking the circular dependency between neo-p2p and neo-core
//! (Chain 2: Transaction → ApplicationEngine).
//!
//! # Design
//!
//! - `IVerificationContext`: Abstracts witness verification without VM dependency
//! - `IWitness`: Represents witness data (invocation + verification scripts)
//! - `IBlockchainSnapshot`: Read-only blockchain state for verification
//!
//! # Example
//!
//! ```rust,ignore
//! use neo_primitives::{IVerificationContext, IWitness, VerificationError};
//!
//! struct MockVerifier { max_gas: i64, consumed: i64 }
//!
//! impl IVerificationContext for MockVerifier {
//!     fn verify_witness(&self, hash: &UInt160, witness: &dyn IWitness) -> Result<bool, VerificationError> {
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
        Self::VerificationFailed { message: message.into() }
    }

    /// Create a gas limit exceeded error.
    pub fn gas_limit_exceeded(consumed: i64, max: i64) -> Self {
        Self::GasLimitExceeded { consumed, max }
    }

    /// Create an invalid script error.
    pub fn invalid_script<S: Into<String>>(message: S) -> Self {
        Self::InvalidScript { message: message.into() }
    }

    /// Create an invalid signature error.
    pub fn invalid_signature<S: Into<String>>(message: S) -> Self {
        Self::InvalidSignature { message: message.into() }
    }

    /// Create a missing witness error.
    pub fn missing_witness(hash: &UInt160) -> Self {
        Self::MissingWitness { hash: format!("{:?}", hash) }
    }
}

/// Result type for verification operations.
pub type VerificationResult<T> = Result<T, VerificationError>;

/// Trait for witness data.
///
/// A witness contains the invocation and verification scripts needed
/// to authorize a transaction or operation.
pub trait IWitness: Send + Sync {
    /// Returns the invocation script (contains signatures/parameters).
    fn invocation_script(&self) -> &[u8];

    /// Returns the verification script (contains public keys/conditions).
    fn verification_script(&self) -> &[u8];
}

/// Context for verifying transactions and blocks.
///
/// This trait allows payloads (Transaction, Block) to verify themselves
/// without depending on the concrete ApplicationEngine implementation.
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
pub trait IVerificationContext: Send + Sync {
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
    fn verify_witness(
        &self,
        hash: &UInt160,
        witness: &dyn IWitness,
    ) -> VerificationResult<bool>;

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
pub trait IBlockchainSnapshot: Send + Sync {
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
mod tests {
    use super::*;

    // ============ Mock Implementations ============

    /// Mock witness for testing.
    #[derive(Debug, Clone)]
    struct MockWitness {
        invocation: Vec<u8>,
        verification: Vec<u8>,
    }

    impl MockWitness {
        fn new(invocation: Vec<u8>, verification: Vec<u8>) -> Self {
            Self { invocation, verification }
        }
    }

    impl IWitness for MockWitness {
        fn invocation_script(&self) -> &[u8] {
            &self.invocation
        }

        fn verification_script(&self) -> &[u8] {
            &self.verification
        }
    }

    /// Mock verification context for testing.
    struct MockVerifier {
        max_gas: i64,
        consumed: i64,
        should_pass: bool,
    }

    impl MockVerifier {
        fn new(max_gas: i64, should_pass: bool) -> Self {
            Self { max_gas, consumed: 0, should_pass }
        }

        fn with_consumed(mut self, consumed: i64) -> Self {
            self.consumed = consumed;
            self
        }
    }

    impl IVerificationContext for MockVerifier {
        fn verify_witness(
            &self,
            _hash: &UInt160,
            _witness: &dyn IWitness,
        ) -> VerificationResult<bool> {
            if self.should_pass {
                Ok(true)
            } else {
                Ok(false)
            }
        }

        fn get_gas_consumed(&self) -> i64 {
            self.consumed
        }

        fn get_max_gas(&self) -> i64 {
            self.max_gas
        }
    }

    /// Mock blockchain snapshot for testing.
    struct MockSnapshot {
        height: u32,
        storage: std::collections::HashMap<Vec<u8>, Vec<u8>>,
        transactions: std::collections::HashSet<UInt256>,
        blocks: std::collections::HashSet<UInt256>,
    }

    impl MockSnapshot {
        fn new(height: u32) -> Self {
            Self {
                height,
                storage: std::collections::HashMap::new(),
                transactions: std::collections::HashSet::new(),
                blocks: std::collections::HashSet::new(),
            }
        }

        fn with_storage(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
            self.storage.insert(key, value);
            self
        }

        fn with_transaction(mut self, hash: UInt256) -> Self {
            self.transactions.insert(hash);
            self
        }
    }

    impl IBlockchainSnapshot for MockSnapshot {
        fn height(&self) -> u32 {
            self.height
        }

        fn get_storage(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.storage.get(key).cloned()
        }

        fn contains_transaction(&self, hash: &UInt256) -> bool {
            self.transactions.contains(hash)
        }

        fn contains_block(&self, hash: &UInt256) -> bool {
            self.blocks.contains(hash)
        }

        fn get_block_hash(&self, height: u32) -> Option<UInt256> {
            if height <= self.height {
                Some(UInt256::zero()) // Simplified for testing
            } else {
                None
            }
        }
    }

    // ============ VerificationError Tests ============

    #[test]
    fn test_verification_error_verification_failed() {
        let err = VerificationError::verification_failed("bad signature");
        assert!(err.to_string().contains("witness verification failed"));
        assert!(err.to_string().contains("bad signature"));
    }

    #[test]
    fn test_verification_error_gas_limit_exceeded() {
        let err = VerificationError::gas_limit_exceeded(100, 50);
        assert!(err.to_string().contains("gas limit exceeded"));
        assert!(err.to_string().contains("consumed=100"));
        assert!(err.to_string().contains("max=50"));
    }

    #[test]
    fn test_verification_error_invalid_script() {
        let err = VerificationError::invalid_script("empty script");
        assert!(err.to_string().contains("invalid script"));
        assert!(err.to_string().contains("empty script"));
    }

    #[test]
    fn test_verification_error_invalid_signature() {
        let err = VerificationError::invalid_signature("wrong key");
        assert!(err.to_string().contains("invalid signature"));
        assert!(err.to_string().contains("wrong key"));
    }

    #[test]
    fn test_verification_error_missing_witness() {
        let hash = UInt160::zero();
        let err = VerificationError::missing_witness(&hash);
        assert!(err.to_string().contains("missing witness"));
    }

    #[test]
    fn test_verification_error_clone() {
        let err1 = VerificationError::verification_failed("test");
        let err2 = err1.clone();
        assert_eq!(err1, err2);
    }

    // ============ IWitness Tests ============

    #[test]
    fn test_mock_witness_scripts() {
        let witness = MockWitness::new(vec![0x01, 0x02], vec![0x03, 0x04]);
        assert_eq!(witness.invocation_script(), &[0x01, 0x02]);
        assert_eq!(witness.verification_script(), &[0x03, 0x04]);
    }

    #[test]
    fn test_mock_witness_empty_scripts() {
        let witness = MockWitness::new(vec![], vec![]);
        assert!(witness.invocation_script().is_empty());
        assert!(witness.verification_script().is_empty());
    }

    // ============ IVerificationContext Tests ============

    #[test]
    fn test_mock_verifier_passes() {
        let verifier = MockVerifier::new(1000, true);
        let hash = UInt160::zero();
        let witness = MockWitness::new(vec![0x01], vec![0x02]);

        let result = verifier.verify_witness(&hash, &witness);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_mock_verifier_fails() {
        let verifier = MockVerifier::new(1000, false);
        let hash = UInt160::zero();
        let witness = MockWitness::new(vec![0x01], vec![0x02]);

        let result = verifier.verify_witness(&hash, &witness);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_mock_verifier_gas_tracking() {
        let verifier = MockVerifier::new(1000, true).with_consumed(500);
        assert_eq!(verifier.get_gas_consumed(), 500);
        assert_eq!(verifier.get_max_gas(), 1000);
        assert_eq!(verifier.get_remaining_gas(), 500);
    }

    #[test]
    fn test_mock_verifier_should_abort_false() {
        let verifier = MockVerifier::new(1000, true).with_consumed(500);
        assert!(!verifier.should_abort());
    }

    #[test]
    fn test_mock_verifier_should_abort_true() {
        let verifier = MockVerifier::new(1000, true).with_consumed(1000);
        assert!(verifier.should_abort());
    }

    #[test]
    fn test_mock_verifier_should_abort_exceeded() {
        let verifier = MockVerifier::new(1000, true).with_consumed(1500);
        assert!(verifier.should_abort());
    }

    #[test]
    fn test_remaining_gas_saturating() {
        let verifier = MockVerifier::new(100, true).with_consumed(200);
        // Should saturate to 0, not negative
        assert_eq!(verifier.get_remaining_gas(), 0);
    }

    // ============ IBlockchainSnapshot Tests ============

    #[test]
    fn test_mock_snapshot_height() {
        let snapshot = MockSnapshot::new(12345);
        assert_eq!(snapshot.height(), 12345);
    }

    #[test]
    fn test_mock_snapshot_storage() {
        let snapshot = MockSnapshot::new(100)
            .with_storage(vec![0x01, 0x02], vec![0xAA, 0xBB]);

        assert_eq!(snapshot.get_storage(&[0x01, 0x02]), Some(vec![0xAA, 0xBB]));
        assert_eq!(snapshot.get_storage(&[0x03, 0x04]), None);
    }

    #[test]
    fn test_mock_snapshot_contains_transaction() {
        let tx_hash = UInt256::from_bytes(&[1u8; 32]).unwrap_or_default();
        let snapshot = MockSnapshot::new(100).with_transaction(tx_hash);

        assert!(snapshot.contains_transaction(&tx_hash));
        assert!(!snapshot.contains_transaction(&UInt256::zero()));
    }

    #[test]
    fn test_mock_snapshot_contains_block() {
        let snapshot = MockSnapshot::new(100);
        // MockSnapshot doesn't add any blocks by default
        assert!(!snapshot.contains_block(&UInt256::zero()));
    }

    #[test]
    fn test_mock_snapshot_get_block_hash() {
        let snapshot = MockSnapshot::new(100);

        // Height within range
        assert!(snapshot.get_block_hash(50).is_some());
        assert!(snapshot.get_block_hash(100).is_some());

        // Height out of range
        assert!(snapshot.get_block_hash(101).is_none());
    }

    // ============ Trait Object Tests ============

    #[test]
    fn test_witness_as_trait_object() {
        fn accept_witness(w: &dyn IWitness) -> usize {
            w.invocation_script().len() + w.verification_script().len()
        }

        let witness = MockWitness::new(vec![0x01, 0x02, 0x03], vec![0x04, 0x05]);
        assert_eq!(accept_witness(&witness), 5);
    }

    #[test]
    fn test_verifier_as_trait_object() {
        fn accept_verifier(v: &dyn IVerificationContext) -> i64 {
            v.get_remaining_gas()
        }

        let verifier = MockVerifier::new(1000, true).with_consumed(300);
        assert_eq!(accept_verifier(&verifier), 700);
    }

    #[test]
    fn test_snapshot_as_trait_object() {
        fn accept_snapshot(s: &dyn IBlockchainSnapshot) -> u32 {
            s.height()
        }

        let snapshot = MockSnapshot::new(42);
        assert_eq!(accept_snapshot(&snapshot), 42);
    }

    // ============ Send + Sync Tests ============

    #[test]
    fn test_verification_error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VerificationError>();
    }

    // ============ Additional Coverage Tests ============

    #[test]
    fn test_verification_error_all_variants_eq() {
        // Test PartialEq for all error variants
        let err1 = VerificationError::VerificationFailed { message: "test".to_string() };
        let err2 = VerificationError::VerificationFailed { message: "test".to_string() };
        let err3 = VerificationError::VerificationFailed { message: "other".to_string() };
        assert_eq!(err1, err2);
        assert_ne!(err1, err3);

        let err4 = VerificationError::GasLimitExceeded { consumed: 100, max: 50 };
        let err5 = VerificationError::GasLimitExceeded { consumed: 100, max: 50 };
        assert_eq!(err4, err5);
        assert_ne!(err1, err4);

        let err6 = VerificationError::InvalidScript { message: "bad".to_string() };
        let err7 = VerificationError::InvalidScript { message: "bad".to_string() };
        assert_eq!(err6, err7);

        let err8 = VerificationError::InvalidSignature { message: "wrong".to_string() };
        let err9 = VerificationError::InvalidSignature { message: "wrong".to_string() };
        assert_eq!(err8, err9);

        let err10 = VerificationError::MissingWitness { hash: "0x123".to_string() };
        let err11 = VerificationError::MissingWitness { hash: "0x123".to_string() };
        assert_eq!(err10, err11);
    }

    #[test]
    fn test_verification_error_debug_all_variants() {
        let err1 = VerificationError::verification_failed("msg");
        assert!(format!("{:?}", err1).contains("VerificationFailed"));

        let err2 = VerificationError::gas_limit_exceeded(200, 100);
        assert!(format!("{:?}", err2).contains("GasLimitExceeded"));

        let err3 = VerificationError::invalid_script("script error");
        assert!(format!("{:?}", err3).contains("InvalidScript"));

        let err4 = VerificationError::invalid_signature("sig error");
        assert!(format!("{:?}", err4).contains("InvalidSignature"));

        let err5 = VerificationError::missing_witness(&UInt160::zero());
        assert!(format!("{:?}", err5).contains("MissingWitness"));
    }

    #[test]
    fn test_mock_verifier_error_result() {
        // Test verification that returns an error
        struct FailingVerifier;
        impl IVerificationContext for FailingVerifier {
            fn verify_witness(&self, _hash: &UInt160, _witness: &dyn IWitness) -> VerificationResult<bool> {
                Err(VerificationError::gas_limit_exceeded(500, 100))
            }
            fn get_gas_consumed(&self) -> i64 { 500 }
            fn get_max_gas(&self) -> i64 { 100 }
        }

        let verifier = FailingVerifier;
        let hash = UInt160::zero();
        let witness = MockWitness::new(vec![], vec![]);
        let result = verifier.verify_witness(&hash, &witness);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VerificationError::GasLimitExceeded { .. }));
    }

    #[test]
    fn test_snapshot_with_multiple_blocks() {
        let snapshot = MockSnapshot::new(1000)
            .with_storage(vec![1, 2], vec![10, 20])
            .with_storage(vec![3, 4], vec![30, 40]);

        assert_eq!(snapshot.get_storage(&[1, 2]), Some(vec![10, 20]));
        assert_eq!(snapshot.get_storage(&[3, 4]), Some(vec![30, 40]));
        assert!(snapshot.get_block_hash(500).is_some());
        assert!(snapshot.get_block_hash(1000).is_some());
        assert!(snapshot.get_block_hash(1001).is_none());
    }

    #[test]
    fn test_verification_result_type_alias() {
        fn returns_verification_result() -> VerificationResult<i32> {
            Ok(42)
        }

        fn returns_verification_error() -> VerificationResult<i32> {
            Err(VerificationError::verification_failed("test"))
        }

        assert_eq!(returns_verification_result().unwrap(), 42);
        assert!(returns_verification_error().is_err());
    }

    #[test]
    fn test_gas_edge_cases() {
        // Zero gas
        let zero_gas = MockVerifier::new(0, true).with_consumed(0);
        assert!(zero_gas.should_abort()); // 0 >= 0 is true
        assert_eq!(zero_gas.get_remaining_gas(), 0);

        // Large gas values
        let large_gas = MockVerifier::new(i64::MAX, true).with_consumed(i64::MAX / 2);
        assert!(!large_gas.should_abort());
        assert_eq!(large_gas.get_remaining_gas(), i64::MAX - i64::MAX / 2);
    }
}
