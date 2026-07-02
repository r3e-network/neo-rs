//! Neo Merkle tree construction for block and MerkleBlock payloads.
//!
//! The root algorithm is protocol data, not an interchangeable generic tree:
//! odd leaves are duplicated and parent hashes are Neo `Hash256(left || right)`
//! values over little-endian `UInt256` bytes. Generic Merkle crates can provide
//! useful proof machinery, but this module keeps the Neo/C# byte semantics at
//! the boundary.

use crate::Crypto;
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

/// Neo-compatible Merkle tree used by ledger validation and network payloads.
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
    UInt256::from(Crypto::hash256(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> UInt256 {
        UInt256::from_bytes(&[byte; 32]).expect("fixed-width hash")
    }

    fn expected_pair(left: UInt256, right: UInt256) -> UInt256 {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&left.to_array());
        bytes[32..].copy_from_slice(&right.to_array());
        UInt256::from(Crypto::hash256(&bytes))
    }

    #[test]
    fn root_duplicates_odd_leaf_and_hashes_little_endian_uint256_bytes() {
        let a = hash(0x11);
        let b = hash(0x22);
        let c = hash(0x33);

        let ab = expected_pair(a, b);
        let cc = expected_pair(c, c);
        let expected = expected_pair(ab, cc);

        assert_eq!(MerkleTree::compute_root(&[a, b, c]), Some(expected));
        assert_eq!(
            MerkleTree::compute_root_with_tree(&[a, b, c]),
            Some(expected)
        );
    }

    #[test]
    fn compute_root_matches_tree_builder_for_small_leaf_counts() {
        for count in 0..=8 {
            let leaves = (0..count).map(|byte| hash(byte as u8)).collect::<Vec<_>>();

            assert_eq!(
                MerkleTree::compute_root(&leaves),
                MerkleTree::compute_root_with_tree(&leaves),
                "optimized and full-tree root paths must match for {count} leaves"
            );
        }
    }

    #[test]
    fn empty_tree_has_no_root_and_single_leaf_is_its_own_root() {
        let leaf = hash(0x42);

        assert_eq!(MerkleTree::compute_root(&[]), None);
        assert_eq!(MerkleTree::new(&[]).root(), None);
        assert_eq!(MerkleTree::compute_root(&[leaf]), Some(leaf));

        let tree = MerkleTree::new(&[leaf]);
        assert_eq!(tree.depth(), 1);
        assert_eq!(tree.root(), Some(&leaf));
    }

    #[test]
    fn trim_keeps_matched_leaf_sibling_and_prunes_unmatched_branches() {
        let leaves = [hash(0x01), hash(0x02), hash(0x03), hash(0x04)];
        let mut tree = MerkleTree::new(&leaves);

        tree.trim(&[true, false, false, false]);

        assert_eq!(
            tree.to_hash_array(),
            vec![leaves[0], leaves[1], expected_pair(leaves[2], leaves[3])]
        );
    }

    #[test]
    fn trim_all_false_collapses_tree_to_root_hash() {
        let leaves = [hash(0x01), hash(0x02), hash(0x03), hash(0x04)];
        let root = MerkleTree::compute_root(&leaves).expect("root");
        let mut tree = MerkleTree::new(&leaves);

        tree.trim(&[false, false, false, false]);

        assert_eq!(tree.to_hash_array(), vec![root]);
    }

    #[test]
    fn trim_treats_missing_flags_as_false_and_ignores_extra_flags() {
        let leaves = [hash(0x01), hash(0x02), hash(0x03), hash(0x04)];
        let expected = vec![leaves[0], leaves[1], expected_pair(leaves[2], leaves[3])];

        for flags in [
            &[true][..],
            &[true, false, false, false],
            &[true, false, false, false, true],
        ] {
            let mut tree = MerkleTree::new(&leaves);
            tree.trim(flags);

            assert_eq!(tree.to_hash_array(), expected, "flags: {flags:?}");
        }
    }

    #[test]
    fn to_hash_array_preserves_duplicated_odd_leaf_in_depth_first_order() {
        let leaves = [hash(0x11), hash(0x22), hash(0x33)];
        let tree = MerkleTree::new(&leaves);

        assert_eq!(
            tree.to_hash_array(),
            vec![leaves[0], leaves[1], leaves[2], leaves[2]]
        );
    }
}
