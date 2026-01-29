use super::super::ConsensusService;
use crate::{ConsensusError, ConsensusResult};
use neo_primitives::{UInt160, UInt256};
use neo_vm::{op_code::OpCode, script_builder::ScriptBuilder};
use tracing::{debug, warn};

pub(in crate::service) fn invocation_script_from_signature(signature: &[u8]) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push(signature);
    builder.to_array()
}

/// Extracts signature from invocation script.
/// 
/// Returns `Option<&[u8]>` to avoid unnecessary allocation.
/// The signature slice is a reference into the input invocation script,
/// valid for the lifetime of the input.
pub(in crate::service) fn signature_from_invocation_script(invocation: &[u8]) -> Option<&[u8]> {
    if invocation.len() != 66 {
        return None;
    }
    if invocation[0] != OpCode::PUSHDATA1 as u8 || invocation[1] != 0x40 {
        return None;
    }
    // Return a slice instead of allocating a new Vec.
    // The caller can call .to_vec() if they need an owned copy.
    Some(&invocation[2..66])
}

impl ConsensusService {
    fn my_script_hash(&self) -> ConsensusResult<UInt160> {
        let my_index = self.my_index()?;
        self.context
            .validators
            .get(my_index as usize)
            .map(|validator| validator.script_hash)
            .ok_or(ConsensusError::InvalidValidatorIndex(my_index))
    }

    /// Signs data with the private key using secp256r1 ECDSA
    pub(in crate::service) fn sign(&self, data: &[u8]) -> ConsensusResult<Vec<u8>> {
        if let Some(signer) = &self.signer {
            let script_hash = self.my_script_hash()?;
            let signature = signer.sign(data, &script_hash)?;
            if signature.len() != 64 {
                return Err(ConsensusError::InvalidSignatureLength {
                    expected: 64,
                    got: signature.len(),
                });
            }
            return Ok(signature);
        }

        use neo_crypto::Secp256r1Crypto;

        if self.private_key.len() != 32 {
            return Err(ConsensusError::state_error(
                "Consensus signing key not available",
            ));
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&self.private_key);

        Secp256r1Crypto::sign(data, &key_bytes)
            .map(|sig| sig.to_vec())
            .map_err(|e| {
                warn!(error = %e, "ECDSA signing failed");
                ConsensusError::state_error(format!("ECDSA signing failed: {e}"))
            })
    }

    /// Signs a block hash
    pub(in crate::service) fn sign_block_hash(&self, hash: &UInt256) -> ConsensusResult<Vec<u8>> {
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
