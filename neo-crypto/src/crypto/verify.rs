use crate::{
    ecc256::PublicKey,
    ecdsa::{self, SignatureBytes},
    hash_algorithm::HashAlgorithm,
    secp256k1,
};

use super::{Curve, VerifyError};

/// Verify the signature for the given message.
pub fn verify(
    message: &[u8],
    signature: &[u8],
    public_key: &[u8],
    curve: Curve,
    hash_algorithm: HashAlgorithm,
) -> Result<(), VerifyError> {
    match curve {
        Curve::Secp256r1 => {
            let public = PublicKey::from_sec1_bytes(public_key)
                .map_err(|_| VerifyError::InvalidPublicKey)?;
            let array: [u8; 64] = signature
                .try_into()
                .map_err(|_| VerifyError::InvalidSignature)?;
            let wrapped = SignatureBytes(array);
            ecdsa::verify_with_algorithm(&public, message, &wrapped, hash_algorithm)
                .map_err(VerifyError::Signature)
        }
        Curve::Secp256k1 => secp256k1::verify(message, signature, public_key, hash_algorithm)
            .map_err(VerifyError::Secp256k1),
    }
}
