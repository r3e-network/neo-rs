//! # neo-crypto::mpt_trie
//!
//! Merkle Patricia Trie nodes, cache logic, and trie operations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `cache`: Cache state and mutation helpers.
//! - `error`: Typed error definitions and conversions.
//! - `node`: Daemon composition, CLI modes, and long-running node startup.
//! - `node_type`: MPT node type identifiers.
//! - `trie`: MPT trie operations and state-root helpers.
//! - `tests`: Module-local tests and regression coverage.

mod cache;
mod error;
mod node;
mod node_type;
mod trie;

#[cfg(test)]
#[path = "../tests/mpt_trie.rs"]
mod tests;

pub use cache::{MptCache, MptStoreSnapshot};
/// Type alias for [`MptCache`].
pub type Cache<S> = MptCache<S>;
pub use error::{MptError, MptResult};
pub use node::Node;
pub use node_type::NodeType;
pub use trie::{Trie, TrieEntry};
