//! `chain.acc` file-format parsing.

use std::io::{Read, Seek, SeekFrom};

use neo_io::{MemoryReader, Serializable};
use neo_payloads::block::Block;

const MAX_CHAIN_ACC_BLOCK_SIZE: i32 = 0x0200_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChainAccHeader {
    pub(super) count: usize,
    pub(super) start_height: Option<u32>,
}

pub(super) fn read_chain_acc_header<R>(reader: &mut R) -> anyhow::Result<ChainAccHeader>
where
    R: Read + Seek,
{
    let header_start = reader.stream_position()?;
    let first_u32 = read_u32_le(reader)?;
    let second_u32 = read_u32_le(reader)?;

    let count_only_data_start = header_start + 4;
    let prefixed_data_start = header_start + 8;

    let mut block_bytes = Vec::new();
    let count_only_first_block =
        peek_chain_acc_block_at(reader, count_only_data_start, &mut block_bytes);
    let prefixed_first_block =
        peek_chain_acc_block_at(reader, prefixed_data_start, &mut block_bytes);

    if prefixed_first_block
        .as_ref()
        .is_some_and(|block| block.index() == first_u32)
    {
        reader.seek(SeekFrom::Start(prefixed_data_start))?;
        return Ok(ChainAccHeader {
            count: second_u32 as usize,
            start_height: Some(first_u32),
        });
    }

    if count_only_first_block.is_some() {
        reader.seek(SeekFrom::Start(count_only_data_start))?;
        return Ok(ChainAccHeader {
            count: first_u32 as usize,
            start_height: None,
        });
    }

    Err(anyhow::anyhow!(
        "chain.acc header does not point to a valid first block"
    ))
}

fn peek_chain_acc_block_at<R>(
    reader: &mut R,
    position: u64,
    block_bytes: &mut Vec<u8>,
) -> Option<Block>
where
    R: Read + Seek,
{
    reader.seek(SeekFrom::Start(position)).ok()?;
    read_next_chain_acc_block(reader, 0, block_bytes).ok()
}

pub(super) fn read_next_chain_acc_block<R>(
    reader: &mut R,
    index: usize,
    block_bytes: &mut Vec<u8>,
) -> anyhow::Result<Block>
where
    R: Read,
{
    let size = read_i32_le(reader)?;
    if size <= 0 || size > MAX_CHAIN_ACC_BLOCK_SIZE {
        return Err(anyhow::anyhow!(
            "invalid block size at chain.acc record {index}: {size}"
        ));
    }
    block_bytes.resize(size as usize, 0);
    reader
        .read_exact(block_bytes)
        .map_err(|e| anyhow::anyhow!("reading block {index}: {e}"))?;
    Block::deserialize(&mut MemoryReader::new(block_bytes))
        .map_err(|e| anyhow::anyhow!("deserializing block {index}: {e}"))
}

pub(super) fn skip_chain_acc_records<R>(
    reader: &mut R,
    records_to_skip: usize,
) -> anyhow::Result<()>
where
    R: Read + Seek,
{
    for record in 0..records_to_skip {
        let size = read_i32_le(reader)?;
        if size <= 0 || size > MAX_CHAIN_ACC_BLOCK_SIZE {
            return Err(anyhow::anyhow!(
                "invalid block size at chain.acc record {record}: {size}"
            ));
        }
        reader
            .seek(SeekFrom::Current(i64::from(size)))
            .map_err(|err| anyhow::anyhow!("skipping chain.acc record {record}: {err}"))?;
    }
    Ok(())
}

fn read_u32_le<R>(reader: &mut R) -> anyhow::Result<u32>
where
    R: Read,
{
    let mut bytes = [0u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow::anyhow!("reading u32: {e}"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32_le<R>(reader: &mut R) -> anyhow::Result<i32>
where
    R: Read,
{
    let mut bytes = [0u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow::anyhow!("reading i32: {e}"))?;
    Ok(i32::from_le_bytes(bytes))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use neo_io::{BinaryWriter, Serializable};

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

    fn encode_prefixed_chain_acc(start_height: u32, blocks: &[Block]) -> Vec<u8> {
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

        let first =
            read_next_chain_acc_block(&mut cursor, 0, &mut block_bytes).expect("read first");
        let second =
            read_next_chain_acc_block(&mut cursor, 1, &mut block_bytes).expect("read second");

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
        let first =
            read_next_chain_acc_block(&mut cursor, 0, &mut block_bytes).expect("read first");

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
        let first =
            read_next_chain_acc_block(&mut cursor, 0, &mut block_bytes).expect("read first");

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
}
