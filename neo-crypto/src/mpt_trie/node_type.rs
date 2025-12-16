//! MPT Node Type enumeration matching C# Neo.Cryptography.MPTTrie.NodeType

use serde::{Deserialize, Serialize};

/// Node types in the Merkle Patricia Trie
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeType {
    /// Branch node with up to 17 children
    BranchNode = 0x00,
    /// Extension node with a key prefix and single child
    ExtensionNode = 0x01,
    /// Leaf node containing a value
    LeafNode = 0x02,
    /// Hash-only node representing a not-yet-loaded child.
    HashNode = 0x03,
    /// Empty node
    Empty = 0x04,
}

impl NodeType {
    /// Convert from byte representation
    pub fn from_byte(b: u8) -> Result<Self, String> {
        match b {
            0x00 => Ok(NodeType::BranchNode),
            0x01 => Ok(NodeType::ExtensionNode),
            0x02 => Ok(NodeType::LeafNode),
            0x03 => Ok(NodeType::HashNode),
            0x04 => Ok(NodeType::Empty),
            _ => Err(format!("Invalid NodeType byte: {}", b)),
        }
    }

    /// Convert to byte representation
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}
