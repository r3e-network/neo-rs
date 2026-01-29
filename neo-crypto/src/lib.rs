// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo Crypto
//!
//! Cryptographic utilities for the Neo blockchain implementation.

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
//!
//! This crate provides cryptographic primitives required by Neo N3, including
//! hash functions, elliptic curve operations, and utility types.
//!
//! ## Module Overview
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`crypto_utils`] | Main cryptographic utilities (hash, sign, verify) |
//! | [`ecc`] | Elliptic curve cryptography (secp256r1, secp256k1) |
//! | [`hash`] | Hash algorithms (SHA-256, RIPEMD-160, Keccak) |
//! | [`bloom_filter`] | Bloom filter for efficient set membership |
//! | [`mpt_trie`] | Merkle Patricia Trie for state storage |
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use neo_crypto::{Crypto, HashAlgorithm};
//!
//! // Compute SHA-256 hash
//! let hash = Crypto::sha256(b"Hello, Neo!");
//!
//! // Compute Hash160 (RIPEMD160(SHA256(data)))
//! let script_hash = Crypto::hash160(b"contract script");
//! ```
//!
//! This crate provides cryptographic primitives required by Neo N3:
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
//! ## Design Principles
//!
//! - **Security**: All random number generation uses `OsRng` (cryptographically secure)
//! - **Compatibility**: Matches C# Neo implementation behavior
//! - **Performance**: Optimized for blockchain operations
//!
//! ## Example
//!
//! ```rust
//! use neo_crypto::{Crypto, HashAlgorithm};
//!
//! // Compute SHA-256 hash
//! let hash = Crypto::sha256(b"Hello, Neo!");
//!
//! // Compute Hash160 (RIPEMD160(SHA256(data)))
//! let script_hash = Crypto::hash160(b"contract script");
//! ```

pub mod bloom_filter;
pub mod crypto_utils;
pub mod ecc;
pub mod error;
pub mod hash;
pub mod mpt_trie;
pub mod named_curve_hash;

// Re-exports
pub use bloom_filter::BloomFilter;
pub use crypto_utils::{
    Base58, Bls12381Crypto, ConstantTime, ECDsa, Ed25519Crypto, Hex, NeoHash, Secp256k1Crypto,
    Secp256r1Crypto, ECC,
};
pub use ecc::{ECCurve, ECPoint};
pub use error::{CryptoError, CryptoResult};
pub use hash::{ct_hash_eq, ct_hash_slice_eq, Crypto, HashAlgorithm};
pub use mpt_trie::{
    Cache, MptCache, MptError, MptResult, MptStoreSnapshot, Node, NodeType, Trie, TrieEntry,
};
pub use named_curve_hash::NamedCurveHash;

#[cfg(test)]
mod tests {
    // Tests are inline within source files
}
