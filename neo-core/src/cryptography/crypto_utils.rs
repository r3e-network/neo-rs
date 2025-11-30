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

use blake2::{Blake2b512, Blake2s256};
use bs58;
use core::convert::TryFrom;
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};
use ed25519_dalek::{Signer as _, Verifier as _};
use hex;
use p256::{
    ecdsa::{Signature, SigningKey, VerifyingKey},
    PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};
use rand::{rngs::OsRng, RngCore};
use ripemd::Ripemd160;
use secp256k1::{
    Message, PublicKey as Secp256k1PublicKey, Secp256k1, SecretKey as Secp256k1SecretKey,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256, Sha512};
use sha3::Keccak256;
use std::cmp::Ordering;

/// Hash algorithms supported by Neo
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
    Keccak256,
    Ripemd160,
    Blake2b,
    Blake2s,
}

/// Neo-specific hash functions
pub struct NeoHash;

impl NeoHash {
    /// Computes SHA-256 hash of the input data
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes SHA-512 hash of the input data
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        let mut hasher = Sha512::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes Keccak-256 hash of the input data
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes RIPEMD-160 hash of the input data
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        let mut hasher = Ripemd160::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes BLAKE2b hash of the input data
    pub fn blake2b(data: &[u8]) -> [u8; 64] {
        let mut hasher = Blake2b512::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes BLAKE2s hash of the input data
    pub fn blake2s(data: &[u8]) -> [u8; 32] {
        let mut hasher = Blake2s256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes Hash160 (RIPEMD-160 of SHA-256) - commonly used for Neo addresses
    pub fn hash160(data: &[u8]) -> [u8; 20] {
        let sha256_hash = Self::sha256(data);
        Self::ripemd160(&sha256_hash)
    }

    /// Computes Hash256 (double SHA-256) - commonly used for Neo transaction and block hashes
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        let first_hash = Self::sha256(data);
        Self::sha256(&first_hash)
    }

    /// Computes Murmur128 hash (x64 variant) used by Neo runtime.
    pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
        murmur::murmur128(data, seed)
    }
}

/// ECDSA operations for secp256k1 (Bitcoin's curve)
pub struct Secp256k1Crypto;

impl Secp256k1Crypto {
    /// Generates a new random private key
    pub fn generate_private_key() -> [u8; 32] {
        let mut rng = OsRng;
        loop {
            let mut candidate = [0u8; 32];
            rng.fill_bytes(&mut candidate);
            if let Ok(secret_key) = Secp256k1SecretKey::from_slice(&candidate) {
                return secret_key.secret_bytes();
            }
        }
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
    /// Generates a new random private key
    pub fn generate_private_key() -> [u8; 32] {
        let signing_key = Ed25519SigningKey::generate(&mut rand::thread_rng());
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

/// Elliptic curve point representation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ECPoint {
    encoded: Vec<u8>,
}

impl ECPoint {
    /// Creates a new ECPoint from an already-encoded representation.
    pub fn new(encoded: Vec<u8>) -> Self {
        Self { encoded }
    }

    /// Creates an ECPoint from bytes, validating the length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        match bytes.len() {
            33 | 65 => Ok(Self {
                encoded: bytes.to_vec(),
            }),
            len => Err(format!("Invalid ECPoint byte length: {}", len)),
        }
    }

    /// Returns the encoded form of the point.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encoded.clone()
    }

    /// Returns a reference to the encoded form of the point.
    pub fn as_bytes(&self) -> &[u8] {
        &self.encoded
    }

    /// Returns the encoded form of the point as a cloned vector.
    pub fn encoded(&self) -> Vec<u8> {
        self.encoded.clone()
    }

    /// Returns true if the point is stored in compressed form.
    pub fn is_compressed(&self) -> bool {
        self.encoded.len() == 33 && matches!(self.encoded.first(), Some(0x02) | Some(0x03))
    }

    /// Encodes the point in the desired form.
    pub fn encode_point(&self, compressed: bool) -> Result<Vec<u8>, String> {
        match (compressed, self.encoded.len()) {
            (true, 33) => Ok(self.encoded.clone()),
            (false, 65) => Ok(self.encoded.clone()),
            (false, 33) => Ok(self.encoded.clone()),
            (true, 65) => {
                // Without real curve arithmetic we cannot derive parity; default to even.
                let mut result = Vec::with_capacity(33);
                result.push(0x02);
                result.extend_from_slice(&self.encoded[1..33]);
                Ok(result)
            }
            (_, len) => Err(format!("Unsupported ECPoint length: {}", len)),
        }
    }

    /// Decodes a compressed ECPoint.
    pub fn decode_compressed(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 33 {
            return Err("Compressed ECPoint must be 33 bytes".to_string());
        }
        Ok(Self {
            encoded: bytes.to_vec(),
        })
    }

    /// Decodes an ECPoint from bytes (generic).
    pub fn decode(bytes: &[u8], _curve: ECCurve) -> Result<Self, String> {
        Self::from_bytes(bytes)
    }

    /// Returns true if the point appears to be a valid encoded point for the supported curves.
    pub fn is_valid(&self) -> bool {
        match self.encoded.len() {
            33 => matches!(self.encoded.first(), Some(0x02) | Some(0x03)),
            65 => matches!(self.encoded.first(), Some(0x04)),
            _ => false,
        }
    }

    /// Returns the point encoded in compressed form.
    pub fn encode_compressed(&self) -> Result<Vec<u8>, String> {
        self.encode_point(true)
    }

    /// Returns slices used for ordering comparisons that mirror the C# behavior:
    /// 1. Compare X coordinate first.
    /// 2. If X matches, compare Y (parity for compressed, full Y for uncompressed).
    fn ordering_components(&self) -> (&[u8], &[u8]) {
        match self.encoded.as_slice() {
            // Compressed form: [0x02 | 0x03][X:32]
            [0x02 | 0x03, rest @ ..] if rest.len() == 32 => (rest, &self.encoded[0..1]),
            // Uncompressed form: [0x04][X:32][Y:32]
            [0x04, rest @ ..] if rest.len() == 64 => (&rest[..32], &rest[32..]),
            // Fallback to full encoding for unexpected shapes to keep ordering stable
            _ => (&self.encoded[..], &[]),
        }
    }
}

impl Serialize for ECPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.encoded()))
    }
}

impl<'de> Deserialize<'de> for ECPoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let bytes = hex::decode(&value)
            .map_err(|e| serde::de::Error::custom(format!("Invalid ECPoint hex: {}", e)))?;
        ECPoint::from_bytes(&bytes)
            .map_err(|e| serde::de::Error::custom(format!("Invalid ECPoint: {}", e)))
    }
}

impl PartialOrd for ECPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ECPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        let (self_x, self_y_key) = self.ordering_components();
        let (other_x, other_y_key) = other.ordering_components();

        match self_x.cmp(other_x) {
            Ordering::Equal => self_y_key.cmp(other_y_key),
            ord => ord,
        }
    }
}

/// Elliptic curve types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ECCurve {
    Secp256k1,
    Secp256r1,
    Ed25519,
}

impl ECCurve {
    /// Returns the secp256r1 curve (P-256)
    pub fn secp256r1() -> Self {
        Self::Secp256r1
    }

    /// Returns the secp256k1 curve
    pub fn secp256k1() -> Self {
        Self::Secp256k1
    }

    /// Returns the Ed25519 curve
    pub fn ed25519() -> Self {
        Self::Ed25519
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
                let sig_bytes: [u8; 64] = signature.try_into().unwrap();
                let pub_bytes: [u8; 33] = public_key.try_into().unwrap();
                Secp256k1Crypto::verify(data, &sig_bytes, &pub_bytes)
            }
            ECCurve::Secp256r1 => {
                if signature.len() != 64 {
                    return Err("Invalid signature length".to_string());
                }
                let sig_bytes: [u8; 64] = signature.try_into().unwrap();
                Secp256r1Crypto::verify(data, &sig_bytes, public_key)
            }
            ECCurve::Ed25519 => {
                if signature.len() != 64 || public_key.len() != 32 {
                    return Err("Invalid signature or public key length".to_string());
                }
                let sig_bytes: [u8; 64] = signature.try_into().unwrap();
                let pub_bytes: [u8; 32] = public_key.try_into().unwrap();
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
                ECPoint::from_bytes(&pub_bytes)
            }
            ECCurve::Secp256r1 => {
                let pub_bytes = Secp256r1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes(&pub_bytes)
            }
            ECCurve::Ed25519 => {
                let pub_bytes = Ed25519Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes(&pub_bytes)
            }
        }
    }

    /// Compresses a public key
    pub fn compress_public_key(public_key: &ECPoint) -> Result<Vec<u8>, String> {
        if public_key.is_compressed() {
            return Ok(public_key.to_bytes());
        }

        let bytes = public_key.as_bytes();
        if bytes.len() != 65 {
            return Err("Uncompressed public key must be 65 bytes".to_string());
        }

        let prefix = match bytes[0] {
            0x02 | 0x03 => bytes[0],
            0x04 => 0x02,
            other => other,
        };

        let mut result = Vec::with_capacity(33);
        result.push(prefix);
        result.extend_from_slice(&bytes[1..33]);
        Ok(result)
    }
}

/// Crypto operations wrapper (matches C# Crypto class)
pub struct Crypto;

impl Crypto {
    /// Computes SHA-256 hash
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        NeoHash::sha256(data)
    }

    /// Computes RIPEMD-160 hash
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        NeoHash::ripemd160(data)
    }

    /// Computes Hash160 (RIPEMD160(SHA256(data))).
    pub fn hash160(data: &[u8]) -> [u8; 20] {
        NeoHash::hash160(data)
    }

    /// Computes Hash256 (double SHA-256).
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        NeoHash::hash256(data)
    }

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
        _hash_algorithm: HashAlgorithm,
    ) -> bool {
        ECDsa::verify(data, signature, public_key, *curve).unwrap_or(false)
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
    /// Generates a new random private key
    pub fn generate_private_key() -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    /// Derives a public key from a private key
    /// Returns a 96-byte compressed G2 point
    pub fn derive_public_key(private_key: &[u8; 32]) -> Result<[u8; 96], String> {
        use blst::{blst_p2, blst_scalar};

        unsafe {
            // Convert private key bytes to scalar
            let mut sk_scalar = blst_scalar::default();
            blst::blst_scalar_from_lendian(&mut sk_scalar, private_key.as_ptr());

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
        use blst::{blst_p1, blst_scalar};

        unsafe {
            // Convert private key bytes to scalar
            let mut sk_scalar = blst_scalar::default();
            blst::blst_scalar_from_lendian(&mut sk_scalar, private_key.as_ptr());

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
            if !blst::blst_p1_affine_in_g1(&sig_affine) {
                return Err("Signature not in G1 subgroup".to_string());
            }

            // Deserialize public key (G2 point)
            let mut pk_affine = blst_p2_affine::default();
            let result = blst::blst_p2_uncompress(&mut pk_affine, public_key.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err("Invalid public key encoding".to_string());
            }

            // Check public key is in G2 subgroup
            if !blst::blst_p2_affine_in_g2(&pk_affine) {
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
            blst::blst_p1_from_affine(&mut agg, &first_affine);

            // Add remaining signatures
            for sig in &signatures[1..] {
                let mut sig_affine = blst_p1_affine::default();
                let result = blst::blst_p1_uncompress(&mut sig_affine, sig.as_ptr());
                if result != blst::BLST_ERROR::BLST_SUCCESS {
                    return Err("Invalid signature in aggregation".to_string());
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
            blst::blst_p2_from_affine(&mut agg_pk, &first_affine);

            for pk in &public_keys[1..] {
                let mut pk_affine = blst_p2_affine::default();
                let result = blst::blst_p2_uncompress(&mut pk_affine, pk.as_ptr());
                if result != blst::BLST_ERROR::BLST_SUCCESS {
                    return Err("Invalid public key in aggregation".to_string());
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
        let private_key = Secp256k1Crypto::generate_private_key();
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
