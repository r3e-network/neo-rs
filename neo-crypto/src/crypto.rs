use crate::{
    ecc256::{PrivateKey, PublicKey},
    ecdsa::{sign_with_algorithm, verify_with_algorithm, SignatureBytes},
    hash_algorithm::HashAlgorithm,
    secp256k1,
};

/// Supported elliptic curves for signing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Curve {
    Secp256r1,
    Secp256k1,
}

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
            let signature = sign_with_algorithm(&private, message, hash_algorithm)
                .map_err(SignError::Signing)?;
            Ok(signature.0)
        }
        Curve::Secp256k1 => {
            secp256k1::sign(message, private_key, hash_algorithm).map_err(SignError::Secp256k1)
        }
    }
}

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
            verify_with_algorithm(&public, message, &wrapped, hash_algorithm)
                .map_err(VerifyError::Signature)
        }
        Curve::Secp256k1 => secp256k1::verify(message, signature, public_key, hash_algorithm)
            .map_err(VerifyError::Secp256k1),
    }
}

/// Convenience helper to compute Hash160 = RIPEMD160(SHA256(data)).
#[inline]
pub fn hash160(message: &[u8]) -> [u8; 20] {
    neo_base::hash::hash160(message)
}

/// Convenience helper to compute double SHA-256.
#[inline]
pub fn hash256(message: &[u8]) -> [u8; 32] {
    neo_base::hash::double_sha256(message)
}

/// Errors returned by [`sign`].
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("invalid private key material")]
    InvalidPrivateKey,
    #[error(transparent)]
    Signing(#[from] crate::ecdsa::SignError),
    #[error(transparent)]
    Secp256k1(#[from] secp256k1::Secp256k1Error),
}

/// Errors returned by [`verify`].
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("invalid public key encoding")]
    InvalidPublicKey,
    #[error("invalid signature payload")]
    InvalidSignature,
    #[error(transparent)]
    Signature(#[from] crate::ecdsa::VerifyError),
    #[error(transparent)]
    Secp256k1(#[from] secp256k1::Secp256k1Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecc256::Keypair;
    use ::secp256k1::{PublicKey as K1PublicKey, Secp256k1 as K1Ctx, SecretKey as K1SecretKey};
    use hex_literal::hex;

    #[test]
    fn sign_verify_p256_roundtrip() {
        let private = hex!("c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75");
        let p256 = PrivateKey::from_slice(&private).unwrap();
        let keypair = Keypair::from_private(p256.clone()).unwrap();
        let public = keypair.public_key.to_compressed();

        let sig = sign(b"neo", &private, Curve::Secp256r1, HashAlgorithm::Sha256).unwrap();
        verify(
            b"neo",
            &sig,
            &public,
            Curve::Secp256r1,
            HashAlgorithm::Sha256,
        )
        .unwrap();
    }

    #[test]
    fn sign_verify_k1_roundtrip() {
        let private = hex!("1b7f730fc3ac386a1ae1c2cbaabdd99e3bb85da7d5236f9b1a92bb0b742d30ca");
        let secp = K1Ctx::new();
        let secret = K1SecretKey::from_slice(&private).expect("valid secp256k1 private key");
        let public = K1PublicKey::from_secret_key(&secp, &secret).serialize();

        let sig = sign(b"neo", &private, Curve::Secp256k1, HashAlgorithm::Sha256).unwrap();
        verify(
            b"neo",
            &sig,
            &public,
            Curve::Secp256k1,
            HashAlgorithm::Sha256,
        )
        .unwrap();
    }
}
