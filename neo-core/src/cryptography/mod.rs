//! Cryptography module for Neo blockchain
//!
//! This module provides cryptographic functionality matching the C# Neo.Cryptography namespace.

// NOTE: `neo-crypto` is the single source of truth for crypto primitives.
// This module exists for backward compatibility and only re-exports items.

pub use neo_crypto::{
    Base58, Base64, Bip32Crypto, BloomFilter, Bls12381Crypto, Crypto, CryptoError, CryptoResult,
    ECC, ECCurve, ECDsa, ECPoint, Ed25519Crypto, HashAlgorithm, Hex, MerkleTree,
    NEOFS_ECDSA_SHA512_PREFIX, NEOFS_ECDSA_SHA512_SIGNATURE_LEN, NamedCurveHash,
    Secp256k1Crypto, Secp256r1Crypto, Sha256Hasher, bloom_filter, crypto_utils, mpt_trie,
    murmur32, murmur128,
};

pub use neo_crypto::mpt_trie::{
    Cache, MptCache, MptError, MptResult, MptStoreSnapshot, Node, NodeType, Trie, TrieEntry,
};
