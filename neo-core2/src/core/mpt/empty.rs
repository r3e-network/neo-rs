use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt;

use crate::io::{BinReader, BinWriter};
use crate::util::Uint256;
use crate::core::mpt::{Node, NodeType, EmptyT};

// EmptyNode represents an empty node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyNode;

// Implementing the io::Serializable trait for EmptyNode
impl EmptyNode {
    pub fn decode_binary(&self, _reader: &mut BinReader) {
        // No-op
    }

    pub fn encode_binary(&self, _writer: &mut BinWriter) {
        // No-op
    }
}

// Implementing the Node trait for EmptyNode
impl Node for EmptyNode {
    fn size(&self) -> usize {
        0
    }

    fn marshal_json(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(b"{}".to_vec())
    }

    fn unmarshal_json(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let m: Value = serde_json::from_slice(bytes)?;
        if !m.as_object().unwrap().is_empty() {
            return Err(Box::new(EmptyNodeError));
        }
        Ok(())
    }

    fn hash(&self) -> Uint256 {
        panic!("can't get hash of an EmptyNode")
    }

    fn node_type(&self) -> NodeType {
        EmptyT
    }

    fn bytes(&self) -> Vec<u8> {
        Vec::new()
    }

    fn clone_node(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

// Custom error for EmptyNode
#[derive(Debug)]
struct EmptyNodeError;

impl fmt::Display for EmptyNodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "expected empty node")
    }
}

impl Error for EmptyNodeError {}
