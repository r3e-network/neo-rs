use super::error::{MptError, MptResult};
use super::node_type::NodeType;
use crate::Crypto;
use neo_io::serializable::helper::SerializeHelper;
use neo_io::var_int::VarInt;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use std::mem;
use std::sync::{Arc, OnceLock};
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
///
/// Uses `Arc<Node>` for children to enable structural sharing and reduce cloning overhead.
#[derive(Debug)]
pub struct Node {
    /// The type of this trie node (branch, extension, leaf, hash, or empty).
    pub node_type: NodeType,
    /// Reference count tracking how many parents point to this node.
    ///
    /// Neo's MPT implementation stores this as a C# `int`. Keeping the signed
    /// domain is consensus-relevant at decode and overflow boundaries.
    pub reference: i32,
    hash: OnceLock<UInt256>,
    accounted_hash: Option<UInt256>,
    dirty: bool,
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
        Self {
            node_type: NodeType::Empty,
            reference: 0,
            hash: OnceLock::new(),
            accounted_hash: None,
            dirty: false,
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value: Vec::new(),
        }
    }
}

impl Clone for Node {
    /// Clone the node using the C# MPT representation: embedded branch,
    /// extension, and leaf children are replaced by hash-only child nodes.
    fn clone(&self) -> Self {
        match self.node_type {
            NodeType::BranchNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                hash: OnceLock::new(),
                accounted_hash: self.accounted_hash,
                dirty: self.dirty,
                children: self.children.iter().map(Self::clone_arc_as_child).collect(),
                key: Vec::new(),
                next: None,
                value: Vec::new(),
            },
            NodeType::ExtensionNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                hash: OnceLock::new(),
                accounted_hash: self.accounted_hash,
                dirty: self.dirty,
                children: Vec::new(),
                key: self.key.clone(),
                next: self.next.as_ref().map(Self::clone_arc_as_child),
                value: Vec::new(),
            },
            NodeType::LeafNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                hash: OnceLock::new(),
                accounted_hash: self.accounted_hash,
                dirty: self.dirty,
                children: Vec::new(),
                key: Vec::new(),
                next: None,
                value: self.value.clone(),
            },
            NodeType::HashNode => Self {
                node_type: self.node_type,
                reference: self.reference,
                hash: self.cloned_hash_cache(),
                accounted_hash: None,
                dirty: false,
                children: Vec::new(),
                key: Vec::new(),
                next: None,
                value: Vec::new(),
            },
            NodeType::Empty => Self {
                node_type: self.node_type,
                reference: self.reference,
                hash: OnceLock::new(),
                accounted_hash: None,
                dirty: false,
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new branch node with default children.
    #[must_use]
    pub fn new_branch() -> Self {
        Self {
            node_type: NodeType::BranchNode,
            reference: 1,
            hash: OnceLock::new(),
            accounted_hash: None,
            dirty: true,
            children: (0..BRANCH_CHILD_COUNT)
                .map(|_| Arc::new(Self::new()))
                .collect(),
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
            hash: OnceLock::new(),
            accounted_hash: None,
            dirty: true,
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
            hash: OnceLock::new(),
            accounted_hash: None,
            dirty: true,
            children: Vec::new(),
            key: Vec::new(),
            next: None,
            value,
        }
    }

    /// Creates a new hash-only node.
    #[must_use]
    pub fn new_hash(hash: UInt256) -> Self {
        Self {
            node_type: NodeType::HashNode,
            reference: 0,
            hash: OnceLock::from(hash),
            accounted_hash: None,
            dirty: false,
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
        self.hash = OnceLock::new();
        self.accounted_hash = None;
        self.dirty = true;
    }

    pub(crate) const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(crate) fn cached_hash(&self) -> Option<UInt256> {
        self.hash.get().copied()
    }

    pub(crate) const fn accounted_hash(&self) -> Option<UInt256> {
        self.accounted_hash
    }

    #[cfg(test)]
    pub(crate) fn hash_is_cached(&self) -> bool {
        self.hash.get().is_some()
    }

    pub(crate) fn set_finalized_hash(&mut self, hash: UInt256) {
        self.hash = OnceLock::from(hash);
        self.accounted_hash = Some(hash);
        self.dirty = false;
    }

    pub(crate) fn set_pending_hash(&mut self, hash: UInt256) {
        self.hash = OnceLock::from(hash);
    }

    pub(crate) fn set_accounted_hash(&mut self, hash: UInt256) {
        self.accounted_hash = Some(hash);
        self.dirty = false;
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
        if let Some(hash) = self.hash.get() {
            return Ok(*hash);
        }

        let data = self.to_array_without_reference()?;
        super::metrics::record_hash_computation();
        let hash_bytes = Crypto::hash256(&data);
        let hash = UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?;
        let _ = self.hash.set(hash);
        Ok(hash)
    }

    /// Returns the size of the serialized node in bytes.
    pub fn byte_size(&self) -> usize {
        let mut size = 1; // node type byte
        match self.node_type {
            NodeType::BranchNode => {
                size += self.branch_size();
                size += Self::reference_var_size(self.reference);
            }
            NodeType::ExtensionNode => {
                size += self.extension_size();
                size += Self::reference_var_size(self.reference);
            }
            NodeType::LeafNode => {
                size += self.leaf_size();
                size += Self::reference_var_size(self.reference);
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
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => 1 + UINT256_SIZE,
            NodeType::HashNode | NodeType::Empty => self.byte_size(),
        }
    }

    /// Serializes the node without the `reference` field.
    pub fn to_array_without_reference(&self) -> MptResult<Vec<u8>> {
        let mut writer = BinaryWriter::with_capacity(self.byte_size_without_reference());
        self.serialize_without_reference(&mut writer)?;
        Ok(writer.into_bytes())
    }

    /// Serializes a previously computed no-reference payload with the supplied
    /// reference count appended when the node kind stores references.
    ///
    /// The no-reference payload is also the hash preimage. MPT cache commit can
    /// reuse it after staging a dirty node instead of walking the same subtree
    /// again only to write identical node bytes.
    pub(crate) fn array_from_payload_parts(
        node_type: NodeType,
        reference: i32,
        payload_without_reference: &[u8],
    ) -> MptResult<Vec<u8>> {
        let reference_size = match node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                Self::reference_var_size(reference)
            }
            NodeType::HashNode | NodeType::Empty => 0,
        };
        let mut writer =
            BinaryWriter::with_capacity(payload_without_reference.len() + reference_size);
        writer
            .write_bytes(payload_without_reference)
            .map_err(MptError::from)?;
        match node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                let reference = u64::try_from(reference).map_err(|_| {
                    MptError::invalid("MPT node reference count cannot be negative")
                })?;
                writer.write_var_uint(reference).map_err(MptError::from)?;
            }
            NodeType::HashNode | NodeType::Empty => {}
        }
        Ok(writer.into_bytes())
    }

    pub(crate) fn array_from_payload_parts_owned(
        node_type: NodeType,
        reference: i32,
        mut payload_without_reference: Vec<u8>,
    ) -> MptResult<Vec<u8>> {
        if matches!(
            node_type,
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode
        ) {
            let reference = u64::try_from(reference)
                .map_err(|_| MptError::invalid("MPT node reference count cannot be negative"))?;
            payload_without_reference.reserve(VarInt::encoded_len(reference));
            VarInt::write_var_int(reference, &mut payload_without_reference);
        }
        Ok(payload_without_reference)
    }

    /// Splits a serialized node into its type, reference count, and raw hash
    /// payload without materializing its child graph.
    ///
    /// Full-state deferred finalization already has the serialized backing
    /// bytes. This parser validates the same structural bounds as
    /// [`Node::deserialize`] while retaining the payload bytes verbatim, so a
    /// reference update does not need to allocate and reserialize every child.
    pub fn split_serialized_reference(bytes: &[u8]) -> MptResult<(NodeType, i32, Vec<u8>)> {
        let (node_type, reference, payload_len) = Self::serialized_reference_parts(bytes)?;
        Ok((node_type, reference, bytes[..payload_len].to_vec()))
    }

    /// Splits a serialized node while reusing the owned input allocation for
    /// its no-reference payload.
    pub fn split_serialized_reference_owned(
        mut bytes: Vec<u8>,
    ) -> MptResult<(NodeType, i32, Vec<u8>)> {
        let (node_type, reference, payload_len) = Self::serialized_reference_parts(&bytes)?;
        bytes.truncate(payload_len);
        Ok((node_type, reference, bytes))
    }

    /// Validates one canonical node row from the persisted MPT namespace.
    ///
    /// Persisted rows are content-addressed by the hash of their serialized
    /// payload without the reference count. Unlike embedded child nodes, a row
    /// must contain a materialized branch, extension, or leaf node. This path
    /// intentionally avoids allocating a [`Node`], making it suitable for full
    /// pack and checkpoint scrubs.
    pub fn validate_persisted(bytes: &[u8], expected_hash: UInt256) -> MptResult<()> {
        let payload_len = Self::persisted_payload_len(bytes)?;
        let actual_hash = UInt256::from_bytes(&Crypto::hash256(&bytes[..payload_len]))?;
        if actual_hash != expected_hash {
            return Err(MptError::invalid(
                "persisted MPT node hash does not match its storage key",
            ));
        }
        Ok(())
    }

    /// Decodes a canonical persisted MPT row after validating its storage key.
    ///
    /// Use [`Node::validate_persisted`] for bulk scrubs that do not need to
    /// inspect child hashes. This method materializes the bounded node object
    /// needed by root-graph traversal and proof validation.
    pub fn deserialize_persisted(bytes: &[u8], expected_hash: UInt256) -> MptResult<Self> {
        Self::validate_persisted(bytes, expected_hash)?;
        let mut reader = MemoryReader::new(bytes);
        let node = Self::deserialize(&mut reader).map_err(MptError::from)?;
        debug_assert_eq!(reader.remaining(), 0);
        Ok(node)
    }

    fn persisted_payload_len(bytes: &[u8]) -> MptResult<usize> {
        let mut reader = MemoryReader::new(bytes);
        let node_type = Self::read_node_type(&mut reader)?;
        match node_type {
            NodeType::BranchNode => {
                for _ in 0..BRANCH_CHILD_COUNT {
                    Self::skip_persisted_child(&mut reader)?;
                }
            }
            NodeType::ExtensionNode => {
                Self::read_canonical_var_memory(&mut reader, MAX_KEY_LENGTH)?;
                Self::skip_persisted_child(&mut reader)?;
            }
            NodeType::LeafNode => {
                Self::read_canonical_var_memory(&mut reader, MAX_VALUE_LENGTH)?;
            }
            NodeType::HashNode | NodeType::Empty => {
                return Err(MptError::invalid(
                    "persisted MPT row must contain a materialized node",
                ));
            }
        }

        let payload_len = reader.position();
        let reference_start = reader.position();
        let reference = reader.read_var_uint().map_err(MptError::from)?;
        if reference == 0 || reference > i32::MAX as u64 {
            return Err(MptError::invalid(
                "persisted MPT node reference count is outside the positive int32 domain",
            ));
        }
        if reader.position() - reference_start != VarInt::encoded_len(reference) {
            return Err(MptError::invalid(
                "persisted MPT node reference count is not canonically encoded",
            ));
        }
        if reader.remaining() != 0 {
            return Err(MptError::invalid(
                "persisted MPT node contains trailing bytes",
            ));
        }
        Ok(payload_len)
    }

    fn read_canonical_var_memory(reader: &mut MemoryReader<'_>, maximum: usize) -> MptResult<()> {
        let start = reader.position();
        let length = reader
            .read_var_memory(maximum)
            .map_err(MptError::from)?
            .len();
        let encoded_width = reader.position() - start - length;
        if encoded_width != VarInt::encoded_len(length as u64) {
            return Err(MptError::invalid(
                "persisted MPT node length is not canonically encoded",
            ));
        }
        Ok(())
    }

    fn skip_persisted_child(reader: &mut MemoryReader<'_>) -> MptResult<()> {
        match Self::read_node_type(reader)? {
            NodeType::HashNode => {
                reader.read_memory(UINT256_SIZE).map_err(MptError::from)?;
            }
            NodeType::Empty => {}
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                return Err(MptError::invalid(
                    "persisted MPT child is not canonically hash-addressed",
                ));
            }
        }
        Ok(())
    }

    fn serialized_reference_parts(bytes: &[u8]) -> MptResult<(NodeType, i32, usize)> {
        let mut reader = MemoryReader::new(bytes);
        let node_type = Self::read_node_type(&mut reader)?;
        Self::skip_node_payload(&mut reader, node_type, 0)?;
        let reference_start = reader.position();
        let reference = match node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                reader.read_var_uint().map_err(MptError::from)? as i32
            }
            NodeType::HashNode | NodeType::Empty => 0,
        };
        if reader.remaining() != 0 {
            return Err(MptError::invalid(
                "MPT serialized node contains trailing bytes",
            ));
        }
        Ok((node_type, reference, reference_start))
    }

    fn read_node_type(reader: &mut MemoryReader<'_>) -> MptResult<NodeType> {
        NodeType::from_byte(reader.read_byte().map_err(MptError::from)?)
            .map_err(|error| MptError::invalid(error.to_string()))
    }

    fn skip_serialized_node(reader: &mut MemoryReader<'_>, depth: usize) -> MptResult<()> {
        if depth > MAX_KEY_LENGTH {
            return Err(MptError::invalid(
                "MPT node nesting depth exceeds the maximum allowed limit",
            ));
        }
        let node_type = Self::read_node_type(reader)?;
        Self::skip_node_payload(reader, node_type, depth)?;
        if matches!(
            node_type,
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode
        ) {
            reader.read_var_uint().map_err(MptError::from)?;
        }
        Ok(())
    }

    fn skip_node_payload(
        reader: &mut MemoryReader<'_>,
        node_type: NodeType,
        depth: usize,
    ) -> MptResult<()> {
        match node_type {
            NodeType::BranchNode => {
                for _ in 0..BRANCH_CHILD_COUNT {
                    Self::skip_serialized_node(reader, depth + 1)?;
                }
            }
            NodeType::ExtensionNode => {
                reader
                    .read_var_memory(MAX_KEY_LENGTH)
                    .map_err(MptError::from)?;
                Self::skip_serialized_node(reader, depth + 1)?;
            }
            NodeType::LeafNode => {
                reader
                    .read_var_memory(MAX_VALUE_LENGTH)
                    .map_err(MptError::from)?;
            }
            NodeType::HashNode => {
                reader.read_memory(UINT256_SIZE).map_err(MptError::from)?;
            }
            NodeType::Empty => {}
        }
        Ok(())
    }

    fn reference_var_size(reference: i32) -> usize {
        if reference < 0 {
            1
        } else {
            SerializeHelper::get_var_size(reference as u64)
        }
    }

    /// Serializes the node as a child according to the C# implementation rules.
    pub fn serialize_as_child(&self, writer: &mut BinaryWriter) -> MptResult<()> {
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                writer
                    .write_u8(NodeType::HashNode.to_byte())
                    .map_err(MptError::from)?;
                let hash = self.hash();
                writer.write_bytes(&hash.to_array()).map_err(MptError::from)
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

    /// Takes a shallow snapshot for read-only traversal without replacing
    /// materialized descendants with hash-only nodes.
    pub(crate) fn clone_for_traversal(&self) -> Self {
        Self {
            node_type: self.node_type,
            reference: self.reference,
            hash: self.cloned_hash_cache(),
            accounted_hash: self.accounted_hash,
            dirty: self.dirty,
            children: self.children.clone(),
            key: self.key.clone(),
            next: self.next.clone(),
            value: self.value.clone(),
        }
    }

    fn clone_arc_as_child(child: &Arc<Self>) -> Arc<Self> {
        match child.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                Arc::new(Self::new_hash(child.hash()))
            }
            NodeType::HashNode | NodeType::Empty => Arc::clone(child),
        }
    }

    fn cloned_hash_cache(&self) -> OnceLock<UInt256> {
        self.hash
            .get()
            .copied()
            .map_or_else(OnceLock::new, OnceLock::from)
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
            self.children[index] = Arc::new(child);
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

    fn byte_size_without_reference(&self) -> usize {
        let mut size = 1; // node type byte
        match self.node_type {
            NodeType::BranchNode => size += self.branch_size(),
            NodeType::ExtensionNode => size += self.extension_size(),
            NodeType::LeafNode => size += self.leaf_size(),
            NodeType::HashNode => size += self.hash_size(),
            NodeType::Empty => {}
        }
        size
    }

    fn extension_size(&self) -> usize {
        debug_assert!(
            self.next.is_some(),
            "extension node missing child during size computation"
        );
        let key_size = SerializeHelper::get_var_size_bytes(&self.key);
        key_size
            + self
                .next
                .as_ref()
                .map(|child| child.byte_size_as_child())
                .unwrap_or_default()
    }

    fn leaf_size(&self) -> usize {
        SerializeHelper::get_var_size_bytes(&self.value)
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
        // C# LeafNode serialization writes only the value.
        // The path to the leaf is encoded in the trie structure
        // (extension keys + branch positions), NOT in the leaf itself.
        writer.write_var_bytes(&self.value)
    }

    fn serialize_hash(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        let Some(hash) = self.hash.get() else {
            return Err(IoError::invalid_data("hash node without cached hash"));
        };
        writer.write_bytes(&hash.to_array())
    }

    fn deserialize_branch(reader: &mut MemoryReader, depth: usize) -> IoResult<Vec<Arc<Self>>> {
        let mut children = Vec::with_capacity(BRANCH_CHILD_COUNT);
        for _ in 0..BRANCH_CHILD_COUNT {
            children.push(Arc::new(Self::deserialize_with_depth(reader, depth + 1)?));
        }
        Ok(children)
    }

    fn deserialize_extension(
        reader: &mut MemoryReader,
        depth: usize,
    ) -> IoResult<(Vec<u8>, Arc<Self>)> {
        let key = reader.read_var_bytes(MAX_KEY_LENGTH)?;
        let next = Arc::new(Self::deserialize_with_depth(reader, depth + 1)?);
        Ok((key, next))
    }

    fn deserialize_leaf(reader: &mut MemoryReader) -> IoResult<Vec<u8>> {
        reader.read_var_bytes(MAX_VALUE_LENGTH)
    }

    fn deserialize_hash(reader: &mut MemoryReader) -> IoResult<UInt256> {
        let bytes = reader.read_bytes(UINT256_SIZE)?;
        UInt256::from_bytes(&bytes).map_err(|e| IoError::invalid_data(e.to_string()))
    }

    fn deserialize_with_depth(reader: &mut MemoryReader, depth: usize) -> IoResult<Self> {
        if depth > MAX_KEY_LENGTH {
            return Err(IoError::invalid_data(
                "MPT node nesting depth exceeds the maximum allowed limit",
            ));
        }

        let node_type = NodeType::from_byte(reader.read_byte()?)
            .map_err(|e| IoError::invalid_data(e.to_string()))?;
        match node_type {
            NodeType::BranchNode => {
                let children = Self::deserialize_branch(reader, depth)?;
                let reference = reader.read_var_uint()? as i32;
                Ok(Self {
                    node_type,
                    reference,
                    hash: OnceLock::new(),
                    accounted_hash: None,
                    dirty: false,
                    children,
                    key: Vec::new(),
                    next: None,
                    value: Vec::new(),
                })
            }
            NodeType::ExtensionNode => {
                let (key, next) = Self::deserialize_extension(reader, depth)?;
                let reference = reader.read_var_uint()? as i32;
                Ok(Self {
                    node_type,
                    reference,
                    hash: OnceLock::new(),
                    accounted_hash: None,
                    dirty: false,
                    children: Vec::new(),
                    key,
                    next: Some(next),
                    value: Vec::new(),
                })
            }
            NodeType::LeafNode => {
                let value = Self::deserialize_leaf(reader)?;
                let reference = reader.read_var_uint()? as i32;
                Ok(Self {
                    node_type,
                    reference,
                    hash: OnceLock::new(),
                    accounted_hash: None,
                    dirty: false,
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
                    hash: OnceLock::from(hash),
                    accounted_hash: None,
                    dirty: false,
                    children: Vec::new(),
                    key: Vec::new(),
                    next: None,
                    value: Vec::new(),
                })
            }
            NodeType::Empty => Ok(Self::default()),
        }
    }
}

impl Serializable for Node {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        Self::deserialize_with_depth(reader, 0)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_without_reference(writer)?;
        match self.node_type {
            NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                let reference = u64::try_from(self.reference).map_err(|_| {
                    IoError::invalid_data("MPT node reference count cannot be negative")
                })?;
                writer.write_var_uint(reference)?;
            }
            NodeType::HashNode | NodeType::Empty => {}
        }
        Ok(())
    }

    fn size(&self) -> usize {
        self.byte_size()
    }
}
