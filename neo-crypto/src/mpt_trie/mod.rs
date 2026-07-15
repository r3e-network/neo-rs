//! # neo-crypto::mpt_trie
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
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `cache`: Store snapshot trait and write-through cache helpers.
//! - `error`: Typed error definitions and conversions.
//! - `node`: Neo MPT node representation and C#-compatible serialization.
//! - `node_type`: MPT node type identifiers.
//! - `trie`: MPT trie operations and state-root helpers.
//! - `tests`: Module-local tests and regression coverage.

mod cache;
mod error;
mod metrics;
mod node;
mod node_type;
mod trie;

#[cfg(test)]
#[path = "../tests/mpt_trie.rs"]
mod tests;

pub use cache::{MptCache, MptMutationStats, MptStoreLookup, MptStoreSnapshot};
/// Type alias for [`MptCache`].
pub type Cache<S> = MptCache<S>;
pub use error::{MptError, MptResult};
pub use node::Node;
pub use node_type::NodeType;
pub use trie::{Trie, TrieEntry};
