use alloc::fmt;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToHex};

use crate::{ecc256, hash_algorithm::HashAlgorithm};
use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::SecretKey;

pub const SIGNATURE_SIZE: usize = 64;

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct SignatureBytes(pub [u8; SIGNATURE_SIZE]);

impl SignatureBytes {
    #[inline]
    pub fn as_ref(&self) -> &[u8; SIGNATURE_SIZE] {
        &self.0
    }
}

impl fmt::Debug for SignatureBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SignatureBytes")
            .field(&self.0.to_hex_lower())
            .finish()
    }
}

impl NeoEncode for SignatureBytes {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for SignatureBytes {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; SIGNATURE_SIZE];
        reader.read_into(&mut buf)?;
        Ok(SignatureBytes(buf))
    }
}

impl TryFrom<SignatureBytes> for Signature {
    type Error = VerifyError;

    #[inline]
    fn try_from(value: SignatureBytes) -> Result<Self, Self::Error> {
        Signature::try_from(value.0.as_slice()).map_err(|_| VerifyError::InvalidSignature)
    }
}

impl From<Signature> for SignatureBytes {
    #[inline]
    fn from(value: Signature) -> Self {
        SignatureBytes(value.to_bytes().into())
    }
}

pub trait Secp256r1Sign {
    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<SignatureBytes, SignError>;
}

pub trait Secp256r1Verify {
    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        signature: &SignatureBytes,
    ) -> Result<(), VerifyError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SignError {
    #[error("ecdsa: invalid private key")]
    InvalidKey,

    #[error("ecdsa: signing failed")]
    SigningFailed,
}

impl Secp256r1Sign for ecc256::PrivateKey {
    #[inline]
    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<SignatureBytes, SignError> {
        sign_with_algorithm(self, message.as_ref(), HashAlgorithm::Sha256)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VerifyError {
    #[error("ecdsa: invalid public key")]
    InvalidKey,

    #[error("ecdsa: invalid signature")]
    InvalidSignature,
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn sign_and_verify() {
        let sk_bytes = hex!("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b848a7d84b8b620");
        let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
        let secret = SecretKey::from_slice(&sk_bytes).unwrap();
        let signing = SigningKey::from(secret.clone());
        let public_encoded = signing.verifying_key().to_encoded_point(true);
        let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();
        let message = b"neo-n3";

        let signature = private.secp256r1_sign(message).unwrap();
        public.secp256r1_verify(message, &signature).unwrap();
    }

    #[test]
    fn sign_with_keccak_roundtrip() {
        let sk_bytes = hex!("c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75");
        let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
        let secret = SecretKey::from_slice(&sk_bytes).unwrap();
        let public_encoded = SigningKey::from(secret)
            .verifying_key()
            .to_encoded_point(true);
        let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();

        let signature =
            sign_with_algorithm(&private, b"keccak payload", HashAlgorithm::Keccak256).unwrap();
        verify_with_algorithm(
            &public,
            b"keccak payload",
            &signature,
            HashAlgorithm::Keccak256,
        )
        .unwrap();
    }

    #[test]
    fn sign_with_sha512_roundtrip() {
        let sk_bytes = hex!("c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75");
        let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
        let secret = SecretKey::from_slice(&sk_bytes).unwrap();
        let public_encoded = SigningKey::from(secret)
            .verifying_key()
            .to_encoded_point(true);
        let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();

        let signature =
            sign_with_algorithm(&private, b"sha512 payload", HashAlgorithm::Sha512).unwrap();
        verify_with_algorithm(
            &public,
            b"sha512 payload",
            &signature,
            HashAlgorithm::Sha512,
        )
        .unwrap();
    }

    #[test]
    fn rfc6979_vector_matches() {
        let sk = hex!("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721");
        let private = ecc256::PrivateKey::from_slice(&sk).unwrap();
        let secret = SecretKey::from_slice(&sk).unwrap();
        let signing = SigningKey::from(secret.clone());
        let public_encoded = signing.verifying_key().to_encoded_point(true);
        let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();

        let sample_sig = private.secp256r1_sign(b"sample").unwrap();
        // Low-S normalization is enforced; expected value mirrors the C# low-S output.
        assert_eq!(
            sample_sig.0,
            hex!(
                "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716\
                 0834e36ad29a83bf2bc9385e491d6099c8fdf9d1ed67aa7ea5f51f93782857a9"
            )
        );
        public.secp256r1_verify(b"sample", &sample_sig).unwrap();
        // High-S signatures from legacy vectors are still accepted.
        let high_s_sample = SignatureBytes(hex!(
            "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716\
             f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8"
        ));
        verify_with_algorithm(&public, b"sample", &high_s_sample, HashAlgorithm::Sha256).unwrap();

        let test_sig = private.secp256r1_sign(b"test").unwrap();
        assert_eq!(
            test_sig.0,
            hex!(
                "f1abb023518351cd71d881567b1ea663ed3efcf6c5132b354f28d3b0b7d38367\
                 019f4113742a2b14bd25926b49c649155f267e60d3814b4c0cc84250e46f0083"
            )
        );
        public.secp256r1_verify(b"test", &test_sig).unwrap();
        let high_s_test = SignatureBytes(hex!(
            "f1abb023518351cd71d881567b1ea663ed3efcf6c5132b354f28d3b0b7d38367\
             019f4113742a2b14bd25926b49c649155f267e60d3814b4c0cc84250e46f0083"
        ));
        verify_with_algorithm(&public, b"test", &high_s_test, HashAlgorithm::Sha256).unwrap();
    }
}
