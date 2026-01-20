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
//! - **Ed25519**: EdDSA signatures
//!
//! # Key Types
//!
//! - [`NeoHash`]: Hash function implementations (hash160, hash256, sha256, etc.)
//! - [`Secp256r1Crypto`]: P-256 key generation, signing, verification
//! - [`Secp256k1Crypto`]: secp256k1 operations for compatibility
//! - [`Ed25519Crypto`]: EdDSA operations
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

use crate::{Crypto, CryptoResult, ECCurve, ECPoint, HashAlgorithm};
use bs58;
use core::convert::TryFrom;
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};
use ed25519_dalek::{Signer as _, Verifier as _};
use hex;
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
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        Crypto::sha256(data)
    }

    /// Computes SHA-512 hash of the input data
    #[inline]
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        Crypto::sha512(data)
    }

    /// Computes Keccak-256 hash of the input data
    #[inline]
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        Crypto::keccak256(data)
    }

    /// Computes SHA3-256 hash of the input data
    #[inline]
    pub fn sha3_256(data: &[u8]) -> [u8; 32] {
        Crypto::sha3_256(data)
    }

    /// Computes SHA3-512 hash of the input data
    #[inline]
    pub fn sha3_512(data: &[u8]) -> [u8; 64] {
        Crypto::sha3_512(data)
    }

    /// Computes RIPEMD-160 hash of the input data
    #[inline]
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        Crypto::ripemd160(data)
    }

    /// Computes BLAKE2b hash of the input data
    #[inline]
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
    pub fn blake2s(data: &[u8]) -> [u8; 32] {
        Crypto::blake2s(data)
    }

    /// Computes Hash160 (RIPEMD-160 of SHA-256) - commonly used for Neo addresses
    #[inline]
    pub fn hash160(data: &[u8]) -> [u8; 20] {
        Crypto::hash160(data)
    }

    /// Computes Hash256 (double SHA-256) - commonly used for Neo transaction and block hashes
    #[inline]
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        Crypto::hash256(data)
    }

    /// Computes Murmur128 hash (x64 variant) used by Neo runtime.
    /// This is Neo-specific and not available in [`Crypto`].
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
    pub fn generate_private_key() -> Result<[u8; 32], String> {
        let mut rng = OsRng;
        for _ in 0..MAX_KEY_GEN_ATTEMPTS {
            let mut candidate = Zeroizing::new([0u8; 32]);
            rng.fill_bytes(candidate.as_mut());
            if let Ok(secret_key) = Secp256k1SecretKey::from_slice(candidate.as_ref()) {
                return Ok(secret_key.secret_bytes());
            }
        }
        Err(format!(
            "Failed to generate valid secp256k1 private key after {} attempts - RNG may be broken",
            MAX_KEY_GEN_ATTEMPTS
        ))
    }

    /// Derives public key from private key
    pub fn derive_public_key(private_key: &[u8; 32]) -> Result<[u8; 33], String> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let public_key = Secp256k1PublicKey::from_secret_key(&secp, &secret_key);
        Ok(public_key.serialize())
    }

    /// Signs a message with secp256k1
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 64], String> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| format!("Invalid private key: {}", e))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| format!("Invalid message: {}", e))?;

        let signature = secp.sign_ecdsa(&message, &secret_key);
        Ok(signature.serialize_compact())
    }

    /// Verifies a secp256k1 signature
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 33],
    ) -> Result<bool, String> {
        let secp = Secp256k1::verification_only();
        let public_key = Secp256k1PublicKey::from_slice(public_key)
            .map_err(|e| format!("Invalid public key: {}", e))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| format!("Invalid message: {}", e))?;

        let signature = secp256k1::ecdsa::Signature::from_compact(signature)
            .map_err(|e| format!("Invalid signature: {}", e))?;

        Ok(secp.verify_ecdsa(&message, &signature, &public_key).is_ok())
    }

    /// Recovers a compressed secp256k1 public key from a message hash and signature.
    /// Accepts 65-byte (r||s||v) or 64-byte EIP-2098 compact signatures.
    pub fn recover_public_key(message_hash: &[u8], signature: &[u8]) -> Result<Vec<u8>, String> {
        if signature.len() != 65 && signature.len() != 64 {
            return Err("Signature must be 65 or 64 bytes".to_string());
        }
        if message_hash.len() != 32 {
            return Err("Message hash must be 32 bytes".to_string());
        }

        let msg = Message::from_digest_slice(message_hash)
            .map_err(|e| format!("Invalid message hash: {}", e))?;

        let (rec_id, sig_bytes) = if signature.len() == 65 {
            let rec = signature[64];
            let rec_id = if rec >= 27 { rec - 27 } else { rec };
            if rec_id > 3 {
                return Err("Recovery id must be in range 0..3".to_string());
            }
            (rec_id, signature[..64].to_vec())
        } else {
            let mut sig = signature.to_vec();
            let y_parity = (sig[32] & 0x80) != 0;
            sig[32] &= 0x7f;
            let rec_id = if y_parity { 1 } else { 0 };
            (rec_id, sig)
        };

        let rec_id =
            RecoveryId::from_i32(rec_id as i32).map_err(|e| format!("Invalid recovery id: {}", e))?;
        let recoverable = RecoverableSignature::from_compact(&sig_bytes, rec_id)
            .map_err(|e| format!("Invalid recoverable signature: {}", e))?;

        let secp = Secp256k1::new();
        let public_key = secp
            .recover_ecdsa(&msg, &recoverable)
            .map_err(|e| format!("Failed to recover public key: {}", e))?;

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
    pub fn derive_public_key(private_key: &[u8; 32]) -> Result<Vec<u8>, String> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let verifying_key = VerifyingKey::from(&signing_key);
        Ok(verifying_key.to_encoded_point(true).as_bytes().to_vec())
    }

    /// Signs a message with secp256r1
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 64], String> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let signature: Signature = signing_key.sign(message);
        let bytes: [u8; 64] = signature.to_bytes().into();
        Ok(bytes)
    }

    /// Verifies a secp256r1 signature
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8]) -> Result<bool, String> {
        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| format!("Invalid public key: {}", e))?;
        let verifying_key = VerifyingKey::from(public_key);

        let signature = Signature::try_from(signature.as_slice())
            .map_err(|e| format!("Invalid signature: {}", e))?;

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
    pub fn derive_public_key(private_key: &[u8; 32]) -> Result<[u8; 32], String> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| format!("Invalid private key: {}", e))?;
        Ok(signing_key.verifying_key().to_bytes())
    }

    /// Signs a message with Ed25519
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 64], String> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Verifies an Ed25519 signature
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 32],
    ) -> Result<bool, String> {
        let verifying_key = Ed25519VerifyingKey::from_bytes(public_key)
            .map_err(|e| format!("Invalid public key: {}", e))?;
        let signature = Ed25519Signature::try_from(signature.as_slice())
            .map_err(|e| format!("Invalid signature: {}", e))?;

        Ok(verifying_key.verify_strict(message, &signature).is_ok())
    }
}

/// Base58 encoding/decoding utilities
pub struct Base58;

impl Base58 {
    /// Encodes data to Base58 string
    pub fn encode(data: &[u8]) -> String {
        bs58::encode(data).into_string()
    }

    /// Decodes Base58 string to bytes
    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        bs58::decode(s)
            .into_vec()
            .map_err(|e| format!("Base58 decode error: {}", e))
    }

    /// Encodes data to Base58Check string (Base58 with 4-byte checksum).
    pub fn encode_check(data: &[u8]) -> String {
        let mut payload = Vec::with_capacity(data.len() + 4);
        payload.extend_from_slice(data);
        let checksum = NeoHash::hash256(data);
        payload.extend_from_slice(&checksum[..4]);
        bs58::encode(payload).into_string()
    }

    /// Decodes Base58Check string back to bytes, verifying the checksum.
    pub fn decode_check(s: &str) -> Result<Vec<u8>, String> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|e| format!("Base58 decode error: {}", e))?;

        if bytes.len() < 4 {
            return Err("Invalid Base58Check payload: too short".to_string());
        }

        let (payload, checksum) = bytes.split_at(bytes.len() - 4);
        let expected = NeoHash::hash256(payload);
        if checksum != &expected[..4] {
            return Err("Invalid Base58Check checksum".to_string());
        }

        Ok(payload.to_vec())
    }
}

/// Hex encoding/decoding utilities
pub struct Hex;

impl Hex {
    /// Encodes data to hex string
    pub fn encode(data: &[u8]) -> String {
        hex::encode(data)
    }

    /// Decodes hex string to bytes
    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        hex::decode(s).map_err(|e| format!("Hex decode error: {}", e))
    }
}

/// ECDSA operations wrapper
pub struct ECDsa;

impl ECDsa {
    /// Signs data with ECDSA
    pub fn sign(data: &[u8], private_key: &[u8; 32], curve: ECCurve) -> Result<[u8; 64], String> {
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
    ) -> Result<bool, String> {
        match curve {
            ECCurve::Secp256k1 => {
                if signature.len() != 64 || public_key.len() != 33 {
                    return Err("Invalid signature or public key length".to_string());
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| "Invalid signature length".to_string())?;
                let pub_bytes: [u8; 33] = public_key
                    .try_into()
                    .map_err(|_| "Invalid public key length".to_string())?;
                Secp256k1Crypto::verify(data, &sig_bytes, &pub_bytes)
            }
            ECCurve::Secp256r1 => {
                if signature.len() != 64 {
                    return Err("Invalid signature length".to_string());
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| "Invalid signature length".to_string())?;
                Secp256r1Crypto::verify(data, &sig_bytes, public_key)
            }
            ECCurve::Ed25519 => {
                if signature.len() != 64 || public_key.len() != 32 {
                    return Err("Invalid signature or public key length".to_string());
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| "Invalid signature length".to_string())?;
                let pub_bytes: [u8; 32] = public_key
                    .try_into()
                    .map_err(|_| "Invalid public key length".to_string())?;
                Ed25519Crypto::verify(data, &sig_bytes, &pub_bytes)
            }
        }
    }
}

/// ECC operations wrapper
pub struct ECC;

impl ECC {
    /// Generates a public key from private key
    pub fn generate_public_key(private_key: &[u8; 32], curve: ECCurve) -> Result<ECPoint, String> {
        match curve {
            ECCurve::Secp256k1 => {
                let pub_bytes = Secp256k1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes).map_err(|e| e.to_string())
            }
            ECCurve::Secp256r1 => {
                let pub_bytes = Secp256r1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes).map_err(|e| e.to_string())
            }
            ECCurve::Ed25519 => {
                let pub_bytes = Ed25519Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes).map_err(|e| e.to_string())
            }
        }
    }

    /// Compresses a public key
    pub fn compress_public_key(public_key: &ECPoint) -> Result<Vec<u8>, String> {
        public_key.encode_compressed().map_err(|e| e.to_string())
    }
}

impl Crypto {
    /// Verifies ECDSA signature with secp256r1
    pub fn verify_signature_secp256r1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256r1).unwrap_or(false)
    }

    /// Verifies ECDSA signature with secp256k1
    pub fn verify_signature_secp256k1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256k1).unwrap_or(false)
    }

    pub fn verify_signature_with_curve(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: &ECCurve,
        hash_algorithm: HashAlgorithm,
    ) -> bool {
        match (curve, hash_algorithm) {
            (ECCurve::Secp256k1, HashAlgorithm::Keccak256) => {
                if signature.len() != 64 {
                    return false;
                }
                let sig = match secp256k1::ecdsa::Signature::from_compact(signature) {
                    Ok(sig) => sig,
                    Err(_) => return false,
                };
                let pubkey = match Secp256k1PublicKey::from_slice(public_key) {
                    Ok(key) => key,
                    Err(_) => return false,
                };
                let hash = Crypto::keccak256(data);
                let msg = match Message::from_digest_slice(&hash) {
                    Ok(msg) => msg,
                    Err(_) => return false,
                };
                Secp256k1::verification_only()
                    .verify_ecdsa(&msg, &sig, &pubkey)
                    .is_ok()
            }
            (ECCurve::Secp256r1, HashAlgorithm::Keccak256) => {
                if signature.len() != 64 {
                    return false;
                }
                let public_key = match P256PublicKey::from_sec1_bytes(public_key) {
                    Ok(key) => key,
                    Err(_) => return false,
                };
                let verifying_key = VerifyingKey::from(public_key);
                let signature = match Signature::try_from(signature) {
                    Ok(sig) => sig,
                    Err(_) => return false,
                };
                let hash = Crypto::keccak256(data);
                verifying_key.verify_prehash(&hash, &signature).is_ok()
            }
            _ => ECDsa::verify(data, signature, public_key, *curve).unwrap_or(false),
        }
    }

    /// Verifies a signature against the supplied public key, inferring the curve where possible.
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
                if let Ok(true) = Secp256k1Crypto::verify(message, &sig, &pk) {
                    return true;
                }
                Secp256r1Crypto::verify(message, &sig, public_key).unwrap_or(false)
            }
            64 | 65 => {
                if let Ok(true) = Secp256r1Crypto::verify(message, &sig, public_key) {
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

/// BLS12-381 operations using blst crate
/// Neo uses the "minimal-signature-size" scheme:
/// - Private key: scalar (32 bytes)
/// - Public key: G2 point (96 bytes compressed)
/// - Signature: G1 point (48 bytes compressed)
pub struct Bls12381Crypto;

/// Domain Separation Tag for Neo BLS12-381 signatures
/// This must match the C# implementation exactly for cross-compatibility
const NEO_BLS_DST: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";

impl Bls12381Crypto {
    fn validate_private_key(private_key: &[u8; 32]) -> Result<blst::blst_scalar, String> {
        use blst::blst_scalar;

        if private_key.iter().all(|b| *b == 0) {
            return Err("Invalid private key: scalar cannot be zero".to_string());
        }

        let mut sk_scalar = blst_scalar::default();
        unsafe {
            blst::blst_scalar_from_lendian(&mut sk_scalar, private_key.as_ptr());
            if !blst::blst_scalar_fr_check(&sk_scalar) {
                return Err("Invalid private key: scalar not in Fr field".to_string());
            }
        }
        Ok(sk_scalar)
    }

    /// Generates a new random private key using cryptographically secure RNG
    pub fn generate_private_key() -> [u8; 32] {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        bytes
    }

    /// Derives a public key from a private key
    /// Returns a 96-byte compressed G2 point
    pub fn derive_public_key(private_key: &[u8; 32]) -> Result<[u8; 96], String> {
        use blst::blst_p2;

        let sk_scalar = Self::validate_private_key(private_key)?;

        unsafe {
            // Derive public key: PK = sk * G2
            let mut pk = blst_p2::default();
            blst::blst_sk_to_pk_in_g2(&mut pk, &sk_scalar);

            // Compress and serialize
            let mut pk_bytes = [0u8; 96];
            blst::blst_p2_compress(pk_bytes.as_mut_ptr(), &pk);

            Ok(pk_bytes)
        }
    }

    /// Signs a message with BLS12-381
    /// Returns a 48-byte compressed G1 signature
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 48], String> {
        use blst::blst_p1;

        let sk_scalar = Self::validate_private_key(private_key)?;

        unsafe {
            // Hash message to G1 curve point
            let mut msg_point = blst_p1::default();
            blst::blst_hash_to_g1(
                &mut msg_point,
                message.as_ptr(),
                message.len(),
                NEO_BLS_DST.as_ptr(),
                NEO_BLS_DST.len(),
                std::ptr::null(), // No augmentation data
                0,
            );

            // Sign: signature = sk * H(msg)
            let mut signature = blst_p1::default();
            blst::blst_p1_mult(&mut signature, &msg_point, sk_scalar.b.as_ptr(), 255);

            // Compress and serialize signature
            let mut sig_bytes = [0u8; 48];
            blst::blst_p1_compress(sig_bytes.as_mut_ptr(), &signature);

            Ok(sig_bytes)
        }
    }

    /// Verifies a BLS12-381 signature
    /// signature: 48-byte compressed G1 point
    /// public_key: 96-byte compressed G2 point
    pub fn verify(
        message: &[u8],
        signature: &[u8; 48],
        public_key: &[u8; 96],
    ) -> Result<bool, String> {
        use blst::{blst_p1, blst_p1_affine, blst_p2_affine, BLST_ERROR};

        unsafe {
            // Deserialize signature (G1 point)
            let mut sig_affine = blst_p1_affine::default();
            let result = blst::blst_p1_uncompress(&mut sig_affine, signature.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err("Invalid signature encoding".to_string());
            }

            // Check signature point is in G1 subgroup
            if blst::blst_p1_affine_is_inf(&sig_affine) || !blst::blst_p1_affine_in_g1(&sig_affine)
            {
                return Err("Signature not in G1 subgroup".to_string());
            }

            // Deserialize public key (G2 point)
            let mut pk_affine = blst_p2_affine::default();
            let result = blst::blst_p2_uncompress(&mut pk_affine, public_key.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err("Invalid public key encoding".to_string());
            }

            // Check public key is in G2 subgroup
            if blst::blst_p2_affine_is_inf(&pk_affine) || !blst::blst_p2_affine_in_g2(&pk_affine) {
                return Err("Public key not in G2 subgroup".to_string());
            }

            // Hash message to G1 curve point
            let mut msg_point = blst_p1::default();
            blst::blst_hash_to_g1(
                &mut msg_point,
                message.as_ptr(),
                message.len(),
                NEO_BLS_DST.as_ptr(),
                NEO_BLS_DST.len(),
                std::ptr::null(),
                0,
            );

            // Convert hashed message to affine
            let mut msg_affine = blst_p1_affine::default();
            blst::blst_p1_to_affine(&mut msg_affine, &msg_point);

            // Verify pairing: e(sig, G2_gen) == e(H(msg), PK)
            let result = blst::blst_core_verify_pk_in_g2(
                &pk_affine,
                &sig_affine,
                true, // Check points are in correct subgroups
                message.as_ptr(),
                message.len(),
                NEO_BLS_DST.as_ptr(),
                NEO_BLS_DST.len(),
                std::ptr::null(),
                0,
            );

            Ok(result == BLST_ERROR::BLST_SUCCESS)
        }
    }

    /// Aggregates multiple BLS signatures into one
    /// Used for dBFT consensus where multiple validators sign
    pub fn aggregate_signatures(signatures: &[[u8; 48]]) -> Result<[u8; 48], String> {
        use blst::{blst_p1, blst_p1_affine};

        if signatures.is_empty() {
            return Err("No signatures to aggregate".to_string());
        }

        if signatures.len() == 1 {
            return Ok(signatures[0]);
        }

        unsafe {
            // Initialize with first signature
            let mut agg = blst_p1::default();
            let mut first_affine = blst_p1_affine::default();
            let result = blst::blst_p1_uncompress(&mut first_affine, signatures[0].as_ptr());
            if result != blst::BLST_ERROR::BLST_SUCCESS {
                return Err("Invalid first signature".to_string());
            }
            if blst::blst_p1_affine_is_inf(&first_affine)
                || !blst::blst_p1_affine_in_g1(&first_affine)
            {
                return Err("First signature not in G1 subgroup".to_string());
            }
            blst::blst_p1_from_affine(&mut agg, &first_affine);

            // Add remaining signatures
            for sig in &signatures[1..] {
                let mut sig_affine = blst_p1_affine::default();
                let result = blst::blst_p1_uncompress(&mut sig_affine, sig.as_ptr());
                if result != blst::BLST_ERROR::BLST_SUCCESS {
                    return Err("Invalid signature in aggregation".to_string());
                }
                if blst::blst_p1_affine_is_inf(&sig_affine)
                    || !blst::blst_p1_affine_in_g1(&sig_affine)
                {
                    return Err("Signature not in G1 subgroup".to_string());
                }
                blst::blst_p1_add_or_double_affine(&mut agg, &agg, &sig_affine);
            }

            // Compress aggregated signature
            let mut out = [0u8; 48];
            blst::blst_p1_compress(out.as_mut_ptr(), &agg);

            Ok(out)
        }
    }

    /// Verifies an aggregated signature against multiple public keys
    pub fn verify_aggregated(
        message: &[u8],
        aggregated_signature: &[u8; 48],
        public_keys: &[[u8; 96]],
    ) -> Result<bool, String> {
        use blst::{blst_p2, blst_p2_affine};

        if public_keys.is_empty() {
            return Err("No public keys provided".to_string());
        }

        // Aggregate public keys
        unsafe {
            let mut agg_pk = blst_p2::default();
            let mut first_affine = blst_p2_affine::default();
            let result = blst::blst_p2_uncompress(&mut first_affine, public_keys[0].as_ptr());
            if result != blst::BLST_ERROR::BLST_SUCCESS {
                return Err("Invalid first public key".to_string());
            }
            if blst::blst_p2_affine_is_inf(&first_affine)
                || !blst::blst_p2_affine_in_g2(&first_affine)
            {
                return Err("First public key not in G2 subgroup".to_string());
            }
            blst::blst_p2_from_affine(&mut agg_pk, &first_affine);

            for pk in &public_keys[1..] {
                let mut pk_affine = blst_p2_affine::default();
                let result = blst::blst_p2_uncompress(&mut pk_affine, pk.as_ptr());
                if result != blst::BLST_ERROR::BLST_SUCCESS {
                    return Err("Invalid public key in aggregation".to_string());
                }
                if blst::blst_p2_affine_is_inf(&pk_affine)
                    || !blst::blst_p2_affine_in_g2(&pk_affine)
                {
                    return Err("Public key not in G2 subgroup".to_string());
                }
                blst::blst_p2_add_or_double_affine(&mut agg_pk, &agg_pk, &pk_affine);
            }

            // Compress aggregated public key
            let mut agg_pk_bytes = [0u8; 96];
            blst::blst_p2_compress(agg_pk_bytes.as_mut_ptr(), &agg_pk);

            // Verify against aggregated public key
            Self::verify(message, aggregated_signature, &agg_pk_bytes)
        }
    }
}

pub mod base58 {
    use super::Base58;

    pub fn encode(data: &[u8]) -> String {
        Base58::encode(data)
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        Base58::decode(s)
    }

    pub fn encode_check(data: &[u8]) -> String {
        Base58::encode_check(data)
    }

    pub fn decode_check(s: &str) -> Result<Vec<u8>, String> {
        Base58::decode_check(s)
    }
}

pub mod hash {
    use super::NeoHash;

    pub fn sha256(data: &[u8]) -> [u8; 32] {
        NeoHash::sha256(data)
    }

    pub fn sha512(data: &[u8]) -> [u8; 64] {
        NeoHash::sha512(data)
    }

    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        NeoHash::keccak256(data)
    }

    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        NeoHash::ripemd160(data)
    }

    pub fn hash160(data: &[u8]) -> [u8; 20] {
        NeoHash::hash160(data)
    }

    pub fn hash256(data: &[u8]) -> [u8; 32] {
        NeoHash::hash256(data)
    }
}

pub mod murmur {
    use murmur3::murmur3_32;
    use std::convert::TryInto;
    use std::io::Cursor;

    pub fn murmur32(data: &[u8], seed: u32) -> u32 {
        murmur3_32(&mut Cursor::new(data), seed).expect("murmur32 hashing should not fail")
    }

    pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
        const C1: u64 = 0x87c3_7b91_1142_53d5;
        const C2: u64 = 0x4cf5_ad43_2745_937f;
        const R1: u32 = 31;
        const R2: u32 = 33;
        const M: u64 = 5;
        const N1: u64 = 0x52dc_e729;
        const N2: u64 = 0x3849_5ab5;

        fn fmix(mut h: u64) -> u64 {
            h = (h ^ (h >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
            h = (h ^ (h >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            h ^ (h >> 33)
        }

        let mut h1 = seed as u64;
        let mut h2 = seed as u64;

        let mut chunks = data.chunks_exact(16);
        for chunk in &mut chunks {
            let k1 = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
            let k2 = u64::from_le_bytes(chunk[8..16].try_into().unwrap());

            h1 ^= (k1.wrapping_mul(C1)).rotate_left(R1).wrapping_mul(C2);
            h1 = h1.rotate_left(27).wrapping_add(h2);
            h1 = h1.wrapping_mul(M).wrapping_add(N1);

            h2 ^= (k2.wrapping_mul(C2)).rotate_left(R2).wrapping_mul(C1);
            h2 = h2.rotate_left(31).wrapping_add(h1);
            h2 = h2.wrapping_mul(M).wrapping_add(N2);
        }

        let remainder = chunks.remainder();
        if !remainder.is_empty() {
            let mut tail = [0u8; 16];
            tail[..remainder.len()].copy_from_slice(remainder);
            let k1 = u64::from_le_bytes(tail[0..8].try_into().unwrap());
            let k2 = u64::from_le_bytes(tail[8..16].try_into().unwrap());

            h2 ^= (k2.wrapping_mul(C2)).rotate_left(R2).wrapping_mul(C1);
            h1 ^= (k1.wrapping_mul(C1)).rotate_left(R1).wrapping_mul(C2);
        }

        let length = data.len() as u64;
        h1 ^= length;
        h2 ^= length;

        h1 = h1.wrapping_add(h2);
        h2 = h2.wrapping_add(h1);

        h1 = fmix(h1);
        h2 = fmix(h2);

        h1 = h1.wrapping_add(h2);
        h2 = h2.wrapping_add(h1);

        let mut output = [0u8; 16];
        output[..8].copy_from_slice(&h1.to_le_bytes());
        output[8..].copy_from_slice(&h2.to_le_bytes());
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
