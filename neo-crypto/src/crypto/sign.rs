use crate::{ecc256::PrivateKey, ecdsa, hash_algorithm::HashAlgorithm, secp256k1};

use super::{Curve, SignError};

/// Sign the given message using the selected curve and hashing algorithm.
pub fn sign(
    message: &[u8],
    private_key: &[u8],
    curve: Curve,
    hash_algorithm: HashAlgorithm,
) -> Result<[u8; 64], SignError> {
    match curve {
        Curve::Secp256r1 => {
            let private =
                PrivateKey::from_slice(private_key).map_err(|_| SignError::InvalidPrivateKey)?;
            let signature = ecdsa::sign_with_algorithm(&private, message, hash_algorithm)
                .map_err(SignError::Signing)?;
            Ok(signature.0)
        }
        Curve::Secp256k1 => {
            secp256k1::sign(message, private_key, hash_algorithm).map_err(SignError::Secp256k1)
        }
    }
}
