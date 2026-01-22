use crate::ConsensusResult;
use neo_primitives::UInt160;

/// Signing interface for consensus messages.
pub trait ConsensusSigner: Send + Sync {
    /// Returns true if the signer can sign for the given script hash.
    fn can_sign(&self, script_hash: &UInt160) -> bool;

    /// Signs the provided data for the given script hash.
    fn sign(&self, data: &[u8], script_hash: &UInt160) -> ConsensusResult<Vec<u8>>;
}

impl ConsensusSigner for Box<dyn ConsensusSigner> {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.as_ref().can_sign(script_hash)
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> ConsensusResult<Vec<u8>> {
        self.as_ref().sign(data, script_hash)
    }
}

impl ConsensusSigner for std::sync::Arc<dyn ConsensusSigner> {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.as_ref().can_sign(script_hash)
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> ConsensusResult<Vec<u8>> {
        self.as_ref().sign(data, script_hash)
    }
}
