use std::error::Error;
use std::fmt;

use crate::io::{BinReader, BinWriter, Serializable};
use crate::util::Uint256;

// HashNode represents an MPT's hash node.
#[derive(Clone)]
pub struct HashNode {
    base_node: BaseNode,
    collapsed: bool,
}

impl HashNode {
    // NewHashNode returns a hash node with the specified hash.
    pub fn new(h: Uint256) -> Self {
        HashNode {
            base_node: BaseNode {
                hash: h,
                hash_valid: true,
            },
            collapsed: false,
        }
    }

    // Type implements Node interface.
    pub fn node_type(&self) -> NodeType {
        NodeType::HashT
    }

    // Size implements Node interface.
    pub fn size(&self) -> usize {
        Uint256::size()
    }

    // Hash implements Node interface.
    pub fn hash(&self) -> Uint256 {
        if !self.base_node.hash_valid {
            panic!("can't get hash of an empty HashNode");
        }
        self.base_node.hash
    }

    // Bytes returns serialized HashNode.
    pub fn bytes(&self) -> Vec<u8> {
        self.base_node.get_bytes()
    }

    // DecodeBinary implements io::Serializable.
    pub fn decode_binary(&mut self, r: &mut BinReader) {
        if self.base_node.hash_valid {
            self.base_node.hash.decode_binary(r);
        }
    }

    // EncodeBinary implements io::Serializable.
    pub fn encode_binary(&self, w: &mut BinWriter) {
        if !self.base_node.hash_valid {
            return;
        }
        w.write_bytes(&self.base_node.hash.to_bytes());
    }

    // MarshalJSON implements the json::Marshaler.
    pub fn marshal_json(&self) -> Result<String, Box<dyn Error>> {
        Ok(format!(r#"{{"hash":"{}"}}"#, self.base_node.hash.to_string_le()))
    }

    // UnmarshalJSON implements the json::Unmarshaler.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        let obj: NodeObject = serde_json::from_slice(data)?;
        if let Some(u) = obj.node.as_any().downcast_ref::<HashNode>() {
            *self = u.clone();
            Ok(())
        } else {
            Err(Box::new(fmt::Error::new(fmt::Error, "expected hash node")))
        }
    }

    // Clone implements Node interface.
    pub fn clone_node(&self) -> Box<dyn Node> {
        let mut res = self.clone();
        res.collapsed = false;
        Box::new(res)
    }
}
