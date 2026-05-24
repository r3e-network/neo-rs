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
pub use crate::murmur;
pub use crate::neo_hash::NeoHash;
pub use crate::signature::{ECDsa, Ed25519Crypto, Secp256k1Crypto, Secp256r1Crypto, ECC};

/// Convenience functions for Base58 encoding and decoding.
pub mod base58 {
    pub use crate::encoding::base58::*;
}

// NOTE: Removed duplicate `pub mod hash` - use `crate::hash` or `NeoHash` instead
