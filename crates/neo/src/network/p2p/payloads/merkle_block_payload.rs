// Copyright (C) 2015-2025 The Neo Project.
//
// merkle_block_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{block::Block, header::Header};
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::uint256::UINT256_SIZE;
use crate::UInt256;
use serde::{Deserialize, Serialize};

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
        // Build merkle tree from transaction hashes
        let tx_hashes: Vec<UInt256> = block.transactions.iter_mut().map(|tx| tx.hash()).collect();

        // Create merkle tree and trim based on filter
        // This is a simplified version - full implementation would use MerkleTree
        let hashes = tx_hashes.clone(); // Simplified - should be trimmed merkle tree

        // Convert filter bits to bytes
        let mut flags = vec![0u8; (filter_bits.len() + 7) / 8];
        for (i, bit) in filter_bits.iter().enumerate() {
            if *bit {
                flags[i / 8] |= 1 << (i % 8);
            }
        }

        Self {
            header: block.header.clone(),
            tx_count: block.transactions.len() as u32,
            hashes,
            flags,
        }
    }
}

impl Serializable for MerkleBlockPayload {
    fn size(&self) -> usize {
        self.header.size()
            + get_var_size(self.tx_count as u64)
            + get_var_size(self.hashes.len() as u64)
            + self.hashes.len() * UINT256_SIZE
            + get_var_size(self.flags.len() as u64)
            + self.flags.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        // Write tx count as var int
        if self.tx_count as u64 > u16::MAX as u64 {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        writer.write_var_uint(self.tx_count as u64)?;

        // Write hashes
        writer.write_var_uint(self.hashes.len() as u64)?;
        for hash in &self.hashes {
            writer.write_serializable(hash)?;
        }

        // Write flags
        let max_flags = ((self.tx_count.max(1) as usize) + 7) / 8;
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

        // Read hashes
        let hash_count = reader.read_var_int(tx_count as u64)?;
        if hash_count > tx_count as u64 {
            return Err(IoError::invalid_data("Too many hashes"));
        }

        let mut hashes = Vec::with_capacity(hash_count as usize);
        for _ in 0..hash_count {
            hashes.push(<UInt256 as Serializable>::deserialize(reader)?);
        }

        // Read flags
        let max_flags = ((tx_count.max(1) + 7) / 8) as usize;
        let flags = reader.read_var_bytes(max_flags)?;

        Ok(Self {
            header,
            tx_count,
            hashes,
            flags,
        })
    }
}
