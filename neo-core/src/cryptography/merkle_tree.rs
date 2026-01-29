//! Merkle tree implementation for Neo blockchain.
//!
//! This module provides Merkle tree functionality used for computing
//! transaction and block payload roots.

use neo_crypto::NeoHash;
use neo_primitives::UInt256;

/// A node in the Merkle tree.
#[derive(Clone, Debug)]
struct MerkleTreeNode {
    hash: UInt256,
    left: Option<Box<MerkleTreeNode>>,
    right: Option<Box<MerkleTreeNode>>,
}

impl MerkleTreeNode {
    fn leaf(hash: UInt256) -> Self {
        Self {
            hash,
            left: None,
            right: None,
        }
    }

    fn is_pruned(&self) -> bool {
        self.left.is_none() && self.right.is_none()
    }
}

/// Merkle tree implementation used across the network layer.
#[derive(Debug)]
pub struct MerkleTree {
    root: Option<Box<MerkleTreeNode>>,
    depth: usize,
}

impl MerkleTree {
    /// Builds a merkle tree from the supplied hashes.
    pub fn new(hashes: &[UInt256]) -> Self {
        if hashes.is_empty() {
            return Self {
                root: None,
                depth: 0,
            };
        }

        let mut nodes: Vec<MerkleTreeNode> =
            hashes.iter().copied().map(MerkleTreeNode::leaf).collect();

        let mut depth = 1;
        while nodes.len() > 1 {
            let mut parents = Vec::with_capacity(nodes.len().div_ceil(2));
            let mut index = 0;
            while index < nodes.len() {
                let left = nodes[index].clone();
                let right = if index + 1 < nodes.len() {
                    nodes[index + 1].clone()
                } else {
                    left.clone()
                };

                let hash = hash_pair(&left.hash, &right.hash);
                parents.push(MerkleTreeNode {
                    hash,
                    left: Some(Box::new(left)),
                    right: Some(Box::new(right)),
                });

                index += 2;
            }
            nodes = parents;
            depth += 1;
        }

        Self {
            root: nodes.pop().map(Box::new),
            depth,
        }
    }

    /// Returns the depth of the tree (leaf-only trees report depth 1).
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Computes the merkle root for the supplied hashes.
    ///
    /// Performance: Uses an optimized in-place algorithm that avoids building
    /// the full tree structure. Only allocates a single working buffer.
    /// Time complexity: O(n), Space complexity: O(n) where n = number of hashes.
    pub fn compute_root(hashes: &[UInt256]) -> Option<UInt256> {
        if hashes.is_empty() {
            return None;
        }
        if hashes.len() == 1 {
            return Some(hashes[0]);
        }

        // Work buffer - we'll reduce this in-place level by level
        let mut current: Vec<UInt256> = hashes.to_vec();

        while current.len() > 1 {
            let mut next = Vec::with_capacity(current.len().div_ceil(2));
            let mut i = 0;
            while i < current.len() {
                let left = &current[i];
                // If odd number of elements, duplicate the last one
                let right = current.get(i + 1).unwrap_or(left);
                next.push(hash_pair(left, right));
                i += 2;
            }
            current = next;
        }

        current.pop()
    }

    /// Computes the merkle root by building the full tree.
    /// Use this when you need the tree structure for trimming or proof generation.
    pub fn compute_root_with_tree(hashes: &[UInt256]) -> Option<UInt256> {
        let tree = Self::new(hashes);
        tree.root().copied()
    }

    /// Returns the root hash when available.
    pub fn root(&self) -> Option<&UInt256> {
        self.root.as_ref().map(|node| &node.hash)
    }

    /// Trims the tree according to the provided bloom-filter flags.
    ///
    /// Flags represent which leaves should be retained. When both leaves under
    /// a node are excluded the branch is pruned and replaced by the parent hash.
    pub fn trim(&mut self, flags: &[bool]) {
        let Some(root) = self.root.as_mut() else {
            return;
        };

        if self.depth <= 1 {
            return;
        }

        let required = 1usize << (self.depth - 1);
        let mut padded = vec![false; required];
        for (index, flag) in flags.iter().enumerate().take(required) {
            padded[index] = *flag;
        }

        trim_node(root, 0, self.depth, &padded);
    }

    /// Returns the hashes in depth-first order.
    pub fn to_hash_array(&self) -> Vec<UInt256> {
        let mut hashes = Vec::new();
        if let Some(root) = self.root.as_ref() {
            depth_first_collect(root, &mut hashes);
        }
        hashes
    }
}

fn depth_first_collect(node: &MerkleTreeNode, hashes: &mut Vec<UInt256>) {
    if node.left.is_none() {
        hashes.push(node.hash);
    } else {
        if let Some(left) = node.left.as_ref() {
            depth_first_collect(left, hashes);
        }
        if let Some(right) = node.right.as_ref() {
            depth_first_collect(right, hashes);
        }
    }
}

fn trim_node(node: &mut MerkleTreeNode, index: usize, depth: usize, flags: &[bool]) {
    if depth <= 1 || node.left.is_none() {
        return;
    }

    if depth == 2 {
        let left_flag = flags.get(index * 2).copied().unwrap_or(false);
        let right_flag = flags.get(index * 2 + 1).copied().unwrap_or(false);

        if !left_flag && !right_flag {
            node.left = None;
            node.right = None;
        }
        return;
    }

    if let Some(left) = node.left.as_mut() {
        trim_node(left, index * 2, depth - 1, flags);
    }
    if let Some(right) = node.right.as_mut() {
        trim_node(right, index * 2 + 1, depth - 1, flags);
    }

    let left_pruned = node
        .left
        .as_ref()
        .map(|child| child.is_pruned())
        .unwrap_or(true);
    let right_pruned = node
        .right
        .as_ref()
        .map(|child| child.is_pruned())
        .unwrap_or(true);

    if left_pruned && right_pruned {
        node.left = None;
        node.right = None;
    }
}

fn hash_pair(left: &UInt256, right: &UInt256) -> UInt256 {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(&left.to_array());
    bytes[32..].copy_from_slice(&right.to_array());
    UInt256::from(NeoHash::hash256(&bytes))
}
