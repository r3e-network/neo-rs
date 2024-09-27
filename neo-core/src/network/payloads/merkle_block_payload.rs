use std::mem;
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::merkle::MerkleTree;
use crate::network::payloads::{Block, Header};
use neo_type::H256;

/// Represents a block that is filtered by a BloomFilter.
pub struct MerkleBlockPayload {
    /// The header of the block.
    pub header: Header,

    /// The number of the transactions in the block.
    pub tx_count: u32,

    /// The nodes of the transactions hash tree.
    pub hashes: Vec<H256>,

    /// The data in the BloomFilter that filtered the block.
    pub flags: Vec<u8>,
}

impl MerkleBlockPayload {
    /// Creates a new instance of the MerkleBlockPayload struct.
    pub fn create(block: &Block, flags: &[bool]) -> Self {
        let tree = MerkleTree::new(block.transactions.iter().map(|tx| tx.hash()).collect());
        let trimmed_tree = tree.trim(flags);
        let buffer: Vec<u8> = flags.chunks(8)
            .map(|chunk| chunk.iter().enumerate().fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i)))
            .collect();

        MerkleBlockPayload {
            header: block.header.clone(),
            tx_count: block.transactions.len() as u32,
            hashes: trimmed_tree.to_hash_array(),
            flags: buffer,
        }
    }
}

impl ISerializable for MerkleBlockPayload {
    fn size(&self) -> usize {
        self.header.size()
            + mem::size_of::<u32>()
            + self.hashes.iter().map(|h| h.size()).sum::<usize>()
            + self.flags.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        self.header.serialize(writer).expect("TODO: panic message");
        writer.write_var_int(self.tx_count as u64);
        writer.write_serializable_list(&self.hashes);
        writer.write_var_bytes(&self.flags);
    }

    fn deserialize(reader: &mut BinaryReader) -> Result<Self, std::io::Error> {
        let header = Header::deserialize(reader, true)?;
        let tx_count = reader.read_var_int(u16::MAX as u64) as u32;
        let hashes = reader.read_serializable_list(tx_count as usize);
        let flags = reader.read_var_bytes((tx_count.max(1) + 7) / 8);

        Ok(MerkleBlockPayload { header, tx_count, hashes, flags })
    }
}
