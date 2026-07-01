//! # neo-crypto
//!
//! Cryptographic hashes, curves, signatures, filters, and MPT trie helpers.
//!
//! ## Boundary
//!
//! This foundation crate owns cryptographic primitives and must not depend on
//! node services, RPC, storage engines, or UI crates.
//!
//! ## Contents
//!
//! - `curves`: Elliptic-curve adapters and point types used by Neo
//!   cryptography.
//! - `error`: Typed error definitions and conversions.
//! - `filters`: Probabilistic filters and related helpers used by networking
//!   and indexes.
//! - `formats`: Binary and textual conversion helpers for cryptographic data.
//! - `hashes`: Hash functions and hash-domain helpers used by protocol code.
//! - `keys`: wallet-facing key derivation, signing, and verification helpers.
//! - `mpt_trie`: Merkle Patricia Trie nodes, cache logic, and trie operations.
//! - `tests`: Module-local tests and regression coverage.

pub mod curves;
#[path = "errors/error.rs"]
pub mod error;
pub mod filters;
pub mod formats;
pub mod hashes;
pub mod keys;
pub mod mpt_trie;

pub use curves::{bls12381, bls12381_point, ecc};
pub use filters::bloom_filter;
pub use formats::encoding;
pub use hashes::{hash, merkle_tree, murmur, named_curve_hash};
pub use keys::{bip32, signature};

// Re-exports
pub use bip32::Bip32Crypto;
pub use bloom_filter::BloomFilter;
pub use bls12381::Bls12381Crypto;
pub use bls12381_point::Bls12381Point;
pub use ecc::{ECCurve, ECPoint};
pub use encoding::{base58, base64};
pub use error::{CryptoError, CryptoResult};
pub use hash::{Crypto, CtCompare, HashAlgorithm, Sha256Hasher};
pub use merkle_tree::MerkleTree;
pub use mpt_trie::{
    Cache, MptCache, MptError, MptResult, MptStoreSnapshot, Node, NodeType, Trie, TrieEntry,
};
pub use named_curve_hash::NamedCurveHash;
pub use signature::{
    ECC, ECDsa, Ed25519Crypto, NEOFS_ECDSA_SHA512_PREFIX, NEOFS_ECDSA_SHA512_SIGNATURE_LEN,
    Secp256k1Crypto, Secp256r1Crypto,
};

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
