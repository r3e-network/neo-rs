use crate::types::UInt256;

pub struct MerkleTreeNode {
    pub hash: UInt256,
    pub parent: Option<Box<MerkleTreeNode>>,
    pub left_child: Option<Box<MerkleTreeNode>>,
    pub right_child: Option<Box<MerkleTreeNode>>,
}

impl MerkleTreeNode {
    pub fn is_leaf(&self) -> bool {
        self.left_child.is_none() && self.right_child.is_none()
    }

    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}
