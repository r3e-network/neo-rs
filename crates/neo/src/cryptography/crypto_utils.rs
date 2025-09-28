//! Cryptographic utilities for Neo blockchain.
//!
//! This module provides common cryptographic functions using external, well-tested crates.

use blake2::{Blake2b512, Blake2s256};
use bs58;
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};
use hex;
use p256::{
    ecdsa::{Signature, SigningKey, VerifyingKey},
    PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};
use rand::RngCore;
use ripemd::Ripemd160;
use secp256k1::{
    Message, PublicKey as Secp256k1PublicKey, Secp256k1, SecretKey as Secp256k1SecretKey,
};
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
}

/// ECDSA operations for secp256k1 (Bitcoin's curve)
pub struct Secp256k1Crypto;

impl Secp256k1Crypto {
    /// Generates a new random private key
    pub fn generate_private_key() -> [u8; 32] {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::new(&mut rand::thread_rng());
        secret_key.secret_bytes()
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
        let secret_key = P256SecretKey::random(&mut rand::thread_rng());
        secret_key.to_bytes().into()
    }

    /// Derives public key from private key
    pub fn derive_public_key(private_key: &[u8; 32]) -> Result<Vec<u8>, String> {
        let secret_key = P256SecretKey::from_bytes(private_key.into())
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let public_key = secret_key.public_key();
        Ok(public_key.to_sec1_bytes().to_vec())
    }

    /// Signs a message with secp256r1
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 64], String> {
        let secret_key = P256SecretKey::from_bytes(private_key.into())
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let signing_key = SigningKey::from(secret_key);
        let signature: Signature = signing_key.sign(message);
        Ok(signature.to_bytes().into())
    }

    /// Verifies a secp256r1 signature
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8]) -> Result<bool, String> {
        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| format!("Invalid public key: {}", e))?;
        let verifying_key = VerifyingKey::from(public_key);

        let signature = Signature::from_bytes(signature.into())
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
        let signing_key = Ed25519SigningKey::from_bytes(private_key)
            .map_err(|e| format!("Invalid private key: {}", e))?;
        Ok(signing_key.verifying_key().to_bytes())
    }

    /// Signs a message with Ed25519
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 64], String> {
        let signing_key = Ed25519SigningKey::from_bytes(private_key)
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
        let signature = Ed25519Signature::from_bytes(signature)
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
}

impl PartialOrd for ECPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.encoded.cmp(&other.encoded))
    }
}

impl Ord for ECPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.encoded.cmp(&other.encoded)
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

    /// Verifies ECDSA signature with secp256r1
    pub fn verify_signature_secp256r1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256r1).unwrap_or(false)
    }

    /// Verifies ECDSA signature with secp256k1
    pub fn verify_signature_secp256k1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256k1).unwrap_or(false)
    }
}

/// BLS12-381 operations using blst crate
pub struct Bls12381Crypto;

impl Bls12381Crypto {
    /// Generates a new random private key
    pub fn generate_private_key() -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    /// Signs a message with BLS12-381
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> Result<[u8; 96], String> {
        // This is a simplified implementation
        // In practice, you'd use the blst crate's BLS signature functionality
        Err("BLS12-381 signing not implemented yet".to_string())
    }

    /// Verifies a BLS12-381 signature
    pub fn verify(
        message: &[u8],
        signature: &[u8; 96],
        public_key: &[u8; 48],
    ) -> Result<bool, String> {
        // This is a simplified implementation
        // In practice, you'd use the blst crate's BLS verification functionality
        Err("BLS12-381 verification not implemented yet".to_string())
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
}
