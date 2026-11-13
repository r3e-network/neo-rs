use super::error::{MptError, MptResult};
use super::node_type::NodeType;
use super::profile::{MptProfile, MptProfileStage, MptProfileTimer};
use crate::Crypto;
use neo_io::serializable::helper::{get_var_size, get_var_size_bytes};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::mem;
use std::sync::{Arc, LazyLock};
use tracing::error;

static EMPTY_NODE_ARC: LazyLock<Arc<Node>> = LazyLock::new(|| Arc::new(Node::new()));

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
///
/// Uses `Arc<Node>` for children to enable structural sharing and reduce cloning overhead.
#[derive(Debug)]
pub struct Node {
    /// The type of this trie node (branch, extension, leaf, hash, or empty).
    pub node_type: NodeType,
    /// Reference count tracking how many parents point to this node.
    pub reference: u32,
    /// Whether this node has been created or mutated in memory and not yet flushed to `MptCache`.
    dirty: bool,
    hash: RwLock<Option<UInt256>>,
    /// Body serialized without the reference suffix. Filled by `try_hash` and
    /// consumed when the node is written, so commit does not walk the node again.
    hash_preimage: RwLock<Option<Vec<u8>>>,
    /// Children for branch nodes - stored as Arc for structural sharing
    pub children: Vec<Arc<Self>>,
    /// Key for extension nodes
    pub key: Vec<u8>,
    /// Next node for extension nodes - stored as Arc for structural sharing
    pub next: Option<Arc<Self>>,
    /// Stored value for leaf nodes.
    pub value: Vec<u8>,
}

impl Default for Node {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Node {
    /// Clone the node with structural sharing of subtrees.
    ///
    /// Instead of deep-cloning children, this clones the Arc pointers,
    /// enabling O(1) subtree sharing while the data itself remains immutable.
    fn clone(&self) -> Self {
        let cached_hash = *self.hash.read();
        let cached_preimage = self.hash_preimage.read();
        match self.node_type {
            NodeType::BranchNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                dirty: self.dirty,
                hash: RwLock::new(cached_hash),
                hash_preimage: RwLock::new(cached_preimage.clone()),
                children: self.children.iter().map(Arc::clone).collect(),
                key: Vec::new(),
                next: None,
                value: Vec::new(),
            },
            NodeType::ExtensionNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                dirty: self.dirty,
                hash: RwLock::new(cached_hash),
                hash_preimage: RwLock::new(cached_preimage.clone()),
                children: Vec::new(),
                key: self.key.clone(),
                next: self.next.as_ref().map(Arc::clone),
                value: Vec::new(),
            },
            NodeType::LeafNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                dirty: self.dirty,
                hash: RwLock::new(cached_hash),
                hash_preimage: RwLock::new(cached_preimage.clone()),
                children: Vec::new(),
                key: self.key.clone(),
                next: None,
                value: self.value.clone(),
            },
            NodeType::HashNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                dirty: false,
                hash: RwLock::new(cached_hash),
                hash_preimage: RwLock::new(cached_preimage.clone()),
                children: Vec::new(),
                key: Vec::new(),
                next: None,
                value: Vec::new(),
            },
            NodeType::Empty => Self {
                node_type: self.node_type,
                reference: self.reference,
                dirty: false,
                hash: RwLock::new(None),
                hash_preimage: RwLock::new(None),
                children: Vec::new(),
                key: Vec::new(),
                next: None,
                value: Vec::new(),
            },
        }
    }
}

impl Node {
    /// Creates an empty node.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            node_type: NodeType::Empty,
            reference: 0,
            dirty: false,
            hash: RwLock::new(None),
            hash_preimage: RwLock::new(None),
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value: Vec::new(),
        }
    }

    /// Returns a shared `Arc<Node>` pointing to the immutable empty node sentinel.
    #[inline]
    #[must_use]
    pub fn empty_arc() -> Arc<Self> {
        Arc::clone(&EMPTY_NODE_ARC)
    }

    /// Creates a new branch node with default children.
    #[must_use]
    pub fn new_branch() -> Self {
        Self {
            node_type: NodeType::BranchNode,
            reference: 1,
            dirty: true,
            hash: RwLock::new(None),
            hash_preimage: RwLock::new(None),
            children: vec![Self::empty_arc(); BRANCH_CHILD_COUNT],
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
            dirty: true,
            hash: RwLock::new(None),
            hash_preimage: RwLock::new(None),
            children: Vec::new(),
            key,
            next: Some(Arc::new(next)),
            value: Vec::new(),
        })
    }

    /// Creates a new leaf node with the supplied value.
    #[must_use]
    pub const fn new_leaf(value: Vec<u8>) -> Self {
        Self {
            node_type: NodeType::LeafNode,
            reference: 1,
            dirty: true,
            hash: RwLock::new(None),
            hash_preimage: RwLock::new(None),
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
            dirty: false,
            hash: RwLock::new(Some(hash)),
            hash_preimage: RwLock::new(None),
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value: Vec::new(),
        }
    }

    /// Returns `true` if the node represents the empty sentinel.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.node_type == NodeType::Empty
    }

    /// Returns `true` if this node has not yet been flushed into `MptCache`.
    #[inline]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clears the uncommitted dirty flag once the node has been registered in `MptCache`.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Sets the known hash on a node (e.g. immediately after deserializing by hash key).
    #[inline]
    pub fn set_cached_hash(&self, hash: UInt256) {
        *self.hash.write() = Some(hash);
    }

    /// Clears the cached hash without changing the dirty flag.
    #[inline]
    pub fn clear_cached_hash(&self) {
        *self.hash.write() = None;
    }

    /// Marks the node as dirty causing its cached hash to be recomputed next time.
    #[inline]
    pub fn set_dirty(&mut self) {
        self.dirty = true;
        *self.hash.write() = None;
        *self.hash_preimage.write() = None;
    }

    /// Removes the cached hash body so the caller can attach it to another node.
    pub(crate) fn take_hash_preimage(&self) -> Option<Vec<u8>> {
        self.hash_preimage.write().take()
    }

    /// Restores a hash body previously removed with [`Self::take_hash_preimage`].
    pub(crate) fn set_hash_preimage(&self, bytes: Vec<u8>) {
        *self.hash_preimage.write() = Some(bytes);
    }

    /// Returns the bytes that are written to the store for this node.
    ///
    /// When `try_hash` has kept the body, that body is reused and the current
    /// reference count is appended. The kept body is consumed. A node without
    /// a kept body is serialized from scratch.
    pub(crate) fn stored_bytes(&self) -> MptResult<Vec<u8>> {
        if let Some(preimage) = self.hash_preimage.write().take() {
            return Ok(append_reference(preimage, self.node_type, self.reference));
        }
        let mut writer = BinaryWriter::with_capacity(self.byte_size());
        self.serialize(&mut writer).map_err(MptError::from)?;
        Ok(writer.into_bytes())
    }

    /// Converts a committed node into the compact representation stored in `GlobalNodeCache`,
    /// where all non-empty children are `HashNode`s and the node's own hash is pre-cached.
    #[must_use]
    pub fn to_cached_node(&self, hash: UInt256) -> Self {
        match self.node_type {
            NodeType::BranchNode => Self {
                node_type: NodeType::BranchNode,
                reference: self.reference,
                dirty: false,
                hash: RwLock::new(Some(hash)),
                hash_preimage: RwLock::new(None),
                children: self
                    .children
                    .iter()
                    .map(|c| match c.node_type {
                        NodeType::Empty => Self::empty_arc(),
                        NodeType::HashNode => Arc::clone(c),
                        _ => Arc::new(Self::new_hash(c.hash())),
                    })
                    .collect(),
                key: Vec::new(),
                next: None,
                value: Vec::new(),
            },
            NodeType::ExtensionNode => Self {
                node_type: NodeType::ExtensionNode,
                reference: self.reference,
                dirty: false,
                hash: RwLock::new(Some(hash)),
                hash_preimage: RwLock::new(None),
                children: Vec::new(),
                key: self.key.clone(),
                next: self.next.as_ref().map(|n| match n.node_type {
                    NodeType::Empty => Self::empty_arc(),
                    NodeType::HashNode => Arc::clone(n),
                    _ => Arc::new(Self::new_hash(n.hash())),
                }),
                value: Vec::new(),
            },
            NodeType::LeafNode => Self {
                node_type: NodeType::LeafNode,
                reference: self.reference,
                dirty: false,
                hash: RwLock::new(Some(hash)),
                hash_preimage: RwLock::new(None),
                children: Vec::new(),
                key: Vec::new(),
                next: None,
                value: self.value.clone(),
            },
            NodeType::HashNode => Self::new_hash(hash),
            NodeType::Empty => Self::new(),
        }
    }

    /// Returns `(nodes, estimated_bytes)` for resolved nodes retained by this root.
    ///
    /// Hash-only and empty sentinels are excluded. Shared subtrees are counted
    /// once; the estimate includes owned buffers but not allocator or lock overhead.
    #[must_use]
    pub fn resolved_retention_stats(&self) -> (usize, usize) {
        let mut visited = HashSet::<*const Self>::new();
        let mut pending = vec![self];
        let mut nodes = 0usize;
        let mut bytes = 0usize;
        while let Some(node) = pending.pop() {
            if matches!(node.node_type, NodeType::HashNode | NodeType::Empty)
                || !visited.insert(node)
            {
                continue;
            }
            nodes += 1;
            bytes = bytes
                .saturating_add(mem::size_of::<Self>())
                .saturating_add(node.key.capacity())
                .saturating_add(node.value.capacity())
                .saturating_add(node.children.capacity() * mem::size_of::<Arc<Self>>())
                .saturating_add(node.hash_preimage.read().as_ref().map_or(0, Vec::capacity));
            match node.node_type {
                NodeType::BranchNode => pending.extend(node.children.iter().map(Arc::as_ref)),
                NodeType::ExtensionNode => {
                    if let Some(ref next) = node.next {
                        pending.push(next.as_ref());
                    }
                }
                _ => {}
            }
        }
        (nodes, bytes)
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
        self.try_hash_profiled(None)
    }

    /// Computes a node hash while optionally observing serialization and Hash256.
    pub(crate) fn try_hash_profiled(&self, profile: Option<&MptProfile>) -> MptResult<UInt256> {
        if let Some(hash) = *self.hash.read() {
            return Ok(hash);
        }

        let data = {
            let _timer = MptProfileTimer::new(profile, MptProfileStage::Serialization);
            self.to_array_without_reference_profiled(profile)?
        };
        let hash_bytes = {
            let _timer = MptProfileTimer::new(profile, MptProfileStage::Hash256);
            Crypto::hash256(&data)
        };
        let hash = UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?;
        *self.hash.write() = Some(hash);
        *self.hash_preimage.write() = Some(data);
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
    #[inline]
    pub fn byte_size_as_child(&self) -> usize {
        match self.node_type {
            NodeType::BranchNode
            | NodeType::ExtensionNode
            | NodeType::LeafNode
            | NodeType::HashNode => 1 + UINT256_SIZE,
            NodeType::Empty => 1,
        }
    }

    /// Serializes the node without the `reference` field.
    pub fn to_array_without_reference(&self) -> MptResult<Vec<u8>> {
        self.to_array_without_reference_profiled(None)
    }

    fn to_array_without_reference_profiled(
        &self,
        profile: Option<&MptProfile>,
    ) -> MptResult<Vec<u8>> {
        if self.node_type == NodeType::BranchNode {
            // A branch has at most seventeen hash-encoded children. Encode in
            // one pass instead of walking all children once for byte_size and
            // again through BinaryWriter. Invalid hash nodes retain the
            // canonical writer path and its original error behaviour.
            let mut data = Vec::with_capacity(1 + BRANCH_CHILD_COUNT * (1 + UINT256_SIZE));
            data.push(NodeType::BranchNode.to_byte());
            for child in &self.children {
                match child.node_type {
                    NodeType::Empty => data.push(NodeType::Empty.to_byte()),
                    NodeType::HashNode => {
                        let Some(hash) = *child.hash.read() else {
                            let mut writer = BinaryWriter::with_capacity(self.byte_size());
                            self.serialize_without_reference_profiled(&mut writer, profile)?;
                            return Ok(writer.into_bytes());
                        };
                        data.push(NodeType::HashNode.to_byte());
                        data.extend_from_slice(hash.as_ref());
                    }
                    NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                        let hash = child.try_hash_profiled(profile)?;
                        data.push(NodeType::HashNode.to_byte());
                        data.extend_from_slice(hash.as_ref());
                    }
                }
            }
            return Ok(data);
        }

        let mut writer = BinaryWriter::with_capacity(self.byte_size());
        self.serialize_without_reference_profiled(&mut writer, profile)?;
        Ok(writer.into_bytes())
    }

    /// Serializes the node as a child according to the C# implementation rules.
    pub fn serialize_as_child(&self, writer: &mut BinaryWriter) -> MptResult<()> {
        self.serialize_as_child_profiled(writer, None)
    }

    pub(crate) fn serialize_as_child_profiled(
        &self,
        writer: &mut BinaryWriter,
        profile: Option<&MptProfile>,
    ) -> MptResult<()> {
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                let hash = self.try_hash_profiled(profile)?;
                writer.write_u8(NodeType::HashNode.to_byte())?;
                writer.write_bytes(hash.as_ref()).map_err(MptError::from)
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

    /// Gets a mutable reference to a child node, cloning from Arc if necessary.
    ///
    /// This implements copy-on-write semantics for efficient updates.
    pub fn get_child_mut(&mut self, index: usize) -> Option<&mut Self> {
        if index >= self.children.len() {
            return None;
        }
        // Arc::make_mut clones the inner data only if the Arc is shared
        Some(Arc::make_mut(&mut self.children[index]))
    }

    /// Gets a mutable reference to the next node, cloning from Arc if necessary.
    ///
    /// This implements copy-on-write semantics for efficient updates.
    pub fn get_next_mut(&mut self) -> Option<&mut Self> {
        self.next.as_mut().map(Arc::make_mut)
    }

    /// Sets a child node at the given index.
    pub fn set_child(&mut self, index: usize, child: Self) {
        if index < self.children.len() {
            self.children[index] = if child.is_empty() {
                Self::empty_arc()
            } else {
                Arc::new(child)
            };
            self.set_dirty();
        }
    }

    /// Takes the next node from an extension node.
    pub fn take_next(&mut self) -> Option<Self> {
        self.next.take().map(|arc| match Arc::try_unwrap(arc) {
            Ok(node) => node,
            Err(arc) => (*arc).clone(),
        })
    }

    fn branch_size(&self) -> usize {
        self.children
            .iter()
            .map(|c| Self::byte_size_as_child(c))
            .sum()
    }

    fn extension_size(&self) -> usize {
        debug_assert!(
            self.next.is_some(),
            "extension node missing child during size computation"
        );
        let key_size = get_var_size_bytes(&self.key);
        key_size
            + self
                .next
                .as_ref()
                .map(|child| child.byte_size_as_child())
                .unwrap_or_default()
    }

    fn leaf_size(&self) -> usize {
        get_var_size_bytes(&self.value)
    }

    const fn hash_size(&self) -> usize {
        UINT256_SIZE
    }

    fn serialize_without_reference(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_without_reference_profiled(writer, None)
    }

    fn serialize_without_reference_profiled(
        &self,
        writer: &mut BinaryWriter,
        profile: Option<&MptProfile>,
    ) -> IoResult<()> {
        writer.write_u8(self.node_type.to_byte())?;
        match self.node_type {
            NodeType::BranchNode => self.serialize_branch_profiled(writer, profile)?,
            NodeType::ExtensionNode => self.serialize_extension_profiled(writer, profile)?,
            NodeType::LeafNode => self.serialize_leaf(writer)?,
            NodeType::HashNode => self.serialize_hash(writer)?,
            NodeType::Empty => {}
        }
        Ok(())
    }

    fn serialize_branch_profiled(
        &self,
        writer: &mut BinaryWriter,
        profile: Option<&MptProfile>,
    ) -> IoResult<()> {
        for child in &self.children {
            child
                .serialize_as_child_profiled(writer, profile)
                .map_err(|e| IoError::invalid_data(e.to_string()))?;
        }
        Ok(())
    }

    fn serialize_extension_profiled(
        &self,
        writer: &mut BinaryWriter,
        profile: Option<&MptProfile>,
    ) -> IoResult<()> {
        let Some(next) = self.next.as_ref() else {
            return Err(IoError::invalid_data(
                "extension node without child during serialization",
            ));
        };
        writer.write_var_bytes(&self.key)?;
        next.serialize_as_child_profiled(writer, profile)
            .map_err(|e| IoError::invalid_data(e.to_string()))
    }

    fn serialize_leaf(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        // C# LeafNode serialization writes only the value.
        // The path to the leaf is encoded in the trie structure
        // (extension keys + branch positions), NOT in the leaf itself.
        writer.write_var_bytes(&self.value)
    }

    fn serialize_hash(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        let Some(hash) = *self.hash.read() else {
            return Err(IoError::invalid_data("hash node without cached hash"));
        };
        writer.write_bytes(hash.as_ref())
    }

    fn deserialize_branch(reader: &mut MemoryReader) -> IoResult<Vec<Arc<Self>>> {
        let mut children = Vec::with_capacity(BRANCH_CHILD_COUNT);
        for _ in 0..BRANCH_CHILD_COUNT {
            let child = Self::deserialize(reader)?;
            if child.is_empty() {
                children.push(Self::empty_arc());
            } else {
                children.push(Arc::new(child));
            }
        }
        Ok(children)
    }

    fn deserialize_extension(reader: &mut MemoryReader) -> IoResult<(Vec<u8>, Arc<Self>)> {
        let key = reader.read_var_bytes(MAX_KEY_LENGTH)?;
        let child = Self::deserialize(reader)?;
        let next = if child.is_empty() {
            Self::empty_arc()
        } else {
            Arc::new(child)
        };
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
                    dirty: false,
                    hash: RwLock::new(None),
                    hash_preimage: RwLock::new(None),
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
                    dirty: false,
                    hash: RwLock::new(None),
                    hash_preimage: RwLock::new(None),
                    children: Vec::new(),
                    key,
                    next: Some(next),
                    value: Vec::new(),
                })
            }
            NodeType::LeafNode => {
                let value = Self::deserialize_leaf(reader)?;
                let reference = reader.read_var_uint()? as u32;
                Ok(Self {
                    node_type,
                    reference,
                    dirty: false,
                    hash: RwLock::new(None),
                    hash_preimage: RwLock::new(None),
                    children: Vec::new(),
                    key: Vec::new(),
                    next: None,
                    value,
                })
            }
            NodeType::HashNode => {
                let hash = Self::deserialize_hash(reader)?;
                Ok(Self::new_hash(hash))
            }
            NodeType::Empty => Ok(Self::new()),
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

fn append_reference(mut body: Vec<u8>, node_type: NodeType, reference: u32) -> Vec<u8> {
    if matches!(
        node_type,
        NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode
    ) {
        neo_io::var_int::write_var_int(u64::from(reference), &mut body);
    }
    body
}
