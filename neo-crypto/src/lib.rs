#![warn(missing_docs)]
//! # Neo Crypto
//!
//! Cryptographic utilities for the Neo blockchain implementation.
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

/// Bloom filter implementation for probabilistic set membership testing.
pub mod bloom_filter;
/// BLS12-381 signature helpers for Neo.
pub mod bls12381;
pub mod crypto_utils;
pub mod ecc;
/// Encoding helpers used by Neo cryptographic APIs.
pub mod encoding;
pub mod error;
pub mod hash;
pub mod mpt_trie;
/// Murmur3 hash helpers used by Neo runtime and native contracts.
pub mod murmur;
pub mod named_curve_hash;
/// Signature and key helpers used by Neo cryptographic APIs.
pub mod signature;

// Re-exports
pub use bloom_filter::BloomFilter;
pub use bls12381::Bls12381Crypto;
pub use ecc::{ECCurve, ECPoint};
pub use encoding::{Base58, Base64, Hex};
pub use error::{CryptoError, CryptoResult};
pub use hash::{ct_hash_eq, ct_hash_slice_eq, Crypto, HashAlgorithm};
pub use mpt_trie::{
    Cache, MptCache, MptError, MptResult, MptStoreSnapshot, Node, NodeType, Trie, TrieEntry,
};
pub use murmur::{murmur128, murmur32};
pub use named_curve_hash::NamedCurveHash;
pub use signature::{
    ECDsa, Ed25519Crypto, Secp256k1Crypto, Secp256r1Crypto, ECC, NEOFS_ECDSA_SHA512_PREFIX,
    NEOFS_ECDSA_SHA512_SIGNATURE_LEN,
};

#[cfg(test)]
mod tests {
    // Tests are inline within source files
}
