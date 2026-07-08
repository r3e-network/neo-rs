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
