//! Comprehensive MPT tests converted from C# Neo.Cryptography.MPTTrie.Tests
//! Covers all test cases from UT_Node.cs, UT_Trie.cs, and UT_Cache.cs

use crate::mpt_trie::node::MAX_KEY_LENGTH;
use crate::mpt_trie::{MptCache, MptResult, MptStoreSnapshot, Node, NodeType, Trie};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt256;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Helper trait to provide to_array method for serialization
trait SerializableExt: Serializable {
    fn to_array(&self) -> neo_io::IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize(&mut writer)?;
        Ok(writer.into_bytes())
    }
}

impl<T: Serializable> SerializableExt for T {}

/// Mock store for testing - matches C# MemoryStore
struct MockStore {
    data: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MockStore {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_data(&self) -> HashMap<Vec<u8>, Vec<u8>> {
        self.data.lock().clone()
    }
}

impl MptStoreSnapshot for MockStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.data.lock().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }
}

fn serialize_child(node: &Node) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    node.serialize_as_child(&mut writer).unwrap();
    writer.into_bytes()
}

fn deserialize_node(data: &[u8]) -> Node {
    let mut reader = MemoryReader::new(data);
    Node::deserialize(&mut reader).unwrap()
}

fn malicious_nested_extension_entry(depth: usize) -> Vec<u8> {
    let mut entry = Vec::with_capacity((depth * 3) + 1);
    for _ in 0..depth {
        entry.push(NodeType::ExtensionNode as u8);
        entry.push(0x00);
    }
    entry.push(NodeType::Empty as u8);
    entry.extend(std::iter::repeat_n(0x00, depth));
    entry
}

// Helper functions matching Helper.cs
fn prepare_mpt_node1() -> Node {
    Node::new_hash(UInt256::zero())
}

fn prepare_mpt_node2() -> Node {
    Node::new_leaf(vec![0x12, 0x34])
}

fn prepare_mpt_node3() -> Node {
    let mut branch = Node::new_branch();
    branch.set_child(1, prepare_mpt_node1());
    branch.set_child(2, prepare_mpt_node2());
    branch
}

#[path = "mpt_trie/cache.rs"]
mod cache;
#[path = "mpt_trie/diagnostics.rs"]
mod diagnostics;
#[path = "mpt_trie/find_limited.rs"]
mod find_limited;
#[path = "mpt_trie/mainnet_vectors.rs"]
mod mainnet_vectors;
#[path = "mpt_trie/node.rs"]
mod node;
#[path = "mpt_trie/trie.rs"]
mod trie;
