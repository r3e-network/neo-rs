use neo_consensus::{ConsensusError, ConsensusResult, ConsensusSignContext, ConsensusSigner};
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_primitives::UInt160;

pub struct TestConsensusSigner {
    private_key: [u8; 32],
}

impl TestConsensusSigner {
    pub const fn new(private_key: [u8; 32]) -> Self {
        Self { private_key }
    }
}

impl ConsensusSigner for TestConsensusSigner {
    fn can_sign(&self, _script_hash: &UInt160) -> bool {
        true
    }

    fn sign(
        &self,
        data: &[u8],
        _script_hash: &UInt160,
        _context: &ConsensusSignContext,
    ) -> ConsensusResult<Vec<u8>> {
        Secp256r1Crypto::sign_prehash(&Crypto::sha256(data), &self.private_key)
            .map(|signature| signature.to_vec())
            .map_err(|error| {
                ConsensusError::state_error(format!("test consensus signing failed: {error}"))
            })
    }
}
