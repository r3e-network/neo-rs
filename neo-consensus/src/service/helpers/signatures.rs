use super::super::ConsensusService;
use neo_primitives::UInt256;
use tracing::{debug, warn};

impl ConsensusService {
    /// Signs data with the private key using secp256r1 ECDSA
    pub(in crate::service) fn sign(&self, data: &[u8]) -> Vec<u8> {
        use neo_crypto::Secp256r1Crypto;

        // Sign with secp256r1 if we have a valid private key
        if self.private_key.len() == 32 {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&self.private_key);

            match Secp256r1Crypto::sign(data, &key_bytes) {
                Ok(sig) => sig.to_vec(),
                Err(e) => {
                    warn!(error = %e, "ECDSA signing failed");
                    Vec::new()
                }
            }
        } else {
            // Fallback for testing without valid key
            Vec::new()
        }
    }

    /// Signs a block hash
    pub(in crate::service) fn sign_block_hash(&self, hash: &UInt256) -> Vec<u8> {
        let mut sign_data = Vec::with_capacity(4 + 32);
        sign_data.extend_from_slice(&self.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.as_bytes());
        self.sign(&sign_data)
    }

    /// Verifies a signature against a public key
    pub(in crate::service) fn verify_signature(
        &self,
        data: &[u8],
        signature: &[u8],
        validator_index: u8,
    ) -> bool {
        use neo_crypto::Secp256r1Crypto;

        // Get the validator's public key
        let validator = match self.context.validators.get(validator_index as usize) {
            Some(v) => v,
            None => return false,
        };

        // Verify signature length (64 bytes for secp256r1)
        if signature.len() != 64 {
            debug!(
                expected = 64,
                got = signature.len(),
                "Invalid signature length"
            );
            return false;
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(signature);

        // Get public key bytes
        let pub_key_bytes = validator.public_key.encoded();

        match Secp256r1Crypto::verify(data, &sig_bytes, &pub_key_bytes) {
            Ok(valid) => valid,
            Err(e) => {
                debug!(error = %e, "Signature verification failed");
                false
            }
        }
    }
}
