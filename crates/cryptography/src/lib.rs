//! Cryptographic primitives for the Neo blockchain.
//!
//! This crate provides cryptographic functionality required by the Neo blockchain,
//! including elliptic curve cryptography, hashing algorithms, and other cryptographic
//! utilities.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod base58;
pub mod bloom_filter;
pub mod crypto;
pub mod ecc;
pub mod ecdsa;
pub mod ed25519;
pub mod hash;
pub mod hash_algorithm;
pub mod hasher;
pub mod helper;
pub mod merkle_tree;
pub mod murmur;
pub mod ripemd160;

// Re-exports for commonly used types
pub use ecc::{ECCurve, ECFieldElement, ECPoint};
pub use ecdsa::ECDsa;
pub use hash::{sha256, sha512, ripemd160, hash160, hash256};
pub use hash_algorithm::HashAlgorithm;
pub use hasher::Hasher;
pub use merkle_tree::MerkleTree;

// Convenience re-exports for wallet compatibility
pub use ecc as ECC;

/// Error types for cryptography operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Verification failed")]
    VerificationFailed,

    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type for cryptography operations
pub type Result<T> = std::result::Result<T, Error>;
