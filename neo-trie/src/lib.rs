//! # neo-trie
//!
//! Neo-compatible Merkle Patricia Trie nodes, cache logic, and state-root
//! operations.
//!
//! This module is intentionally local even though generic MPT crates exist.
//! Neo state roots depend on the C# `Neo.Cryptography.MPTTrie` node types,
//! serialization, hashing, proof shape, empty-node behavior, and cache-prefix
//! storage layout. Ethereum/Substrate trie crates are useful references, but
//! their encodings and hash domains are consensus-incompatible with Neo.
//!
//! ## Boundary
//!
//! This infrastructure crate exclusively owns Neo's MPT node graph, proof
//! shape, deterministic bytes, and backend-independent mutation cache. It uses
//! `neo-crypto` for Hash256 and must not own durable stores, StateService
//! policy, RPC transport, or node lifecycle.
//!
//! ## Contents
//!
//! - `mpt`: The private implementation tree for cache, nodes, validation, and
//!   trie operations.
//! - `tests`: Module-local tests and regression coverage.

#![doc(html_root_url = "https://docs.rs/neo-trie/0.10.0")]

mod mpt;

#[cfg(test)]
#[path = "tests/mpt_trie.rs"]
mod tests;

pub use mpt::{
    MPT_NODE_PREFIX, MptCache, MptError, MptMutationStats, MptResult, MptStoreLookup,
    MptStoreSnapshot, Node, NodeType, PersistedMptGraphLimits, PersistedMptGraphReport, Trie,
    TrieEntry, UnresolvedDeferredNode, validate_persisted_root_graph,
};
