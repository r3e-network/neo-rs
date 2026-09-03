use neo_core::cryptography::Crypto;
use neo_core::cryptography::MerkleTree;
use neo_core::network::p2p::payloads::{
    block::Block, merkle_block_payload::MerkleBlockPayload, transaction::Transaction,
};
use neo_core::UInt256;

fn make_uint256(value: u8) -> UInt256 {
    let mut bytes = [0u8; 32];
    bytes[0] = value;
    UInt256::from(bytes)
}

fn hash_pair(left: &UInt256, right: &UInt256) -> UInt256 {
    let mut buffer = [0u8; 64];
    buffer[..32].copy_from_slice(&left.to_array());
    buffer[32..].copy_from_slice(&right.to_array());
    UInt256::from(Crypto::hash256(&buffer))
}

fn manual_merkle_root(leaves: &[UInt256]) -> Option<UInt256> {
    if leaves.is_empty() {
        return None;
    }

    let mut level: Vec<UInt256> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut index = 0;
        while index < level.len() {
            let left = level[index];
            let right = if index + 1 < level.len() {
                level[index + 1]
            } else {
                left
            };
            next.push(hash_pair(&left, &right));
            index += 2;
        }
        level = next;
    }

    level.first().copied()
}

#[test]
fn merkle_tree_root_matches_manual_construction() {
    let leaves = vec![make_uint256(1), make_uint256(2), make_uint256(3)];
    let expected = manual_merkle_root(&leaves).expect("manual root");

    let tree = MerkleTree::new(&leaves);
    let actual = tree.root().copied().expect("tree root");

    assert_eq!(actual, expected);
}

#[test]
fn merkle_tree_trim_all_false_prunes_to_root() {
    let leaves = vec![
        make_uint256(1),
        make_uint256(2),
        make_uint256(3),
        make_uint256(4),
    ];
    let expected_root = manual_merkle_root(&leaves).expect("root");

    let mut tree = MerkleTree::new(&leaves);
    tree.trim(&[false, false, false, false]);
    let hashes = tree.to_hash_array();

    assert_eq!(hashes.len(), 1);
    assert_eq!(hashes[0], expected_root);
}

#[test]
fn merkle_block_payload_trims_and_packs_flags() {
    let mut block = Block::new();
    for value in 0u8..5 {
        let mut tx = Transaction::new();
        tx.set_nonce(value as u32);
        tx.set_script(vec![value]);
        block.transactions.push(tx);
    }

    let tx_hashes = block
        .transactions
        .iter_mut()
        .map(|tx| tx.hash())
        .collect::<Vec<_>>();

    let filter_bits = vec![true, false, true, true, false];
    let payload = MerkleBlockPayload::create(&mut block, filter_bits.clone());

    assert_eq!(payload.tx_count, 5);
    assert_eq!(payload.flags, vec![0b0000_1101]);

    let mut tree = MerkleTree::new(&tx_hashes);
    let mut padded_bits = filter_bits.clone();
    let target_len = if tree.depth() <= 1 {
        1usize
    } else {
        1usize << (tree.depth() - 1)
    };
    use std::cmp::Ordering;
    match padded_bits.len().cmp(&target_len) {
        Ordering::Less => padded_bits.resize(target_len, false),
        Ordering::Greater => padded_bits.truncate(target_len),
        Ordering::Equal => {}
    }
    tree.trim(&padded_bits);
    let expected_hashes = tree.to_hash_array();

    assert_eq!(payload.hashes, expected_hashes);
    assert_eq!(payload.flags, vec![0b0000_1101]);
}

#[test]
fn merkle_block_payload_single_transaction_has_single_flag() {
    let mut block = Block::new();
    let mut tx = Transaction::new();
    tx.set_nonce(42);
    tx.set_script(vec![0xAA]);
    block.transactions.push(tx);

    let payload = MerkleBlockPayload::create(&mut block, vec![true]);

    assert_eq!(payload.tx_count, 1);
    assert_eq!(payload.hashes.len(), 1);
    assert_eq!(payload.flags, vec![0b0000_0001]);
}

#[test]
fn merkle_block_payload_pads_missing_flags() {
    let mut block = Block::new();
    for value in 0u8..3 {
        let mut tx = Transaction::new();
        tx.set_nonce(value as u32);
        tx.set_script(vec![value]);
        block.transactions.push(tx);
    }

    let payload = MerkleBlockPayload::create(&mut block, vec![true, false]);

    assert_eq!(payload.tx_count, 3);
    // Depth 3 -> 4 flags padded into one byte.
    assert_eq!(payload.flags, vec![0b0000_0001]);
}
