//! Serializable payload trait for network messages.
//!
//! This trait provides the serialization and hashing interface for blockchain
//! payloads (Block, Header, Transaction) without requiring verification logic.
//! This enables the P2P networking layer to serialize/deserialize messages
//! without depending on the smart contract execution engine.

use crate::{UInt256, Witness};

/// Trait for blockchain payloads that can be serialized and hashed.
///
/// This is the "data-only" interface for payloads that flow over the wire.
/// Verification logic lives in a separate `Verifiable` trait in neo-core.
///
/// # Design Rationale
///
/// Separating serialization from verification allows:
/// - neo-p2p to handle message framing without depending on neo-core
/// - Verification to be deferred or performed asynchronously
/// - Testing serialization without needing a full VM
pub trait SerializablePayload: Send + Sync {
    /// Returns the serialized bytes used for hash computation.
    ///
    /// This is the unsigned serialization - witnesses are NOT included.
    fn hash_data(&self) -> Vec<u8>;

    /// Computes the hash of this payload.
    ///
    /// Default implementation: `SHA256(SHA256(hash_data()))`
    fn hash(&self) -> UInt256 {
        use sha2::{Digest, Sha256};
        let data = self.hash_data();
        let first = Sha256::digest(&data);
        let second = Sha256::digest(&first);
        UInt256::from_bytes(&second).unwrap_or_default()
    }

    /// Returns the number of witnesses attached to this payload.
    fn witness_count(&self) -> usize;

    /// Returns the invocation script for the witness at the given index.
    fn invocation_script(&self, index: usize) -> &[u8];

    /// Returns the verification script for the witness at the given index.
    fn verification_script(&self, index: usize) -> &[u8];
}
