use super::error::{MptError, MptResult};
use super::node_type::NodeType;
use crate::Crypto;
use neo_io::serializable::helper::get_var_size;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::UInt256;
use neo_primitives::UINT256_SIZE;
use parking_lot::RwLock;
use std::mem;
use tracing::error;

const MAX_STORAGE_KEY_SIZE: usize = 64;
const MAX_STORAGE_VALUE_SIZE: usize = u16::MAX as usize;

/// Total number of children supported by a branch node (16 nibbles + value).
pub const BRANCH_CHILD_COUNT: usize = 17;
/// Index used by branch nodes to store their value.
pub const BRANCH_VALUE_INDEX: usize = BRANCH_CHILD_COUNT - 1;
/// Maximum key length when expressed as nibbles (matches C# `ApplicationEngine` constraints).
pub const MAX_KEY_LENGTH: usize = (MAX_STORAGE_KEY_SIZE + mem::size_of::<i32>()) * 2;
/// Maximum value length supported by the trie (matches `ApplicationEngine` limits).
pub const MAX_VALUE_LENGTH: usize = 3 + MAX_STORAGE_VALUE_SIZE + mem::size_of::<bool>();

/// Merkle Patricia trie node.
///
/// This mirrors the behaviour of `Neo.Cryptography.MPTTrie.Node` from the C# reference
/// implementation and provides identical serialization semantics.
#[derive(Debug)]
pub struct Node {
    pub node_type: NodeType,
    pub reference: u32,
    hash: RwLock<Option<UInt256>>,
    pub children: Vec<Self>,
    pub key: Vec<u8>,
    pub next: Option<Box<Self>>,
    pub value: Vec<u8>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            node_type: NodeType::Empty,
            reference: 0,
            hash: RwLock::new(None),
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value: Vec::new(),
        }
    }
}

impl Clone for Node {
    fn clone(&self) -> Self {
        let cached_hash = *self.hash.read();
        let mut node = Self {
            node_type: self.node_type,
            reference: self.reference,
            hash: RwLock::new(None),
            children: Vec::new(),
            key: self.key.clone(),
            next: None,
            value: self.value.clone(),
        };

        match self.node_type {
            NodeType::BranchNode => {
                node.children = self.children.iter().map(Self::clone_as_child).collect();
            }
            NodeType::ExtensionNode => {
                node.next = self
                    .next
                    .as_ref()
                    .map(|child| Box::new(child.clone_as_child()));
            }
            NodeType::LeafNode | NodeType::Empty => {}
            NodeType::HashNode => {
                *node.hash.write() = cached_hash;
            }
        }

        node
    }
}

impl Node {
    /// Creates an empty node.
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new branch node with default children.
    #[must_use] 
    pub fn new_branch() -> Self {
        Self {
            node_type: NodeType::BranchNode,
            reference: 1,
            hash: RwLock::new(None),
            children: (0..BRANCH_CHILD_COUNT).map(|_| Self::new()).collect(),
            key: Vec::new(),
            next: None,
            value: Vec::new(),
        }
    }

    /// Creates a new extension node with the given path and child.
    pub fn new_extension(key: Vec<u8>, next: Self) -> MptResult<Self> {
        if key.is_empty() {
            return Err(MptError::invalid("extension node requires non-empty key"));
        }

        Ok(Self {
            node_type: NodeType::ExtensionNode,
            reference: 1,
            hash: RwLock::new(None),
            children: Vec::new(),
            key,
            next: Some(Box::new(next)),
            value: Vec::new(),
        })
    }

    /// Creates a new leaf node with the supplied value.
    #[must_use] 
    pub const fn new_leaf(value: Vec<u8>) -> Self {
        Self {
            node_type: NodeType::LeafNode,
            reference: 1,
            hash: RwLock::new(None),
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value,
        }
    }

    /// Creates a new hash-only node.
    #[must_use] 
    pub const fn new_hash(hash: UInt256) -> Self {
        Self {
            node_type: NodeType::HashNode,
            reference: 0,
            hash: RwLock::new(Some(hash)),
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value: Vec::new(),
        }
    }

    /// Returns `true` if the node represents the empty sentinel.
    pub fn is_empty(&self) -> bool {
        self.node_type == NodeType::Empty
    }

    /// Marks the node as dirty causing its cached hash to be recomputed next time.
    pub fn set_dirty(&mut self) {
        *self.hash.write() = None;
    }

    /// Computes the node hash (Hash256 of the serialized payload without the reference).
    pub fn hash(&self) -> UInt256 {
        match self.try_hash() {
            Ok(hash) => hash,
            Err(err) => {
                error!(?err, "Failed to compute MPT node hash");
                UInt256::zero()
            }
        }
    }

    /// Attempts to compute the node hash, returning an error if serialization fails.
    pub fn try_hash(&self) -> MptResult<UInt256> {
        if let Some(hash) = *self.hash.read() {
            return Ok(hash);
        }

        let data = self.to_array_without_reference()?;
        let hash_bytes = Crypto::hash256(&data);
        let hash = UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?;
        *self.hash.write() = Some(hash);
        Ok(hash)
    }

    /// Returns the size of the serialized node in bytes.
    pub fn byte_size(&self) -> usize {
        let mut size = 1; // node type byte
        match self.node_type {
            NodeType::BranchNode => {
                size += self.branch_size();
                size += get_var_size(u64::from(self.reference));
            }
            NodeType::ExtensionNode => {
                size += self.extension_size();
                size += get_var_size(u64::from(self.reference));
            }
            NodeType::LeafNode => {
                size += self.leaf_size();
                size += get_var_size(u64::from(self.reference));
            }
            NodeType::HashNode => {
                size += self.hash_size();
            }
            NodeType::Empty => {}
        }
        size
    }

    /// Returns the serialized size when used as a child of another node.
    pub fn byte_size_as_child(&self) -> usize {
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                Self::new_hash(self.hash()).byte_size()
            }
            NodeType::HashNode | NodeType::Empty => self.byte_size(),
        }
    }

    /// Serializes the node without the `reference` field.
    pub fn to_array_without_reference(&self) -> MptResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize_without_reference(&mut writer)?;
        Ok(writer.into_bytes())
    }

    /// Serializes the node as a child according to the C# implementation rules.
    pub fn serialize_as_child(&self, writer: &mut BinaryWriter) -> MptResult<()> {
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                let hashed = Self::new_hash(self.hash());
                Serializable::serialize(&hashed, writer).map_err(MptError::from)
            }
            NodeType::HashNode | NodeType::Empty => {
                Serializable::serialize(self, writer).map_err(MptError::from)
            }
        }
    }

    /// Clones the node into the representation used while embedded inside another node.
    pub fn clone_as_child(&self) -> Self {
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                Self::new_hash(self.hash())
            }
            NodeType::HashNode | NodeType::Empty => self.clone(),
        }
    }

    fn branch_size(&self) -> usize {
        self.children.iter().map(Self::byte_size_as_child).sum()
    }

    fn extension_size(&self) -> usize {
        debug_assert!(
            self.next.is_some(),
            "extension node missing child during size computation"
        );
        let key_size = get_var_size(self.key.len() as u64) + self.key.len();
        key_size
            + self
                .next
                .as_ref()
                .map(|child| child.byte_size_as_child())
                .unwrap_or_default()
    }

    fn leaf_size(&self) -> usize {
        get_var_size(self.value.len() as u64) + self.value.len()
    }

    const fn hash_size(&self) -> usize {
        UINT256_SIZE
    }

    fn serialize_without_reference(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.node_type.to_byte())?;
        match self.node_type {
            NodeType::BranchNode => self.serialize_branch(writer)?,
            NodeType::ExtensionNode => self.serialize_extension(writer)?,
            NodeType::LeafNode => self.serialize_leaf(writer)?,
            NodeType::HashNode => self.serialize_hash(writer)?,
            NodeType::Empty => {}
        }
        Ok(())
    }

    fn serialize_branch(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        for child in &self.children {
            child
                .serialize_as_child(writer)
                .map_err(|e| IoError::invalid_data(e.to_string()))?;
        }
        Ok(())
    }

    fn serialize_extension(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        let Some(next) = self.next.as_ref() else {
            return Err(IoError::invalid_data(
                "extension node without child during serialization",
            ));
        };
        writer.write_var_bytes(&self.key)?;
        next.serialize_as_child(writer)
            .map_err(|e| IoError::invalid_data(e.to_string()))
    }

    fn serialize_leaf(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_var_bytes(&self.value)
    }

    fn serialize_hash(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        let Some(hash) = *self.hash.read() else {
            return Err(IoError::invalid_data("hash node without cached hash"));
        };
        writer.write_bytes(&hash.to_bytes())
    }

    fn deserialize_branch(reader: &mut MemoryReader) -> IoResult<Vec<Self>> {
        let mut children = Vec::with_capacity(BRANCH_CHILD_COUNT);
        for _ in 0..BRANCH_CHILD_COUNT {
            children.push(Self::deserialize(reader)?);
        }
        Ok(children)
    }

    fn deserialize_extension(reader: &mut MemoryReader) -> IoResult<(Vec<u8>, Self)> {
        let key = reader.read_var_bytes(MAX_KEY_LENGTH)?;
        let next = Self::deserialize(reader)?;
        Ok((key, next))
    }

    fn deserialize_leaf(reader: &mut MemoryReader) -> IoResult<Vec<u8>> {
        reader.read_var_bytes(MAX_VALUE_LENGTH)
    }

    fn deserialize_hash(reader: &mut MemoryReader) -> IoResult<UInt256> {
        let bytes = reader.read_bytes(UINT256_SIZE)?;
        UInt256::from_bytes(&bytes).map_err(|e| IoError::invalid_data(e.to_string()))
    }
}

impl Serializable for Node {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let node_type = NodeType::from_byte(reader.read_byte()?).map_err(IoError::invalid_data)?;
        match node_type {
            NodeType::BranchNode => {
                let children = Self::deserialize_branch(reader)?;
                let reference = reader.read_var_uint()? as u32;
                Ok(Self {
                    node_type,
                    reference,
                    hash: RwLock::new(None),
                    children,
                    key: Vec::new(),
                    next: None,
                    value: Vec::new(),
                })
            }
            NodeType::ExtensionNode => {
                let (key, next) = Self::deserialize_extension(reader)?;
                let reference = reader.read_var_uint()? as u32;
                Ok(Self {
                    node_type,
                    reference,
                    hash: RwLock::new(None),
                    children: Vec::new(),
                    key,
                    next: Some(Box::new(next)),
                    value: Vec::new(),
                })
            }
            NodeType::LeafNode => {
                let value = Self::deserialize_leaf(reader)?;
                let reference = reader.read_var_uint()? as u32;
                Ok(Self {
                    node_type,
                    reference,
                    hash: RwLock::new(None),
                    children: Vec::new(),
                    key: Vec::new(),
                    next: None,
                    value,
                })
            }
            NodeType::HashNode => {
                let hash = Self::deserialize_hash(reader)?;
                Ok(Self {
                    node_type,
                    reference: 0,
                    hash: RwLock::new(Some(hash)),
                    children: Vec::new(),
                    key: Vec::new(),
                    next: None,
                    value: Vec::new(),
                })
            }
            NodeType::Empty => Ok(Self::default()),
        }
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_without_reference(writer)?;
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                writer.write_var_uint(u64::from(self.reference))?;
            }
            NodeType::HashNode | NodeType::Empty => {}
        }
        Ok(())
    }

    fn size(&self) -> usize {
        self.byte_size()
    }
}
