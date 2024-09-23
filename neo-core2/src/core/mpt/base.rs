use std::fmt;

use crate::crypto::hash;
use crate::io;
use crate::util::{self, Uint256};

// BaseNode implements basic things every node needs like caching hash and
// serialized representation. It's a basic node building block intended to be
// included into all node types.
pub struct BaseNode {
    hash: Uint256,
    bytes: Vec<u8>,
    hash_valid: bool,
    bytes_valid: bool,
}

// BaseNodeIface abstracts away basic Node functions.
pub trait BaseNodeIface {
    fn hash(&self) -> Uint256;
    fn node_type(&self) -> NodeType;
    fn bytes(&self) -> &[u8];
}

trait FlushedNode {
    fn set_cache(&mut self, bytes: Vec<u8>, hash: Uint256);
}

impl BaseNode {
    pub fn set_cache(&mut self, bytes: Vec<u8>, hash: Uint256) {
        self.bytes = bytes;
        self.hash = hash;
        self.bytes_valid = true;
        self.hash_valid = true;
    }

    // get_hash returns the hash of this BaseNode.
    pub fn get_hash(&mut self, n: &dyn Node) -> Uint256 {
        if !self.hash_valid {
            self.update_hash(n);
        }
        self.hash
    }

    // get_bytes returns a slice of bytes representing this node.
    pub fn get_bytes(&mut self, n: &dyn Node) -> &[u8] {
        if !self.bytes_valid {
            self.update_bytes(n);
        }
        &self.bytes
    }

    // update_hash updates the hash field for this BaseNode.
    fn update_hash(&mut self, n: &dyn Node) {
        if matches!(n.node_type(), NodeType::HashT | NodeType::EmptyT) {
            panic!("can't update hash for empty or hash node");
        }
        self.hash = hash::double_sha256(self.get_bytes(n));
        self.hash_valid = true;
    }

    // update_cache updates the hash and bytes fields for this BaseNode.
    fn update_bytes(&mut self, n: &dyn Node) {
        let mut bw = io::BufBinWriter::new();
        bw.grow(1 + n.size());
        encode_node_with_type(n, &mut bw);
        self.bytes = bw.bytes();
        self.bytes_valid = true;
    }

    // invalidate_cache sets all cache fields to invalid state.
    pub fn invalidate_cache(&mut self) {
        self.bytes_valid = false;
        self.hash_valid = false;
    }
}

pub fn encode_binary_as_child(n: &dyn Node, w: &mut io::BinWriter) {
    if is_empty(n) {
        w.write_u8(NodeType::EmptyT as u8);
        return;
    }
    w.write_u8(NodeType::HashT as u8);
    w.write_bytes(&n.hash().to_bytes_be());
}

// encode_node_with_type encodes the node together with its type.
pub fn encode_node_with_type(n: &dyn Node, w: &mut io::BinWriter) {
    w.write_u8(n.node_type() as u8);
    n.encode_binary(w);
}

// decode_node_with_type decodes the node together with its type.
pub fn decode_node_with_type(r: &mut io::BinReader) -> Option<Box<dyn Node>> {
    if r.err().is_some() {
        return None;
    }
    let n: Box<dyn Node> = match NodeType::from(r.read_u8()) {
        NodeType::BranchT => Box::new(BranchNode::default()),
        NodeType::ExtensionT => Box::new(ExtensionNode::default()),
        NodeType::HashT => Box::new(HashNode {
            base_node: BaseNode {
                hash_valid: true,
                ..Default::default()
            },
        }),
        NodeType::LeafT => Box::new(LeafNode::default()),
        NodeType::EmptyT => Box::new(EmptyNode::default()),
        _ => {
            r.set_err(fmt::Error::new(fmt::ErrorKind::InvalidData, "invalid node type"));
            return None;
        }
    };
    n.decode_binary(r);
    Some(n)
}
