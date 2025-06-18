//! Neo MPT Trie Library - Rust implementation of Neo.Cryptography.MPTTrie
//! 
//! This crate provides a complete implementation of the Neo Merkle Patricia Trie,
//! matching the C# Neo.Cryptography.MPTTrie API exactly for compatibility.

pub mod error;
pub mod helper;
pub mod node_type;
pub mod node;
pub mod cache;
pub mod trie;
pub mod proof;

// Re-export main types
pub use error::{MptError, MptResult};
pub use helper::{to_nibbles, from_nibbles, common_prefix_length};
pub use node_type::NodeType;
pub use node::Node;
pub use cache::{Cache, CacheStats, Storage, MemoryStorage};
pub use trie::Trie;
pub use proof::{ProofVerifier, ProofNode};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_trie_creation() {
        // Basic test to ensure the module compiles
        assert_eq!(NodeType::Empty as u8, 0x04);
    }
} 