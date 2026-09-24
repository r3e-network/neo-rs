//! Merkle Patricia Trie implementation ported from the C# Neo node.
//!
//! This module mirrors `Neo.Cryptography.MPTTrie` providing the `Node`, `MptCache`
//! and `Trie` types together with the supporting serialization logic used
//! throughout the Neo stack.

/// Cache management for MPT nodes
pub mod cache;
/// Error types for MPT operations
pub mod error;
/// MPT node types and operations
pub mod node;
/// Node type enumeration
pub mod node_type;
/// MPT trie implementation
pub mod trie;
/// Root branch buffer for optimized trie operations
pub mod root_branch_buffer;

#[cfg(test)]
mod tests;

pub use cache::{MptCache, MptStoreSnapshot, GlobalNodeCache};
pub use cache::get_cache_stats;
pub use cache::get_cache_count;
/// Type alias for [`MptCache`].
pub type Cache<S> = MptCache<S>;
pub use error::{MptError, MptResult};
pub use node::Node;
pub use node_type::NodeType;
pub use trie::{Trie, TrieEntry};
pub use root_branch_buffer::RootBranchBuffer;
