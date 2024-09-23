use std::sync::Arc;
use crate::core::block;
use crate::core::transaction;
use crate::crypto::hash;
use crate::util;
use crate::testserdes;
use crate::block::Header;
use crate::merkleblock::MerkleBlock;
use crate::block::MAX_TRANSACTIONS_PER_BLOCK;
use crate::block::ERR_MAX_CONTENTS_PER_BLOCK;

fn new_dumb_block() -> Header {
    Header {
        version: 0,
        prev_hash: hash::sha256(b"a"),
        merkle_root: hash::sha256(b"b"),
        timestamp: 100500,
        index: 1,
        next_consensus: hash::hash160(b"a"),
        script: transaction::Witness {
            verification_script: vec![0x51], // PUSH1
            invocation_script: vec![0x61],   // NOP
        },
    }
}

#[test]
fn test_merkle_block_encode_decode_binary() {
    use crate::testserdes::{encode_decode_binary, encode_binary, decode_binary};
    use crate::block::ERR_MAX_CONTENTS_PER_BLOCK;
    use crate::block::MAX_TRANSACTIONS_PER_BLOCK;

    let b = new_dumb_block();
    let _ = b.hash();
    let expected = MerkleBlock {
        header: Arc::new(b),
        tx_count: 0,
        hashes: vec![],
        flags: vec![],
    };
    encode_decode_binary(&expected, &MerkleBlock::default());

    let b = new_dumb_block();
    let _ = b.hash();
    let expected = MerkleBlock {
        header: Arc::new(b),
        tx_count: MAX_TRANSACTIONS_PER_BLOCK + 1,
        hashes: vec![util::Uint256::default(); MAX_TRANSACTIONS_PER_BLOCK],
        flags: vec![],
    };
    let data = encode_binary(&expected).unwrap();
    assert!(decode_binary(&data, &MerkleBlock::default()).is_err());

    let b = new_dumb_block();
    let _ = b.hash();
    let expected = MerkleBlock {
        header: Arc::new(b),
        tx_count: 0,
        hashes: vec![],
        flags: vec![1, 2, 3, 4, 5],
    };
    let data = encode_binary(&expected).unwrap();
    assert!(decode_binary(&data, &MerkleBlock::default()).is_err());
}
