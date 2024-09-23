use std::error::Error;
use crate::util::{Uint256, DoubleSha256};

// MerkleTree implementation.
pub struct MerkleTree {
    root: Option<Box<MerkleTreeNode>>,
    depth: usize,
}

impl MerkleTree {
    // NewMerkleTree returns a new MerkleTree object.
    pub fn new(hashes: Vec<Uint256>) -> Result<Self, Box<dyn Error>> {
        if hashes.is_empty() {
            return Err("length of the hashes cannot be zero".into());
        }

        let mut nodes: Vec<Box<MerkleTreeNode>> = hashes.into_iter()
            .map(|hash| Box::new(MerkleTreeNode { hash, parent: None, left_child: None, right_child: None }))
            .collect();

        Ok(MerkleTree {
            root: Some(build_merkle_tree(&mut nodes)),
            depth: 1,
        })
    }

    // Root returns the computed root hash of the MerkleTree.
    pub fn root(&self) -> Uint256 {
        self.root.as_ref().unwrap().hash
    }
}

fn build_merkle_tree(leaves: &mut Vec<Box<MerkleTreeNode>>) -> Box<MerkleTreeNode> {
    if leaves.is_empty() {
        panic!("length of leaves cannot be zero");
    }
    if leaves.len() == 1 {
        return leaves.remove(0);
    }

    let mut parents: Vec<Box<MerkleTreeNode>> = Vec::with_capacity((leaves.len() + 1) / 2);
    for i in 0..parents.capacity() {
        let mut parent = Box::new(MerkleTreeNode {
            hash: Uint256::default(),
            parent: None,
            left_child: Some(leaves[i * 2].clone()),
            right_child: None,
        });
        leaves[i * 2].parent = Some(parent.clone());

        if i * 2 + 1 == leaves.len() {
            parent.right_child = Some(parent.left_child.clone().unwrap());
        } else {
            parent.right_child = Some(leaves[i * 2 + 1].clone());
            leaves[i * 2 + 1].parent = Some(parent.clone());
        }

        let mut b1 = parent.left_child.as_ref().unwrap().hash.to_bytes_be();
        let b2 = parent.right_child.as_ref().unwrap().hash.to_bytes_be();
        b1.extend_from_slice(&b2);
        parent.hash = DoubleSha256(&b1);

        parents.push(parent);
    }

    build_merkle_tree(&mut parents)
}

// CalcMerkleRoot calculates the Merkle root hash value for the given slice of hashes.
// It doesn't create a full MerkleTree structure and it uses the given slice as a
// scratchpad, so it will destroy its contents in the process. But it's much more
// memory efficient if you only need a root hash value. While NewMerkleTree would
// make 3*N allocations for N hashes, this function will only make 4.
pub fn calc_merkle_root(hashes: &mut [Uint256]) -> Uint256 {
    if hashes.is_empty() {
        return Uint256::default();
    }
    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut scratch = vec![0u8; 64];
    let parents_len = (hashes.len() + 1) / 2;
    let mut parents = &mut hashes[..parents_len];
    for i in 0..parents_len {
        scratch[..32].copy_from_slice(&hashes[i * 2].to_bytes_be());

        if i * 2 + 1 == hashes.len() {
            scratch[32..].copy_from_slice(&hashes[i * 2].to_bytes_be());
        } else {
            scratch[32..].copy_from_slice(&hashes[i * 2 + 1].to_bytes_be());
        }

        parents[i] = DoubleSha256(&scratch);
    }

    calc_merkle_root(parents)
}

// MerkleTreeNode represents a node in the MerkleTree.
#[derive(Clone)]
pub struct MerkleTreeNode {
    hash: Uint256,
    parent: Option<Box<MerkleTreeNode>>,
    left_child: Option<Box<MerkleTreeNode>>,
    right_child: Option<Box<MerkleTreeNode>>,
}

impl MerkleTreeNode {
    // IsLeaf returns whether this node is a leaf node or not.
    pub fn is_leaf(&self) -> bool {
        self.left_child.is_none() && self.right_child.is_none()
    }

    // IsRoot returns whether this node is a root node or not.
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}
