//! Consensus signer trait for message signing.
//!
//! This module provides the `ConsensusSigner` trait, which abstracts the
//! signing functionality required for consensus message authentication.
//!
//! ## Overview
//!
//! The `ConsensusSigner` trait allows the consensus service to sign messages
//! using the validator's private key without directly accessing key material.
//!
//! ## Implementations
//!
//! The trait is automatically implemented for:
//! - `Arc<dyn ConsensusSigner>`
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_consensus::ConsensusSigner;
//! use neo_primitives::UInt160;
//!
//! struct MySigner;
//!
//! impl ConsensusSigner for MySigner {
//!     fn can_sign(&self, script_hash: &UInt160) -> bool {
//!         // Check if we have the key for this script hash
//!         true
//!     }
//!
//!     fn sign(&self, data: &[u8], script_hash: &UInt160) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
//!         // Sign the data
//!         Ok(vec![0u8; 64])
//!     }
//! }
//! ```

use crate::ConsensusResult;
use async_trait::async_trait;
use neo_primitives::UInt160;

/// Signing interface for consensus messages.
///
/// Implementors provide the cryptographic signing capability required
/// for validators to sign consensus messages.
///
/// The `sign` method is `async` so that HSM/network-backed signers (Azure Key
/// Vault, Nitro Enclave, PKCS#11) can perform blocking round-trips without
/// stalling the tokio worker thread. Software (local ECDSA) signers simply
/// perform sync work inside the async fn — the allocation overhead of the
/// `async_trait` `Pin<Box<dyn Future>>` is negligible (signing is called ~once
/// per block).
#[async_trait]
pub trait ConsensusSigner: Send + Sync {
    /// Returns true if the signer can sign for the given script hash.
    fn can_sign(&self, script_hash: &UInt160) -> bool;

    /// Signs the provided data for the given script hash.
    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> ConsensusResult<Vec<u8>>;
}

#[async_trait]
impl ConsensusSigner for std::sync::Arc<dyn ConsensusSigner> {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.as_ref().can_sign(script_hash)
    }

    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> ConsensusResult<Vec<u8>> {
        self.as_ref().sign(data, script_hash).await
    }
}
