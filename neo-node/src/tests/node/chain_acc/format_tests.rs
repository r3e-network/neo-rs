//! Tests and fixtures for `chain.acc` file-format parsing.

use super::format::{read_chain_acc_header, read_next_chain_acc_block, skip_chain_acc_records};
use neo_io::{BinaryWriter, Serializable};
use neo_payloads::block::Block;

pub(in crate::node::chain_acc) fn encode_chain_acc(blocks: &[Block]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
    for block in blocks {
        let mut writer = BinaryWriter::new();
        block.serialize(&mut writer).expect("serialize block");
        let block_bytes = writer.into_bytes();
        bytes.extend_from_slice(&(block_bytes.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&block_bytes);
    }
    bytes
}

pub(in crate::node::chain_acc) fn encode_prefixed_chain_acc(
    start_height: u32,
    blocks: &[Block],
) -> Vec<u8> {
    encode_prefixed_chain_acc_with_count(start_height, blocks.len() as u32, blocks)
}

fn encode_prefixed_chain_acc_with_count(
    start_height: u32,
    count: u32,
    blocks: &[Block],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&start_height.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    for block in blocks {
        let mut writer = BinaryWriter::new();
        block.serialize(&mut writer).expect("serialize block");
        let block_bytes = writer.into_bytes();
        bytes.extend_from_slice(&(block_bytes.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&block_bytes);
    }
    bytes
}

pub(in crate::node::chain_acc) fn empty_block(index: u32) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    Block::from_parts(header, Vec::new())
}

pub(in crate::node::chain_acc) fn empty_block_with_prev_hash(
    index: u32,
    prev_hash: neo_primitives::UInt256,
) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    Block::from_parts(header, Vec::new())
}

pub(in crate::node::chain_acc) fn linked_empty_blocks(start: u32, count: usize) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(count);
    let mut previous_hash = None;
    for offset in 0..count {
        let index = start + offset as u32;
        let block = match previous_hash {
            Some(prev_hash) => empty_block_with_prev_hash(index, prev_hash),
            None => empty_block(index),
        };
        previous_hash = Some(block.hash());
        blocks.push(block);
    }
    blocks
}

#[test]
fn read_chain_acc_header_detects_count_only_format() {
    let bytes = encode_chain_acc(&linked_empty_blocks(0, 2));
    let mut cursor = std::io::Cursor::new(bytes);

    let header = read_chain_acc_header(&mut cursor).expect("read header");

    assert_eq!(header.count, 2);
    assert_eq!(header.start_height, None);
}

#[test]
fn read_next_chain_acc_block_streams_one_block_at_a_time() {
    let blocks = linked_empty_blocks(7, 2);
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let header = read_chain_acc_header(&mut cursor).expect("read header");
    let mut block_bytes = Vec::new();

    let first = read_next_chain_acc_block(&mut cursor, 0, &mut block_bytes).expect("read first");
    let second = read_next_chain_acc_block(&mut cursor, 1, &mut block_bytes).expect("read second");

    assert_eq!(header.count, 2);
    assert_eq!(first.index(), 7);
    assert_eq!(second.index(), 8);
}

#[test]
fn read_chain_acc_header_detects_start_height_prefix() {
    let bytes = encode_prefixed_chain_acc(7, &linked_empty_blocks(7, 2));
    let mut cursor = std::io::Cursor::new(bytes);
    let mut block_bytes = Vec::new();

    let header = read_chain_acc_header(&mut cursor).expect("read header");
    let first = read_next_chain_acc_block(&mut cursor, 0, &mut block_bytes).expect("read first");

    assert_eq!(header.count, 2);
    assert_eq!(header.start_height, Some(7));
    assert_eq!(first.index(), 7);
}

#[test]
fn read_chain_acc_header_detects_start_height_prefix_for_mainnet_sized_count() {
    let bytes = encode_prefixed_chain_acc_with_count(0, 11_092_316, &[empty_block(0)]);
    let mut cursor = std::io::Cursor::new(bytes);
    let mut block_bytes = Vec::new();

    let header = read_chain_acc_header(&mut cursor).expect("read header");
    let first = read_next_chain_acc_block(&mut cursor, 0, &mut block_bytes).expect("read first");

    assert_eq!(header.count, 11_092_316);
    assert_eq!(header.start_height, Some(0));
    assert_eq!(first.index(), 0);
}

#[test]
fn read_chain_acc_header_rejects_leading_zero_garbage() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(b"garbage-not-a-block");
    let mut cursor = std::io::Cursor::new(bytes);

    let err = read_chain_acc_header(&mut cursor)
        .expect_err("leading-zero garbage must not be accepted as an empty chain.acc");

    assert!(
        err.to_string().contains("valid first block"),
        "unexpected error: {err}"
    );
}

#[test]
fn buffered_record_skip_positions_reader_at_the_next_record() {
    let blocks = linked_empty_blocks(7, 3);
    let bytes = encode_chain_acc(&blocks);
    let mut reader = std::io::BufReader::with_capacity(64, std::io::Cursor::new(bytes));
    let mut block_bytes = Vec::new();

    let header = read_chain_acc_header(&mut reader).expect("read header");
    skip_chain_acc_records(&mut reader, 2).expect("skip two records");
    let third = read_next_chain_acc_block(&mut reader, 2, &mut block_bytes).expect("read third");

    assert_eq!(header.count, 3);
    assert_eq!(third.index(), 9);
}

#[test]
fn buffered_record_skip_rejects_a_truncated_payload() {
    let blocks = linked_empty_blocks(7, 2);
    let mut bytes = encode_chain_acc(&blocks);
    bytes.truncate(bytes.len() - 5);
    let mut reader = std::io::BufReader::with_capacity(32, std::io::Cursor::new(bytes));

    read_chain_acc_header(&mut reader).expect("read header from complete first record");
    let error = skip_chain_acc_records(&mut reader, 2)
        .expect_err("truncated second payload must not be skipped as valid");

    assert!(
        error.to_string().contains("truncated"),
        "unexpected error: {error}"
    );
}
