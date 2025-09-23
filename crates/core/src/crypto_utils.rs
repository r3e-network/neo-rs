//! Cryptographic utilities for Neo blockchain.
//!
//! This module provides common cryptographic functions using external, well-tested crates.

use sha2::{Digest, Sha256, Sha512};
use sha3::Keccak256;
use ripemd::Ripemd160;
use blake2::{Blake2b512, Blake2s256};
use secp256k1::{Message, PublicKey as Secp256k1PublicKey, Secp256k1, SecretKey as Secp256k1SecretKey};
use p256::{
    ecdsa::{SigningKey, VerifyingKey, Signature},
    PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};
use ed25519_dalek::{SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey, Signature as Ed25519Signature};
use bs58;
use hex;
use rand::RngCore;

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
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8; 33]) -> Result<bool, String> {
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
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8; 32]) -> Result<bool, String> {
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
        bs58::decode(s).into_vec()
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
        hex::decode(s)
            .map_err(|e| format!("Hex decode error: {}", e))
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
    pub fn verify(message: &[u8], signature: &[u8; 96], public_key: &[u8; 48]) -> Result<bool, String> {
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
