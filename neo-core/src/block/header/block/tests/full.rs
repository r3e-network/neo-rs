use crate::{block::Block, h256::H256};
use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, SliceReader};

use super::helpers::{sample_header, sample_tx};

#[test]
fn block_merkle_roundtrip() {
    let txs = vec![sample_tx(1), sample_tx(2)];
    let mut block = Block::new(sample_header(), txs);
    block.recompute_merkle_root();

    let mut buf = Vec::new();
    block.neo_encode(&mut buf);
    let mut reader = SliceReader::new(&buf);
    let decoded = Block::neo_decode(&mut reader).expect("decode block");
    assert_eq!(decoded.header.merkle_root, block.header.merkle_root);
    assert_eq!(decoded.txs.len(), 2);
}

#[test]
fn block_decode_rejects_bad_merkle() {
    let txs = vec![sample_tx(1)];
    let mut header = sample_header();
    header.merkle_root = H256::default();
    let block = Block::new(header, txs);

    let mut buf = Vec::new();
    block.neo_encode(&mut buf);
    let mut reader = SliceReader::new(&buf);
    let err = Block::neo_decode(&mut reader).unwrap_err();
    assert!(matches!(err, DecodeError::InvalidValue("MerkleRoot")));
}
