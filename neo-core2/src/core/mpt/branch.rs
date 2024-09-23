use std::error::Error;
use std::fmt;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use serde::{Serialize, Deserialize};
use crate::io::{BinWriter, BinReader, Serializable};
use crate::util::{self, Uint256};
use super::{Node, NodeType, BaseNode, EmptyNode, NodeObject, BranchT, is_empty, encode_binary_as_child};

const CHILDREN_COUNT: usize = 17;
const LAST_CHILD: usize = CHILDREN_COUNT - 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct BranchNode {
    base: BaseNode,
    children: [Box<dyn Node>; CHILDREN_COUNT],
}

impl BranchNode {
    pub fn new() -> Self {
        let mut children: [Box<dyn Node>; CHILDREN_COUNT] = Default::default();
        for child in children.iter_mut() {
            *child = Box::new(EmptyNode {});
        }
        BranchNode {
            base: BaseNode::default(),
            children,
        }
    }

    pub fn split_path(path: &[u8]) -> (u8, &[u8]) {
        if !path.is_empty() {
            (path[0], &path[1..])
        } else {
            (LAST_CHILD as u8, path)
        }
    }
}

impl Node for BranchNode {
    fn node_type(&self) -> NodeType {
        BranchT
    }

    fn size(&self) -> usize {
        let mut sz = CHILDREN_COUNT;
        for child in &self.children {
            if !is_empty(child.as_ref()) {
                sz += Uint256::size();
            }
        }
        sz
    }

    fn clone_node(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

impl BaseNode for BranchNode {
    fn hash(&self) -> Uint256 {
        self.get_hash(self)
    }

    fn bytes(&self) -> Vec<u8> {
        self.get_bytes(self)
    }
}

impl Serializable for BranchNode {
    fn encode_binary(&self, writer: &mut BinWriter) {
        for child in &self.children {
            encode_binary_as_child(child.as_ref(), writer);
        }
    }

    fn decode_binary(&mut self, reader: &mut BinReader) {
        for child in &mut self.children {
            let mut no = NodeObject::default();
            no.decode_binary(reader);
            *child = no.node;
        }
    }
}

impl fmt::Debug for BranchNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "BranchNode {{ children: {:?} }}", self.children)
    }
}

impl fmt::Display for BranchNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "BranchNode")
    }
}

impl std::error::Error for BranchNode {}

impl BranchNode {
    pub fn marshal_json(&self) -> Result<String, Box<dyn Error>> {
        serde_json::to_string(&self.children).map_err(|e| e.into())
    }

    pub fn unmarshal_json(&mut self, data: &str) -> Result<(), Box<dyn Error>> {
        let obj: NodeObject = serde_json::from_str(data)?;
        if let Some(u) = obj.node.downcast_ref::<BranchNode>() {
            *self = u.clone();
            Ok(())
        } else {
            Err("expected branch node".into())
        }
    }
}
