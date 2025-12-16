//! Cryptography module for Neo blockchain
//!
//! This module provides cryptographic functionality matching the C# Neo.Cryptography namespace.

// NOTE: `neo-crypto` is the single source of truth for crypto primitives.
// This module exists for backward compatibility and only re-exports items.

pub use neo_crypto::{
    Base58, BloomFilter, Bls12381Crypto, Crypto, CryptoError, CryptoResult, ECCurve, ECDsa,
    ECPoint, Ed25519Crypto, HashAlgorithm, Hex, NamedCurveHash, NeoHash, Secp256k1Crypto,
    Secp256r1Crypto, ECC,
};

pub mod bloom_filter {
    pub use neo_crypto::bloom_filter::*;
}

pub mod crypto_utils {
    pub use neo_crypto::crypto_utils::*;
}

pub mod mpt_trie {
    pub use neo_crypto::mpt_trie::*;
}

pub use neo_crypto::mpt_trie::{
    Cache, MptCache, MptError, MptResult, MptStoreSnapshot, Node, NodeType, Trie, TrieEntry,
};
