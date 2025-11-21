use crate::neo_crypto::hash256;
use crate::neo_io::serializable::helper::{deserialize_array, get_var_size, serialize_array};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::{UInt160, UInt256, Witness};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u32,
    pub previous_hash: UInt256,
    pub merkle_root: UInt256,
    pub timestamp: u64,
    pub nonce: u64,
    pub index: u32,
    pub primary_index: u8,
    pub next_consensus: UInt160,
    pub witnesses: Vec<Witness>,
}

impl BlockHeader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: u32,
        previous_hash: UInt256,
        merkle_root: UInt256,
        timestamp: u64,
        nonce: u64,
        index: u32,
        primary_index: u8,
        next_consensus: UInt160,
        witnesses: Vec<Witness>,
    ) -> Self {
        Self {
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses,
        }
    }

    /// Serializes the unsigned portion of the header (everything except witnesses).
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        writer.write_serializable(&self.previous_hash)?;
        writer.write_serializable(&self.merkle_root)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        writer.write_serializable(&self.next_consensus)?;
        Ok(())
    }

    /// Deserializes the unsigned portion of the header.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u32()?;
        if version > 0 {
            return Err(IoError::invalid_data("Unsupported block header version"));
        }

        let previous_hash = <UInt256 as Serializable>::deserialize(reader)?;
        let merkle_root = <UInt256 as Serializable>::deserialize(reader)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary_index = reader.read_u8()?;
        let next_consensus = <UInt160 as Serializable>::deserialize(reader)?;

        Ok(Self {
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses: Vec::new(),
        })
    }

    /// Computes the header hash (matches C# CalculateHash).
    pub fn hash(&self) -> UInt256 {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)
            .expect("block header serialization should not fail");
        // Ledger header hashes use double SHA256 (Hash256), identical to C# neo-core.
        UInt256::from(hash256(&writer.into_bytes()))
    }

    /// Returns the index (height) of the block header.
    pub fn index(&self) -> u32 {
        self.index
    }
}

impl Default for BlockHeader {
    fn default() -> Self {
        Self {
            version: 0,
            previous_hash: UInt256::zero(),
            merkle_root: UInt256::zero(),
            timestamp: 0,
            nonce: 0,
            index: 0,
            primary_index: 0,
            next_consensus: UInt160::zero(),
            witnesses: Vec::new(),
        }
    }
}

impl Serializable for BlockHeader {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut header = Self::deserialize_unsigned(reader)?;
        let witnesses: Vec<Witness> = deserialize_array(reader, 1)?;
        if witnesses.len() != 1 {
            return Err(IoError::invalid_data(
                "Block header must contain exactly one witness",
            ));
        }
        header.witnesses = witnesses;
        Ok(header)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        serialize_array(&self.witnesses, writer)
    }

    fn size(&self) -> usize {
        let mut size = 0;
        size += 4; // version
        size += UInt256::LENGTH; // previous_hash
        size += UInt256::LENGTH; // merkle_root
        size += 8; // timestamp
        size += 8; // nonce
        size += 4; // index
        size += 1; // primary index
        size += UInt160::LENGTH; // next consensus
        size += get_var_size(self.witnesses.len() as u64);
        size += self
            .witnesses
            .iter()
            .map(|witness| witness.size())
            .sum::<usize>();
        size
    }
}
