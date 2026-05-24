//! Cryptographic utilities for Neo blockchain.
//!
//! This module provides common cryptographic functions using external, well-tested crates,
//! implementing the cryptographic primitives required by Neo N3.
//!
//! # Supported Algorithms
//!
//! ## Hash Functions
//! - **SHA-256**: Primary hash for transaction/block IDs
//! - **SHA-512**: Used in key derivation
//! - **RIPEMD-160**: Script hash computation (Hash160 = RIPEMD160(SHA256(data)))
//! - **Keccak-256**: Ethereum compatibility
//! - **SHA3-256/SHA3-512**: SHA-3 family hashes
//! - **Blake2b/Blake2s**: Alternative hash functions
//!
//! ## Elliptic Curve Cryptography
//! - **secp256r1 (P-256/NIST)**: Primary curve for Neo N3 signatures
//! - **secp256k1**: Bitcoin/Ethereum compatibility
//! - **Ed25519**: `EdDSA` signatures
//!
//! # Key Types
//!
//! - [`NeoHash`]: Hash function implementations (hash160, hash256, sha256, etc.)
//! - [`Secp256r1Crypto`]: P-256 key generation, signing, verification
//! - [`Secp256k1Crypto`]: secp256k1 operations for compatibility
//! - [`Ed25519Crypto`]: `EdDSA` operations
//!
//! # Neo-Specific Functions
//!
//! - `hash160()`: RIPEMD160(SHA256(data)) - used for script hashes
//! - `hash256()`: SHA256(SHA256(data)) - used for transaction hashes
//! - `base58_check_encode/decode()`: Neo address encoding
//!
//! # Security Notes
//!
//! - All random number generation uses `OsRng` (cryptographically secure)
//! - Private keys are handled as `SecretKey` types with zeroization on drop
//! - Signature verification is constant-time to prevent timing attacks

pub use crate::bls12381::Bls12381Crypto;
pub use crate::constant_time::ConstantTime;
pub use crate::encoding::{Base58, Hex};
use crate::error::CryptoError;
pub use crate::murmur;
use crate::{Crypto, CryptoResult, ECCurve, ECPoint, HashAlgorithm};
use core::convert::TryFrom;
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};
use ed25519_dalek::{Signer as _, Verifier as _};
use p256::{
    ecdsa::{signature::hazmat::PrehashVerifier, Signature, SigningKey, VerifyingKey},
    PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};
use rand::{rngs::OsRng, RngCore};
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, PublicKey as Secp256k1PublicKey, Secp256k1, SecretKey as Secp256k1SecretKey,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Neo-specific hash functions.
///
/// This is a convenience wrapper around [`Crypto`] that provides the same
/// hash functions. For new code, prefer using [`Crypto`] directly.
///
/// NOTE: `NeoHash` delegates to `Crypto` to avoid code duplication.
/// The only additional function is `murmur128` which is Neo-specific.
pub struct NeoHash;

impl NeoHash {
    /// Computes SHA-256 hash of the input data
    #[inline]
    #[must_use]
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        Crypto::sha256(data)
    }

    /// Computes SHA-512 hash of the input data
    #[inline]
    #[must_use]
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        Crypto::sha512(data)
    }

    /// Computes Keccak-256 hash of the input data
    #[inline]
    #[must_use]
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        Crypto::keccak256(data)
    }

    /// Computes SHA3-256 hash of the input data
    #[inline]
    #[must_use]
    pub fn sha3_256(data: &[u8]) -> [u8; 32] {
        Crypto::sha3_256(data)
    }

    /// Computes SHA3-512 hash of the input data
    #[inline]
    #[must_use]
    pub fn sha3_512(data: &[u8]) -> [u8; 64] {
        Crypto::sha3_512(data)
    }

    /// Computes RIPEMD-160 hash of the input data
    #[inline]
    #[must_use]
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        Crypto::ripemd160(data)
    }

    /// Computes `BLAKE2b` hash of the input data
    #[inline]
    #[must_use]
    pub fn blake2b(data: &[u8]) -> [u8; 64] {
        Crypto::blake2b(data)
    }

    /// Computes BLAKE2b-512 hash of the input data with optional salt
    #[inline]
    pub fn blake2b_512(data: &[u8], salt: Option<&[u8]>) -> CryptoResult<[u8; 64]> {
        Crypto::blake2b_512(data, salt)
    }

    /// Computes BLAKE2b-256 hash of the input data with optional salt
    #[inline]
    pub fn blake2b_256(data: &[u8], salt: Option<&[u8]>) -> CryptoResult<[u8; 32]> {
        Crypto::blake2b_256(data, salt)
    }

    /// Computes BLAKE2s hash of the input data
    #[inline]
    #[must_use]
    pub fn blake2s(data: &[u8]) -> [u8; 32] {
        Crypto::blake2s(data)
    }

    /// Computes Hash160 (RIPEMD-160 of SHA-256) - commonly used for Neo addresses
    #[inline]
    #[must_use]
    pub fn hash160(data: &[u8]) -> [u8; 20] {
        Crypto::hash160(data)
    }

    /// Computes Hash256 (double SHA-256) - commonly used for Neo transaction and block hashes
    #[inline]
    #[must_use]
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        Crypto::hash256(data)
    }

    /// Computes Murmur128 hash (x64 variant) used by Neo runtime.
    /// This is Neo-specific and not available in [`Crypto`].
    #[must_use]
    pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
        murmur::murmur128(data, seed)
    }
}

/// ECDSA operations for secp256k1 (Bitcoin's curve)
pub struct Secp256k1Crypto;

/// Maximum attempts for key generation to prevent infinite loops in case of RNG failure
const MAX_KEY_GEN_ATTEMPTS: usize = 1000;

impl Secp256k1Crypto {
    /// Generates a new random private key
    ///
    /// # Errors
    /// Returns an error if a valid key cannot be generated after `MAX_KEY_GEN_ATTEMPTS` attempts.
    /// This should only occur if the system RNG is misbehaving.
    pub fn generate_private_key() -> CryptoResult<[u8; 32]> {
        let mut rng = OsRng;
        for _ in 0..MAX_KEY_GEN_ATTEMPTS {
            let mut candidate = Zeroizing::new([0u8; 32]);
            rng.fill_bytes(candidate.as_mut());
            if let Ok(secret_key) = Secp256k1SecretKey::from_slice(candidate.as_ref()) {
                return Ok(secret_key.secret_bytes());
            }
        }
        Err(CryptoError::key_generation_failed(format!(
            "Failed to generate valid secp256k1 private key after {MAX_KEY_GEN_ATTEMPTS} attempts"
        )))
    }

    /// Derives public key from private key
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 33]> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let public_key = Secp256k1PublicKey::from_secret_key(&secp, &secret_key);
        Ok(public_key.serialize())
    }

    /// Signs a message with secp256k1
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;

        let signature = secp.sign_ecdsa(&message, &secret_key);
        Ok(signature.serialize_compact())
    }

    /// Verifies a secp256k1 signature
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 33],
    ) -> CryptoResult<bool> {
        let secp = Secp256k1::verification_only();
        let public_key = Secp256k1PublicKey::from_slice(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;

        let signature = secp256k1::ecdsa::Signature::from_compact(signature)
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(secp.verify_ecdsa(&message, &signature, &public_key).is_ok())
    }

    /// Recovers a compressed secp256k1 public key from a message hash and signature.
    /// Accepts 65-byte (r||s||v) or 64-byte EIP-2098 compact signatures.
    pub fn recover_public_key(message_hash: &[u8], signature: &[u8]) -> CryptoResult<Vec<u8>> {
        if signature.len() != 65 && signature.len() != 64 {
            return Err(CryptoError::invalid_signature(
                "Signature must be 65 or 64 bytes",
            ));
        }
        if message_hash.len() != 32 {
            return Err(CryptoError::invalid_argument(
                "Message hash must be 32 bytes",
            ));
        }

        let msg = Message::from_digest_slice(message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message hash: {e}")))?;

        let (rec_id, sig_bytes) = if signature.len() == 65 {
            let rec = signature[64];
            let rec_id = if rec >= 27 { rec - 27 } else { rec };
            if rec_id > 3 {
                return Err(CryptoError::invalid_signature(
                    "Recovery id must be in range 0..3",
                ));
            }
            (rec_id, signature[..64].to_vec())
        } else {
            let mut sig = signature.to_vec();
            let y_parity = (sig[32] & 0x80) != 0;
            sig[32] &= 0x7f;
            let rec_id = u8::from(y_parity);
            (rec_id, sig)
        };

        let rec_id = RecoveryId::from_i32(i32::from(rec_id))
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid recovery id: {e}")))?;
        let recoverable = RecoverableSignature::from_compact(&sig_bytes, rec_id).map_err(|e| {
            CryptoError::invalid_signature(format!("Invalid recoverable signature: {e}"))
        })?;

        let secp = Secp256k1::new();
        let public_key = secp
            .recover_ecdsa(&msg, &recoverable)
            .map_err(|e| CryptoError::invalid_key(format!("Failed to recover public key: {e}")))?;

        Ok(public_key.serialize().to_vec())
    }
}

/// ECDSA operations for secp256r1 (P-256, Neo's primary curve)
pub struct Secp256r1Crypto;

impl Secp256r1Crypto {
    /// Generates a new random private key
    pub fn generate_private_key() -> [u8; 32] {
        let secret_key = P256SecretKey::random(&mut OsRng);
        let bytes = secret_key.to_bytes();
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes.as_slice());
        key
    }

    /// Derives public key from private key
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<Vec<u8>> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let verifying_key = VerifyingKey::from(&signing_key);
        Ok(verifying_key.to_encoded_point(true).as_bytes().to_vec())
    }

    /// Signs a message with secp256r1
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature: Signature = signing_key.sign(message);
        let bytes: [u8; 64] = signature.to_bytes().into();
        Ok(bytes)
    }

    /// Verifies a secp256r1 signature
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8]) -> CryptoResult<bool> {
        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let verifying_key = VerifyingKey::from(public_key);

        let signature = Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(verifying_key.verify(message, &signature).is_ok())
    }
}

/// Ed25519 operations
pub struct Ed25519Crypto;

impl Ed25519Crypto {
    /// Generates a new random private key using cryptographically secure RNG
    pub fn generate_private_key() -> [u8; 32] {
        let signing_key = Ed25519SigningKey::generate(&mut OsRng);
        signing_key.to_bytes()
    }

    /// Derives public key from private key
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 32]> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        Ok(signing_key.verifying_key().to_bytes())
    }

    /// Signs a message with Ed25519
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Verifies an Ed25519 signature
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 32],
    ) -> CryptoResult<bool> {
        let verifying_key = Ed25519VerifyingKey::from_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let signature = Ed25519Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(verifying_key.verify_strict(message, &signature).is_ok())
    }
}

fn verify_ecdsa_raw64_with_hash(
    data: &[u8],
    signature: &[u8; 64],
    public_key: &[u8],
    curve: ECCurve,
    hash_algorithm: HashAlgorithm,
) -> CryptoResult<bool> {
    match (curve, hash_algorithm) {
        (ECCurve::Secp256k1, HashAlgorithm::Keccak256) => {
            let sig = secp256k1::ecdsa::Signature::from_compact(signature)
                .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
            let pubkey = Secp256k1PublicKey::from_slice(public_key)
                .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
            let hash = Crypto::keccak256(data);
            let msg = Message::from_digest_slice(&hash)
                .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;
            Ok(Secp256k1::verification_only()
                .verify_ecdsa(&msg, &sig, &pubkey)
                .is_ok())
        }
        (ECCurve::Secp256r1, HashAlgorithm::Keccak256) => {
            let public_key = P256PublicKey::from_sec1_bytes(public_key)
                .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
            let verifying_key = VerifyingKey::from(public_key);
            let signature = Signature::try_from(signature.as_slice())
                .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
            let hash = Crypto::keccak256(data);
            Ok(verifying_key.verify_prehash(&hash, &signature).is_ok())
        }
        (ECCurve::Secp256k1, _) => {
            let public_key: [u8; 33] = public_key
                .try_into()
                .map_err(|_| CryptoError::invalid_key("Invalid public key length"))?;
            Secp256k1Crypto::verify(data, signature, &public_key)
        }
        (ECCurve::Secp256r1, _) => Secp256r1Crypto::verify(data, signature, public_key),
        (ECCurve::Ed25519, _) => Err(CryptoError::invalid_argument(
            "Ed25519 is not an ECDSA curve",
        )),
    }
}

/// ECDSA operations wrapper
pub struct ECDsa;

impl ECDsa {
    /// Signs data with ECDSA
    pub fn sign(data: &[u8], private_key: &[u8; 32], curve: ECCurve) -> CryptoResult<[u8; 64]> {
        match curve {
            ECCurve::Secp256k1 => Secp256k1Crypto::sign(data, private_key),
            ECCurve::Secp256r1 => Secp256r1Crypto::sign(data, private_key),
            ECCurve::Ed25519 => Ed25519Crypto::sign(data, private_key),
        }
    }

    /// Verifies ECDSA signature
    pub fn verify(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: ECCurve,
    ) -> CryptoResult<bool> {
        match curve {
            ECCurve::Secp256k1 => {
                if signature.len() != 64 || public_key.len() != 33 {
                    return Err(CryptoError::invalid_argument(
                        "Invalid signature or public key length",
                    ));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                verify_ecdsa_raw64_with_hash(
                    data,
                    &sig_bytes,
                    public_key,
                    ECCurve::Secp256k1,
                    HashAlgorithm::Sha256,
                )
            }
            ECCurve::Secp256r1 => {
                if signature.len() != 64 {
                    return Err(CryptoError::invalid_signature("Invalid signature length"));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                verify_ecdsa_raw64_with_hash(
                    data,
                    &sig_bytes,
                    public_key,
                    ECCurve::Secp256r1,
                    HashAlgorithm::Sha256,
                )
            }
            ECCurve::Ed25519 => {
                if signature.len() != 64 || public_key.len() != 32 {
                    return Err(CryptoError::invalid_argument(
                        "Invalid signature or public key length",
                    ));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                let pub_bytes: [u8; 32] = public_key
                    .try_into()
                    .map_err(|_| CryptoError::invalid_key("Invalid public key length"))?;
                Ed25519Crypto::verify(data, &sig_bytes, &pub_bytes)
            }
        }
    }
}

/// ECC operations wrapper
pub struct ECC;

impl ECC {
    /// Generates a public key from private key
    pub fn generate_public_key(private_key: &[u8; 32], curve: ECCurve) -> CryptoResult<ECPoint> {
        match curve {
            ECCurve::Secp256k1 => {
                let pub_bytes = Secp256k1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
            ECCurve::Secp256r1 => {
                let pub_bytes = Secp256r1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
            ECCurve::Ed25519 => {
                let pub_bytes = Ed25519Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
        }
    }

    /// Compresses a public key
    pub fn compress_public_key(public_key: &ECPoint) -> CryptoResult<Vec<u8>> {
        public_key
            .encode_compressed()
            .map_err(|e| CryptoError::invalid_point(e.to_string()))
    }
}

impl Crypto {
    /// Verifies ECDSA signature with secp256r1
    #[must_use]
    pub fn verify_signature_secp256r1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256r1).unwrap_or(false)
    }

    /// Verifies ECDSA signature with secp256k1
    #[must_use]
    pub fn verify_signature_secp256k1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256k1).unwrap_or(false)
    }

    /// Verifies an ECDSA signature using the specified curve and hash algorithm.
    #[must_use]
    pub fn verify_signature_with_curve(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: &ECCurve,
        hash_algorithm: HashAlgorithm,
    ) -> bool {
        if *curve == ECCurve::Ed25519 {
            return ECDsa::verify(data, signature, public_key, *curve).unwrap_or(false);
        }

        if signature.len() != 64 {
            return false;
        }

        let signature: [u8; 64] = match signature.try_into() {
            Ok(signature) => signature,
            Err(_) => return false,
        };

        verify_ecdsa_raw64_with_hash(data, &signature, public_key, *curve, hash_algorithm)
            .unwrap_or(false)
    }

    /// Verifies a signature against the supplied public key, inferring the curve where possible.
    #[must_use]
    pub fn verify_signature_bytes(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }

        let mut sig = [0u8; 64];
        sig.copy_from_slice(signature);

        match public_key.len() {
            32 => {
                let mut pk = [0u8; 32];
                pk.copy_from_slice(public_key);
                Ed25519Crypto::verify(message, &sig, &pk).unwrap_or(false)
            }
            33 => {
                let mut pk = [0u8; 33];
                pk.copy_from_slice(public_key);
                if Secp256k1Crypto::verify(message, &sig, &pk) == Ok(true) {
                    return true;
                }
                Secp256r1Crypto::verify(message, &sig, public_key).unwrap_or(false)
            }
            64 | 65 => {
                if Secp256r1Crypto::verify(message, &sig, public_key) == Ok(true) {
                    return true;
                }

                if let Ok(pk) = Secp256k1PublicKey::from_slice(public_key) {
                    let compressed = pk.serialize();
                    let mut buf = [0u8; 33];
                    buf.copy_from_slice(&compressed);
                    return Secp256k1Crypto::verify(message, &sig, &buf).unwrap_or(false);
                }
                false
            }
            _ => Secp256r1Crypto::verify(message, &sig, public_key).unwrap_or(false),
        }
    }
}

/// Convenience functions for Base58 encoding and decoding.
pub mod base58 {
    use super::Base58;
    use crate::CryptoResult;

    /// Encodes raw bytes as a Base58 string.
    #[must_use]
    pub fn encode(data: &[u8]) -> String {
        Base58::encode(data)
    }

    /// Decodes a Base58 string into raw bytes.
    pub fn decode(s: &str) -> CryptoResult<Vec<u8>> {
        Base58::decode(s)
    }

    /// Encodes raw bytes as a Base58Check string (with checksum).
    #[must_use]
    pub fn encode_check(data: &[u8]) -> String {
        Base58::encode_check(data)
    }

    /// Decodes a Base58Check string, verifying the embedded checksum.
    pub fn decode_check(s: &str) -> CryptoResult<Vec<u8>> {
        Base58::decode_check(s)
    }
}

// NOTE: Removed duplicate `pub mod hash` - use `crate::hash` or `NeoHash` instead

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        let a = [0u8; 32];
        let b = [0u8; 32];
        let c = [1u8; 32];

        // Same values should be equal
        assert!(ConstantTime::eq(&a, &b));
        assert!(ConstantTime::eq(&a, &a));

        // Different values should not be equal
        assert!(!ConstantTime::eq(&a, &c));

        // Single byte difference should be detected
        let mut d = [0u8; 32];
        d[15] = 1;
        assert!(!ConstantTime::eq(&a, &d));

        // Different positions
        d[15] = 0;
        d[0] = 1;
        assert!(!ConstantTime::eq(&a, &d));

        d[0] = 0;
        d[31] = 1;
        assert!(!ConstantTime::eq(&a, &d));
    }

    #[test]
    fn test_constant_time_eq_slice() {
        let a = vec![0u8; 32];
        let b = vec![0u8; 32];
        let c = vec![1u8; 32];
        let d = vec![0u8; 64]; // Different length

        // Same values should be equal
        assert!(ConstantTime::eq_slice(&a, &b));
        assert!(ConstantTime::eq_slice(&a, &a));

        // Different values should not be equal
        assert!(!ConstantTime::eq_slice(&a, &c));

        // Different lengths should not be equal
        assert!(!ConstantTime::eq_slice(&a, &d));

        // Empty slices
        assert!(ConstantTime::eq_slice(&[], &[]));
        assert!(!ConstantTime::eq_slice(&[], &[0u8]));
    }

    #[test]
    fn test_constant_time_eq_signature() {
        let sig1 = [0u8; 64];
        let sig2 = [0u8; 64];
        let mut sig3 = [0u8; 64];
        sig3[63] = 1;

        assert!(ConstantTime::eq_signature(&sig1, &sig2));
        assert!(!ConstantTime::eq_signature(&sig1, &sig3));
    }

    #[test]
    fn test_constant_time_eq_hash256() {
        let hash1 = [0u8; 32];
        let hash2 = [0u8; 32];
        let mut hash3 = [0u8; 32];
        hash3[31] = 1;

        assert!(ConstantTime::eq_hash256(&hash1, &hash2));
        assert!(!ConstantTime::eq_hash256(&hash1, &hash3));
    }

    #[test]
    fn test_constant_time_eq_hash160() {
        let hash1 = [0u8; 20];
        let hash2 = [0u8; 20];
        let mut hash3 = [0u8; 20];
        hash3[19] = 1;

        assert!(ConstantTime::eq_hash160(&hash1, &hash2));
        assert!(!ConstantTime::eq_hash160(&hash1, &hash3));
    }

    #[test]
    fn test_hash_functions() {
        let data = b"hello world";

        let sha256_hash = NeoHash::sha256(data);
        assert_eq!(sha256_hash.len(), 32);

        let hash160 = NeoHash::hash160(data);
        assert_eq!(hash160.len(), 20);

        let hash256 = NeoHash::hash256(data);
        assert_eq!(hash256.len(), 32);
    }

    #[test]
    fn test_secp256k1_operations() {
        let private_key = Secp256k1Crypto::generate_private_key().unwrap();
        let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"test message";

        let signature = Secp256k1Crypto::sign(message, &private_key).unwrap();
        let is_valid = Secp256k1Crypto::verify(message, &signature, &public_key).unwrap();

        assert!(is_valid);
    }

    #[test]
    fn test_base58_encoding() {
        let data = b"hello world";
        let encoded = Base58::encode(data);
        let decoded = Base58::decode(&encoded).unwrap();

        assert_eq!(data, decoded.as_slice());
    }

    #[test]
    fn test_murmur128_vectors() {
        let hex_input = hex::decode("718f952132679baa9c5c2aa0d329fd2a").unwrap();
        let cases: Vec<(&[u8], &str)> = vec![
            (b"hello", "0bc59d0ad25fde2982ed65af61227a0e"),
            (b"world", "3d3810fed480472bd214a14023bb407f"),
            (b"hello world", "e0a0632d4f51302c55e3b3e48d28795d"),
            (&hex_input, "9b4aa747ff0cf4e41b3d96251551c8ae"),
        ];

        for (input, expected) in cases {
            let hash = NeoHash::murmur128(input, 123u32);
            assert_eq!(hex::encode(hash), expected);
        }
    }
}
