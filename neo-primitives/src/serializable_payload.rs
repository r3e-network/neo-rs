//! Serializable payload trait for network messages.
//!
//! This trait provides the serialization and hashing interface for blockchain
//! payloads (Block, Header, Transaction) without requiring verification logic.
//! This enables the P2P networking layer to serialize/deserialize messages
//! without depending on the smart contract execution engine.

use crate::UInt256;

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
    /// Default implementation: `SHA256(hash_data())`. Neo N3 payload hashing is a
    /// single SHA-256 over the unsigned serialization (not Bitcoin's double hash).
    fn hash(&self) -> UInt256 {
        use sha2::{Digest, Sha256};
        UInt256::from_bytes(&Sha256::digest(self.hash_data())).unwrap_or_default()
    }

    /// Returns the number of witnesses attached to this payload.
    fn witness_count(&self) -> usize;

    /// Returns the invocation script for the witness at the given index.
    fn invocation_script(&self, index: usize) -> &[u8];

    /// Returns the verification script for the witness at the given index.
    fn verification_script(&self, index: usize) -> &[u8];
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    struct DummyPayload(Vec<u8>);

    impl SerializablePayload for DummyPayload {
        fn hash_data(&self) -> Vec<u8> {
            self.0.clone()
        }

        fn witness_count(&self) -> usize {
            0
        }

        fn invocation_script(&self, _index: usize) -> &[u8] {
            &[]
        }

        fn verification_script(&self, _index: usize) -> &[u8] {
            &[]
        }
    }

    #[test]
    fn default_hash_is_single_sha256_of_unsigned_data() {
        let payload = DummyPayload(b"neo-n3-payload".to_vec());
        let first = Sha256::digest(payload.hash_data());
        let second = Sha256::digest(first.as_slice());

        assert_eq!(payload.hash(), UInt256::from_bytes(&first).unwrap());
        assert_ne!(payload.hash(), UInt256::from_bytes(&second).unwrap());
    }
}
