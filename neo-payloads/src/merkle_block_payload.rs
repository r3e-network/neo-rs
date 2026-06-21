use super::{block::Block, header::Header};
use bitvec::prelude::{BitVec, Lsb0};
use neo_crypto::MerkleTree;
use neo_error::CoreResult;
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

type MerkleBlockFlags = BitVec<u8, Lsb0>;

/// Represents a block that is filtered by a BloomFilter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleBlockPayload {
    /// The header of the block.
    pub header: Header,

    /// The number of the transactions in the block.
    pub tx_count: u32,

    /// The nodes of the transactions hash tree.
    pub hashes: Vec<UInt256>,

    /// The data in the BloomFilter that filtered the block.
    pub flags: Vec<u8>,
}

impl MerkleBlockPayload {
    /// Creates a new merkle block payload.
    pub fn new(header: Header, tx_count: u32, hashes: Vec<UInt256>, flags: Vec<u8>) -> Self {
        Self {
            header,
            tx_count,
            hashes,
            flags,
        }
    }

    /// Creates from a block and filter flags.
    pub fn create(block: &mut Block, filter_bits: Vec<bool>) -> Self {
        Self::try_create(block, filter_bits)
            .expect("block transactions must be serializable to create a merkle block payload")
    }

    /// Creates from a block and filter flags, failing closed if any transaction
    /// hash cannot be represented on the wire.
    pub fn try_create(block: &mut Block, filter_bits: Vec<bool>) -> CoreResult<Self> {
        let tx_count = block.transactions.len() as u32;
        let tx_hashes: Vec<UInt256> = block
            .transactions
            .iter()
            .map(|tx| tx.try_hash())
            .collect::<CoreResult<Vec<_>>>()?;
        let mut tree = MerkleTree::new(&tx_hashes);
        let padded_flags = pad_flags(filter_bits, tree.depth());
        tree.trim(&padded_flags);
        let hashes = tree.to_hash_array();
        let flags = pack_flags(&padded_flags);

        Ok(Self::new(block.header.clone(), tx_count, hashes, flags))
    }
}

fn pad_flags(mut flags: Vec<bool>, depth: usize) -> Vec<bool> {
    if depth == 0 {
        return flags;
    }
    if depth == 1 {
        if flags.is_empty() {
            flags.push(false);
        } else {
            flags.truncate(1);
        }
        return flags;
    }

    let target_len = 1usize << (depth - 1);
    match flags.len().cmp(&target_len) {
        Ordering::Greater => {
            flags.truncate(target_len);
            flags
        }
        Ordering::Less => {
            flags.resize(target_len, false);
            flags
        }
        Ordering::Equal => flags,
    }
}

fn pack_flags(flags: &[bool]) -> Vec<u8> {
    flags
        .iter()
        .copied()
        .collect::<MerkleBlockFlags>()
        .into_vec()
}

impl Serializable for MerkleBlockPayload {
    fn size(&self) -> usize {
        self.header.size()
            + std::mem::size_of::<u32>()
            + SerializeHelper::get_var_size_serializable_slice(&self.hashes)
            + SerializeHelper::get_var_size_bytes(&self.flags)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        // Write tx count as var int
        if self.tx_count as u64 > u16::MAX as u64 {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        writer.write_var_uint(self.tx_count as u64)?;

        // Write hashes
        SerializeHelper::serialize_array(&self.hashes, writer)?;

        // Write flags
        let max_flags = (self.tx_count.max(1) as usize).div_ceil(8);
        if self.flags.len() > max_flags {
            return Err(IoError::invalid_data("Flag length exceeds limit"));
        }
        writer.write_var_bytes(&self.flags)?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;

        let tx_count = reader.read_var_int(u16::MAX as u64)?;
        let tx_count = tx_count as u32;

        let hashes = SerializeHelper::deserialize_array::<UInt256>(reader, tx_count as usize)?;

        // Read flags
        let max_flags = (tx_count.max(1) as usize).div_ceil(8);
        let flags = reader.read_var_bytes(max_flags)?;

        Ok(Self {
            header,
            tx_count,
            hashes,
            flags,
        })
    }
}

#[cfg(test)]
#[path = "tests/merkle_block_payload.rs"]
mod tests;
