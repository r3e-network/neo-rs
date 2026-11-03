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
//! - `Box<dyn ConsensusSigner>`
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

use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::{UInt160, UInt256};

/// Trusted protocol metadata bound to one consensus signing operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusSignContext {
    /// Block height of the active round.
    pub block_index: u32,
    /// View number of the active round.
    pub view_number: u8,
    /// Consensus message type, when signing an envelope.
    pub message_type: ConsensusMessageType,
    /// Locally verified proposal digest, when available.
    pub proposal_digest: Option<UInt256>,
    /// True when signing the verified proposal block hash for a Commit.
    pub commit_block_hash: bool,
}

/// Signing interface for consensus messages.
///
/// Implementors provide the cryptographic signing capability required
/// for validators to sign consensus messages.
pub trait ConsensusSigner: Send + Sync {
    /// Returns true if the signer can sign for the given script hash.
    fn can_sign(&self, script_hash: &UInt160) -> bool;

    /// Signs data with trusted consensus round metadata.
    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
        context: &ConsensusSignContext,
    ) -> ConsensusResult<Vec<u8>>;
}

impl ConsensusSigner for Box<dyn ConsensusSigner> {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.as_ref().can_sign(script_hash)
    }

    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
        context: &ConsensusSignContext,
    ) -> ConsensusResult<Vec<u8>> {
        self.as_ref().sign(data, script_hash, context)
    }
}

impl ConsensusSigner for std::sync::Arc<dyn ConsensusSigner> {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.as_ref().can_sign(script_hash)
    }

    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
        context: &ConsensusSignContext,
    ) -> ConsensusResult<Vec<u8>> {
        self.as_ref().sign(data, script_hash, context)
    }
}
