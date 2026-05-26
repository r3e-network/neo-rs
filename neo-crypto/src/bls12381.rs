//! BLS12-381 signature helpers for Neo.
//!
//! This module wraps the widely used `blst` crate while keeping Neo's exact
//! domain separation tag and encoding choices isolated from general crypto
//! utilities.

use crate::error::{CryptoError, CryptoResult};
use blst::BLST_ERROR;
use blst::min_sig::{AggregatePublicKey, AggregateSignature, PublicKey, SecretKey, Signature};
use rand::{RngCore, rngs::OsRng};
use zeroize::Zeroizing;

/// BLS12-381 operations using the `blst` crate.
///
/// Neo uses the "minimal-signature-size" scheme:
/// - Private key: scalar (32 bytes)
/// - Public key: G2 point (96 bytes compressed)
/// - Signature: G1 point (48 bytes compressed)
pub struct Bls12381Crypto;

/// Domain Separation Tag for Neo BLS12-381 signatures.
///
/// This must match the C# implementation exactly for cross-compatibility.
const NEO_BLS_DST: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";

impl Bls12381Crypto {
    fn secret_key_from_neo_bytes(private_key: &[u8; 32]) -> CryptoResult<SecretKey> {
        if private_key.iter().all(|b| *b == 0) {
            return Err(CryptoError::invalid_key(
                "Invalid private key: scalar cannot be zero",
            ));
        }

        // Preserve the previous Neo helper behavior: the 32-byte scalar is
        // interpreted as little-endian, while `blst::min_sig` expects big-endian.
        let mut big_endian = *private_key;
        big_endian.reverse();
        SecretKey::from_bytes(&big_endian)
            .map_err(|_| CryptoError::invalid_key("Invalid private key: scalar not in Fr field"))
    }

    fn parse_signature(
        signature: &[u8; 48],
        encoding_message: &'static str,
        subgroup_message: &'static str,
    ) -> CryptoResult<Signature> {
        let signature = Signature::from_bytes(signature)
            .map_err(|_| CryptoError::invalid_signature(encoding_message))?;
        signature
            .validate(true)
            .map_err(|_| CryptoError::invalid_signature(subgroup_message))?;
        Ok(signature)
    }

    fn parse_public_key(
        public_key: &[u8; 96],
        encoding_message: &'static str,
        subgroup_message: &'static str,
    ) -> CryptoResult<PublicKey> {
        let public_key = PublicKey::from_bytes(public_key)
            .map_err(|_| CryptoError::invalid_key(encoding_message))?;
        public_key
            .validate()
            .map_err(|_| CryptoError::invalid_key(subgroup_message))?;
        Ok(public_key)
    }

    /// Generates a new random private key using cryptographically secure RNG.
    #[must_use]
    pub fn generate_private_key() -> Zeroizing<[u8; 32]> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(bytes.as_mut());
        bytes
    }

    /// Derives a public key from a private key.
    ///
    /// Returns a 96-byte compressed G2 point.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 96]> {
        Ok(Self::secret_key_from_neo_bytes(private_key)?
            .sk_to_pk()
            .to_bytes())
    }

    /// Signs a message with BLS12-381.
    ///
    /// Returns a 48-byte compressed G1 signature.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 48]> {
        Ok(Self::secret_key_from_neo_bytes(private_key)?
            .sign(message, NEO_BLS_DST, &[])
            .to_bytes())
    }

    /// Verifies a BLS12-381 signature.
    ///
    /// Signature is a 48-byte compressed G1 point; public key is a 96-byte
    /// compressed G2 point.
    pub fn verify(
        message: &[u8],
        signature: &[u8; 48],
        public_key: &[u8; 96],
    ) -> CryptoResult<bool> {
        let signature = Self::parse_signature(
            signature,
            "Invalid signature encoding",
            "Signature not in G1 subgroup",
        )?;
        let public_key = Self::parse_public_key(
            public_key,
            "Invalid public key encoding",
            "Public key not in G2 subgroup",
        )?;

        Ok(
            signature.verify(true, message, NEO_BLS_DST, &[], &public_key, true)
                == BLST_ERROR::BLST_SUCCESS,
        )
    }

    /// Aggregates multiple BLS signatures into one.
    ///
    /// Used for dBFT consensus where multiple validators sign.
    pub fn aggregate_signatures(signatures: &[[u8; 48]]) -> CryptoResult<[u8; 48]> {
        if signatures.is_empty() {
            return Err(CryptoError::invalid_argument("No signatures to aggregate"));
        }

        if signatures.len() == 1 {
            return Ok(signatures[0]);
        }

        let signatures = signatures
            .iter()
            .enumerate()
            .map(|(index, signature)| {
                if index == 0 {
                    Self::parse_signature(
                        signature,
                        "Invalid first signature",
                        "First signature not in G1 subgroup",
                    )
                } else {
                    Self::parse_signature(
                        signature,
                        "Invalid signature in aggregation",
                        "Signature not in G1 subgroup",
                    )
                }
            })
            .collect::<CryptoResult<Vec<_>>>()?;
        let signature_refs: Vec<&Signature> = signatures.iter().collect();
        let aggregate = AggregateSignature::aggregate(&signature_refs, false)
            .map_err(|_| CryptoError::invalid_signature("Invalid signature in aggregation"))?;

        Ok(aggregate.to_signature().to_bytes())
    }

    /// Verifies an aggregated signature against multiple public keys.
    pub fn verify_aggregated(
        message: &[u8],
        aggregated_signature: &[u8; 48],
        public_keys: &[[u8; 96]],
    ) -> CryptoResult<bool> {
        if public_keys.is_empty() {
            return Err(CryptoError::invalid_argument("No public keys provided"));
        }

        let public_keys = public_keys
            .iter()
            .enumerate()
            .map(|(index, public_key)| {
                if index == 0 {
                    Self::parse_public_key(
                        public_key,
                        "Invalid first public key",
                        "First public key not in G2 subgroup",
                    )
                } else {
                    Self::parse_public_key(
                        public_key,
                        "Invalid public key in aggregation",
                        "Public key not in G2 subgroup",
                    )
                }
            })
            .collect::<CryptoResult<Vec<_>>>()?;
        let public_key_refs: Vec<&PublicKey> = public_keys.iter().collect();
        let aggregate = AggregatePublicKey::aggregate(&public_key_refs, false)
            .map_err(|_| CryptoError::invalid_key("Invalid public key in aggregation"))?;
        let aggregate = aggregate.to_public_key().to_bytes();

        Self::verify(message, aggregated_signature, &aggregate)
    }
}

#[cfg(test)]
mod tests {
    use super::Bls12381Crypto;

    const PRIVATE_KEY: [u8; 32] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ];
    const SECOND_PRIVATE_KEY: [u8; 32] = [
        33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55,
        56, 57, 58, 59, 60, 61, 62, 63, 64,
    ];
    const MESSAGE: &[u8] = b"neo-rs bls compatibility";

    fn decode_array<const N: usize>(hex: &str) -> [u8; N] {
        hex::decode(hex).unwrap().try_into().unwrap()
    }

    #[test]
    fn bls12381_compatibility_vector() {
        let public_key = Bls12381Crypto::derive_public_key(&PRIVATE_KEY).unwrap();
        let signature = Bls12381Crypto::sign(MESSAGE, &PRIVATE_KEY).unwrap();
        let second_public_key = Bls12381Crypto::derive_public_key(&SECOND_PRIVATE_KEY).unwrap();
        let second_signature = Bls12381Crypto::sign(MESSAGE, &SECOND_PRIVATE_KEY).unwrap();
        let aggregated =
            Bls12381Crypto::aggregate_signatures(&[signature, second_signature]).unwrap();

        assert_eq!(
            public_key,
            decode_array(
                "954087aafacc1046c0f0ad35d5b60163cb4771573f995afdd6f26cbeec117caaef1a94eed091f06cfbb04cd44819a4b419629b06ca5701c0c4a53b370db40a5adf174a8627ff0fe765eddfb0e4bb5debddcb7a268afec33c833f7f9466fded0c"
            )
        );
        assert_eq!(
            signature,
            decode_array(
                "8a0843ce5187848624a00a86ce657782def22e8ed59046a1723e0db715a018314d4a5982ac9abb5b8cbbd270f448ba0b"
            )
        );
        assert_eq!(
            aggregated,
            decode_array(
                "abf312ecc4e8c7d1c5acc41147a028cfb4d225abdb09ab0fe5d8bf98f1290328633824cb50768bbaebd1e064c9ad8c66"
            )
        );
        assert_eq!(
            Bls12381Crypto::aggregate_signatures(&[signature]).unwrap(),
            signature
        );
        assert!(Bls12381Crypto::verify(MESSAGE, &signature, &public_key).unwrap());
        assert!(
            Bls12381Crypto::verify_aggregated(
                MESSAGE,
                &aggregated,
                &[public_key, second_public_key],
            )
            .unwrap()
        );
    }
}
