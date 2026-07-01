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
        let mut ikm = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(ikm.as_mut());

        let secret_key = SecretKey::key_gen(ikm.as_ref(), &[])
            .expect("32-byte random IKM should generate a valid BLS secret key");
        let mut private_key = Zeroizing::new(secret_key.to_bytes());
        private_key.as_mut().reverse();
        private_key
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
#[path = "../tests/curves/bls12381.rs"]
mod tests;
