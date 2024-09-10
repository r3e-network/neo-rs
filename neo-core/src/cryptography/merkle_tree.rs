use crate::cryptography::{Crypto, UInt256};
use crate::io::Crypto;
use std::collections::BitVec;
use bloomfilter::reexports::bit_vec::BitVec;
use crate::uint256::UInt256;

/// Represents a merkle tree.
pub struct MerkleTree {
    root: Option<Box<MerkleTreeNode>>,
    depth: usize,
}

#[derive(Clone)]
struct MerkleTreeNode {
    hash: UInt256,
    left_child: Option<Box<MerkleTreeNode>>,
    right_child: Option<Box<MerkleTreeNode>>,
}

impl MerkleTree {
    pub fn new(hashes: &[UInt256]) -> Self {
        let root = Self::build(hashes.iter().map(|&h| MerkleTreeNode {
            hash: h,
            left_child: None,
            right_child: None,
        }).collect::<Vec<_>>());

        let depth = root.as_ref().map_or(0, |r| {
            let mut depth = 1;
            let mut node = r;
            while node.left_child.is_some() {
                depth += 1;
                node = node.left_child.as_ref().unwrap();
            }
            depth
        });

        MerkleTree { root, depth }
    }

    fn build(leaves: Vec<MerkleTreeNode>) -> Option<Box<MerkleTreeNode>> {
        match leaves.len() {
            0 => None,
            1 => Some(Box::new(leaves[0].clone())),
            _ => {
                let mut parents = Vec::with_capacity((leaves.len() + 1) / 2);
                for chunk in leaves.chunks(2) {
                    let left = &chunk[0];
                    let right = chunk.get(1).unwrap_or(left);
                    let hash = Self::concat(&left.hash, &right.hash);
                    parents.push(MerkleTreeNode {
                        hash,
                        left_child: Some(Box::new(left.clone())),
                        right_child: Some(Box::new(right.clone())),
                    });
                }
                Self::build(parents)
            }
        }
    }

    fn concat(hash1: &UInt256, hash2: &UInt256) -> UInt256 {
        let mut buffer = [0u8; 64];
        buffer[..32].copy_from_slice(&hash1.to_vec());
        buffer[32..].copy_from_slice(&hash2.to_vec());
        UInt256::from_slice(&Crypto::hash256(&buffer))
    }

    /// Computes the root of the hash tree.
    pub fn compute_root(hashes: &[UInt256]) -> UInt256 {
        match hashes.len() {
            0 => UInt256::ZERO,
            1 => hashes[0],
            _ => {
                let tree = MerkleTree::new(hashes);
                tree.root.unwrap().hash
            }
        }
    }

    /// Gets all nodes of the hash tree in depth-first order.
    pub fn to_hash_array(&self) -> Vec<UInt256> {
        let mut hashes = Vec::new();
        if let Some(root) = &self.root {
            Self::depth_first_search(root, &mut hashes);
        }
        hashes
    }

    fn depth_first_search(node: &MerkleTreeNode, hashes: &mut Vec<UInt256>) {
        if node.left_child.is_none() {
            hashes.push(node.hash);
        } else {
            Self::depth_first_search(node.left_child.as_ref().unwrap(), hashes);
            Self::depth_first_search(node.right_child.as_ref().unwrap(), hashes);
        }
    }

    /// Trims the hash tree using the specified bit vector.
    pub fn trim(&mut self, flags: &BitVec) {
        if let Some(root) = &mut self.root {
            let mut flags = flags.clone();
            flags.resize(1 << (self.depth - 1), false);
            Self::trim_node(root, 0, self.depth, &flags);
        }
    }

    fn trim_node(node: &mut MerkleTreeNode, index: usize, depth: usize, flags: &BitVec) {
        if depth == 1 || node.left_child.is_none() {
            return;
        }
        if depth == 2 {
            if !flags[index * 2] && !flags[index * 2 + 1] {
                node.left_child = None;
                node.right_child = None;
            }
        } else {
            Self::trim_node(node.left_child.as_mut().unwrap(), index * 2, depth - 1, flags);
            Self::trim_node(node.right_child.as_mut().unwrap(), index * 2 + 1, depth - 1, flags);
            if node.left_child.as_ref().unwrap().left_child.is_none() && node.right_child.as_ref().unwrap().right_child.is_none() {
                node.left_child = None;
                node.right_child = None;
            }
        }
    }
}
