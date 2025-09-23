//! Neo MPT Trie Library - Rust implementation of Neo.Cryptography.MPTTrie
//!
//! This crate provides a complete implementation of the Neo Merkle Patricia Trie,
//! matching the C# Neo.Cryptography.MPTTrie API exactly for compatibility.

pub mod cache;
pub mod error;
pub mod helper;
pub mod node;
#[allow(dead_code)]
pub mod node_branch;
#[allow(dead_code)]
pub mod node_extension;
#[allow(dead_code)]
pub mod node_hash;
#[allow(dead_code)]
pub mod node_leaf;
pub mod node_type;
pub mod proof;
pub mod trie;
#[allow(dead_code)]
pub mod trie_delete;
#[allow(dead_code)]
pub mod trie_find;
#[allow(dead_code)]
pub mod trie_get;
#[allow(dead_code)]
pub mod trie_proof;
#[allow(dead_code)]
pub mod trie_put;

// Re-export main types
pub use cache::{Cache, CacheStats, MemoryStorage, Storage};
pub use error::{MptError, MptResult};
pub use helper::{common_prefix_length, from_nibbles, to_nibbles};
pub use node::Node;
pub use node_type::NodeType;
pub use proof::{ProofNode, ProofVerifier};
pub use trie::Trie;

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::NodeType;
    #[test]
    fn test_basic_trie_creation() {
        // Basic test to ensure the module compiles
        assert_eq!(NodeType::Empty as u8, 0x04);
    }
}
