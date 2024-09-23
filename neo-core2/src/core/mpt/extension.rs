use std::collections::HashMap;
use std::fmt;
use std::error::Error;
use std::convert::TryFrom;
use serde::{Serialize, Deserialize};
use hex;
use crate::config::limits;
use crate::io::{BinReader, BinWriter, Serializable};
use crate::util::{self, Uint256};

const MAX_PATH_LENGTH: usize = (limits::MAX_STORAGE_KEY_LEN + 4) * 2;
const MAX_KEY_LENGTH: usize = MAX_PATH_LENGTH / 2;

#[derive(Clone, Serialize, Deserialize)]
pub struct ExtensionNode {
    #[serde(flatten)]
    base_node: BaseNode,
    key: Vec<u8>,
    next: Box<dyn Node>,
}

impl ExtensionNode {
    pub fn new(key: Vec<u8>, next: Box<dyn Node>) -> Self {
        Self {
            base_node: BaseNode::default(),
            key,
            next,
        }
    }

    pub fn node_type(&self) -> NodeType {
        NodeType::ExtensionT
    }

    pub fn hash(&self) -> Uint256 {
        self.base_node.get_hash(self)
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.base_node.get_bytes(self)
    }

    pub fn decode_binary(&mut self, r: &mut BinReader) -> Result<(), Box<dyn Error>> {
        let sz = r.read_var_uint()?;
        if sz > MAX_PATH_LENGTH as u64 {
            return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, format!("extension node key is too big: {}", sz))));
        }
        self.key = r.read_bytes(sz as usize)?;
        let mut no = NodeObject::default();
        no.decode_binary(r)?;
        self.next = no.node;
        self.base_node.invalidate_cache();
        Ok(())
    }

    pub fn encode_binary(&self, w: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        w.write_var_bytes(&self.key)?;
        encode_binary_as_child(&*self.next, w)?;
        Ok(())
    }

    pub fn size(&self) -> usize {
        util::get_var_size(self.key.len()) + self.key.len() + 1 + Uint256::size()
    }
}

impl fmt::Debug for ExtensionNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionNode")
            .field("key", &hex::encode(&self.key))
            .field("next", &self.next)
            .finish()
    }
}

impl Clone for Box<dyn Node> {
    fn clone(&self) -> Box<dyn Node> {
        self.clone_box()
    }
}

impl Node for ExtensionNode {
    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

impl Serializable for ExtensionNode {
    fn decode_binary(&mut self, r: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.decode_binary(r)
    }

    fn encode_binary(&self, w: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        self.encode_binary(w)
    }
}

impl<'de> Deserialize<'de> for Box<dyn Node> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let obj = NodeObject::deserialize(deserializer)?;
        if let Some(node) = obj.node.downcast_ref::<ExtensionNode>() {
            Ok(Box::new(node.clone()))
        } else {
            Err(serde::de::Error::custom("expected extension node"))
        }
    }
}
