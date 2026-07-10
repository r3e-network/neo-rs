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
//! ## Example
//!
//! ```rust,ignore
//! use neo_consensus::{ConsensusResult, ConsensusSigner};
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
//!     fn sign(
//!         &self,
//!         data: &[u8],
//!         script_hash: &UInt160,
//!     ) -> impl Future<Output = ConsensusResult<Vec<u8>>> + Send {
//!         async move {
//!         // Sign the data (HSM/network signers may await here)
//!             Ok(vec![0u8; 64])
//!         }
//!     }
//! }
//! ```
//!
//! Production implementations live in `neo-hsm` (PKCS#11, Azure Key Vault, GCP
//! KMS), plus the in-process software signer.

use crate::{ConsensusError, ConsensusResult};
use neo_primitives::UInt160;
use std::sync::Arc;

/// Signing interface for consensus messages.
///
/// Implementors provide the cryptographic signing capability required
/// for validators to sign consensus messages.
///
/// The `sign` method is `async` so that HSM/network-backed signers (Azure Key
/// Vault, Nitro Enclave, PKCS#11) can perform blocking round-trips without
/// stalling the tokio worker thread. Software (local ECDSA) signers simply
/// perform sync work inside the async function; signing is called roughly once
/// per block. The returned future remains concrete, so even external signing
/// does not allocate a boxed trait-object future.
pub trait ConsensusSigner: Send + Sync {
    /// Returns true if the signer can sign for the given script hash.
    fn can_sign(&self, script_hash: &UInt160) -> bool;

    /// Signs the provided data for the given script hash.
    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
    ) -> impl std::future::Future<Output = ConsensusResult<Vec<u8>>> + Send;
}

/// Concrete default for consensus services that use the software private key.
///
/// The service stores the signer as `Option<Arc<S>>`; this type parameter keeps
/// the default software-signing path fully concrete without carrying a trait
/// object when no external signer is configured.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoConsensusSigner;

impl ConsensusSigner for NoConsensusSigner {
    fn can_sign(&self, _script_hash: &UInt160) -> bool {
        false
    }

    fn sign(
        &self,
        _data: &[u8],
        _script_hash: &UInt160,
    ) -> impl std::future::Future<Output = ConsensusResult<Vec<u8>>> + Send {
        std::future::ready(Err(ConsensusError::state_error(
            "external consensus signer not configured",
        )))
    }
}

impl<S> ConsensusSigner for Arc<S>
where
    S: ConsensusSigner,
{
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.as_ref().can_sign(script_hash)
    }

    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
    ) -> impl std::future::Future<Output = ConsensusResult<Vec<u8>>> + Send {
        self.as_ref().sign(data, script_hash)
    }
}
