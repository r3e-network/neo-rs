use crate::{ecc256, hash_algorithm::HashAlgorithm};
use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::SecretKey;

use super::{
    signature::SignatureBytes,
    traits::{Secp256r1Sign, Secp256r1Verify},
    SignError, VerifyError,
};

/// Sign a message using the requested hash algorithm.
pub fn sign_with_algorithm(
    private: &ecc256::PrivateKey,
    message: &[u8],
    algorithm: HashAlgorithm,
) -> Result<SignatureBytes, SignError> {
    let secret = SecretKey::from_slice(private.as_be_bytes()).map_err(|_| SignError::InvalidKey)?;
    let signing_key: SigningKey = secret.into();
    let digest = algorithm.digest(message);
    let signature: Signature = signing_key
        .sign_prehash(&digest)
        .map_err(|_| SignError::SigningFailed)?;
    let normalized = signature.normalize_s().unwrap_or(signature);
    Ok(SignatureBytes::from(normalized))
}

/// Verify a signature using the requested hash algorithm.
pub fn verify_with_algorithm(
    public: &ecc256::PublicKey,
    message: &[u8],
    signature: &SignatureBytes,
    algorithm: HashAlgorithm,
) -> Result<(), VerifyError> {
    let signature = Signature::try_from(*signature)?;
    let signature = signature.normalize_s().unwrap_or(signature);
    let verifying = VerifyingKey::from_sec1_bytes(&public.to_uncompressed())
        .map_err(|_| VerifyError::InvalidKey)?;
    let digest = algorithm.digest(message);
    verifying
        .verify_prehash(&digest, &signature)
        .map_err(|_| VerifyError::InvalidSignature)
}

impl Secp256r1Sign for ecc256::PrivateKey {
    #[inline]
    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<SignatureBytes, SignError> {
        sign_with_algorithm(self, message.as_ref(), HashAlgorithm::Sha256)
    }
}

impl Secp256r1Verify for ecc256::PublicKey {
    #[inline]
    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        signature: &SignatureBytes,
    ) -> Result<(), VerifyError> {
        verify_with_algorithm(self, message.as_ref(), signature, HashAlgorithm::Sha256)
    }
}
