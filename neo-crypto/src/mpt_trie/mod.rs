//! Merkle Patricia Trie implementation ported from the C# Neo node.
//!
//! This module mirrors `Neo.Cryptography.MPTTrie` providing the `Node`, `MptCache`
//! and `Trie` types together with the supporting serialization logic used
//! throughout the Neo stack.

mod cache;
mod error;
mod node;
mod node_type;
mod trie;

#[cfg(test)]
mod tests;

pub use cache::{MptCache, MptStoreSnapshot};
pub type Cache<S> = MptCache<S>;
pub use error::{MptError, MptResult};
pub use node::Node;
pub use node_type::NodeType;
pub use trie::{Trie, TrieEntry};
