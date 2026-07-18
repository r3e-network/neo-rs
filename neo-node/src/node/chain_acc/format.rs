//! `chain.acc` file-format parsing.

use std::io::{BufRead, Read, Seek, SeekFrom};

use neo_io::{MemoryReader, Serializable};
use neo_payloads::block::Block;

const MAX_CHAIN_ACC_BLOCK_SIZE: i32 = 0x0200_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum ChainAccFormat {
    CountOnly = 1,
    HeightPrefixed = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChainAccHeader {
    pub(super) count: usize,
    pub(super) start_height: Option<u32>,
    pub(super) data_offset: u64,
    pub(super) format: ChainAccFormat,
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
            data_offset: prefixed_data_start,
            format: ChainAccFormat::HeightPrefixed,
        });
    }

    if count_only_first_block.is_some() {
        reader.seek(SeekFrom::Start(count_only_data_start))?;
        return Ok(ChainAccHeader {
            count: first_u32 as usize,
            start_height: None,
            data_offset: count_only_data_start,
            format: ChainAccFormat::CountOnly,
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
    R: BufRead + Seek,
{
    let start_offset = reader.stream_position()?;
    skip_chain_acc_records_observed(reader, 0, records_to_skip, start_offset, |_, _| Ok(()))?;
    Ok(())
}

pub(super) fn skip_chain_acc_records_observed<R, F>(
    reader: &mut R,
    first_record: usize,
    records_to_skip: usize,
    mut offset: u64,
    mut observe: F,
) -> anyhow::Result<u64>
where
    R: BufRead + Seek,
    F: FnMut(u64, u64) -> anyhow::Result<()>,
{
    for relative_record in 0..records_to_skip {
        let record = first_record
            .checked_add(relative_record)
            .ok_or_else(|| anyhow::anyhow!("chain.acc record number overflow"))?;
        let size = read_i32_le(reader)?;
        if size <= 0 || size > MAX_CHAIN_ACC_BLOCK_SIZE {
            return Err(anyhow::anyhow!(
                "invalid block size at chain.acc record {record}: {size}"
            ));
        }
        consume_exact(reader, size as usize)
            .map_err(|err| anyhow::anyhow!("skipping chain.acc record {record}: {err}"))?;
        offset = offset
            .checked_add(4)
            .and_then(|offset| offset.checked_add(size as u64))
            .ok_or_else(|| anyhow::anyhow!("chain.acc offset overflow at record {record}"))?;
        let next_record = u64::try_from(record)
            .ok()
            .and_then(|record| record.checked_add(1))
            .ok_or_else(|| anyhow::anyhow!("chain.acc next record overflow"))?;
        observe(next_record, offset)?;
    }
    Ok(offset)
}

fn consume_exact<R>(reader: &mut R, mut remaining: usize) -> std::io::Result<()>
where
    R: BufRead + ?Sized,
{
    while remaining > 0 {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("payload is truncated with {remaining} bytes remaining"),
            ));
        }
        let consumed = available.len().min(remaining);
        reader.consume(consumed);
        remaining -= consumed;
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
