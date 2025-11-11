use crate::{block::merkle::compute_merkle_root, block::TrimmedBlock, h256::H256};
use neo_base::encoding::{NeoDecode, NeoEncode, SliceReader};

use super::helpers::sample_header;

#[test]
fn trimmed_block_binary_roundtrip() {
    let mut header = sample_header();
    let hashes = vec![H256::from_le_bytes([1u8; 32])];
    header.merkle_root = compute_merkle_root(&hashes);
    let trimmed = TrimmedBlock::new(header, hashes);

    let mut buf = Vec::new();
    trimmed.neo_encode(&mut buf);
    let mut reader = SliceReader::new(&buf);
    let decoded = TrimmedBlock::neo_decode(&mut reader).expect("decode trimmed");
    assert_eq!(decoded.hashes.len(), 1);
    assert_eq!(decoded.hashes[0], decoded.header.merkle_root);
}
