use std::io::{self, Write};
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::Witness;
use crate::protocol_settings::ProtocolSettings;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// Represents a block header.
#[derive(Clone)]
pub struct Header {
    version: u32,
    prev_hash: UInt256,
    merkle_root: UInt256,
    timestamp: u64,
    nonce: u64,
    index: u32,
    primary_index: u8,
    next_consensus: UInt160,
    witness: Witness,
    hash: Option<UInt256>,
}

impl Header {
    pub fn new() -> Self {
        Header {
            version: 0,
            prev_hash: UInt256::default(),
            merkle_root: UInt256::default(),
            timestamp: 0,
            nonce: 0,
            index: 0,
            primary_index: 0,
            next_consensus: UInt160::default(),
            witness: Witness::default(),
            hash: None,
        }
    }

    pub fn size(&self) -> usize {
        std::mem::size_of::<u32>() +
        UInt256::LENGTH +
        UInt256::LENGTH +
        std::mem::size_of::<u64>() +
        std::mem::size_of::<u64>() +
        std::mem::size_of::<u32>() +
        std::mem::size_of::<u8>() +
        UInt160::LEN +
        1 + self.witness.size()
    }

    pub fn hash(&mut self) -> UInt256 {
        if self.hash.is_none() {
            self.hash = Some(self.calculate_hash());
        }
        self.hash.unwrap()
    }

    pub fn deserialize(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        self.deserialize_unsigned(reader)?;
        let witnesses = reader.read_serializable_array::<Witness>(1)?;
        if witnesses.len() != 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid witness count"));
        }
        self.witness = witnesses[0].clone();
        Ok(())
    }

    pub fn deserialize_unsigned(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        self.hash = None;
        self.version = reader.read_u32()?;
        if self.version > 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid version"));
        }
        self.prev_hash = reader.read_serializable()?;
        self.merkle_root = reader.read_serializable()?;
        self.timestamp = reader.read_u64()?;
        self.nonce = reader.read_u64()?;
        self.index = reader.read_u32()?;
        self.primary_index = reader.read_u8()?;
        self.next_consensus = reader.read_serializable()?;
        Ok(())
    }

    pub fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.serialize_unsigned(writer)?;
        writer.write_all(&[1])?; // Write witness count
        self.witness.serialize(writer)?;
        Ok(())
    }

    pub fn serialize_unsigned(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.version.to_le_bytes())?;
        self.prev_hash.serialize(writer)?;
        self.merkle_root.serialize(writer)?;
        writer.write_all(&self.timestamp.to_le_bytes())?;
        writer.write_all(&self.nonce.to_le_bytes())?;
        writer.write_all(&self.index.to_le_bytes())?;
        writer.write_all(&[self.primary_index])?;
        self.next_consensus.serialize(writer)?;
        Ok(())
    }

    pub fn to_json(&self, settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        json.insert("hash", self.hash().to_string());
        json.insert("size", self.size() as i64);
        json.insert("version", self.version as i64);
        json.insert("previousblockhash", self.prev_hash.to_string());
        json.insert("merkleroot", self.merkle_root.to_string());
        json.insert("time", self.timestamp as i64);
        json.insert("nonce", format!("{:016X}", self.nonce));
        json.insert("index", self.index as i64);
        json.insert("primary", self.primary_index as i64);
        json.insert("nextconsensus", self.next_consensus.to_address(settings.address_version()));
        json.insert("witnesses", JArray::from(vec![self.witness.to_json()]));
        json
    }

    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &DataCache) -> bool {
        if self.primary_index >= settings.validators_count() {
            return false;
        }
        let prev = NativeContract::ledger().get_trimmed_block(snapshot, &self.prev_hash);
        if prev.is_none() {
            return false;
        }
        let prev = prev.unwrap();
        if prev.index() + 1 != self.index {
            return false;
        }
        if prev.hash() != self.prev_hash {
            return false;
        }
        if prev.header().timestamp >= self.timestamp {
            return false;
        }
        if !self.verify_witnesses(settings, snapshot, 3_00000000) {
            return false;
        }
        true
    }

    pub fn verify_with_cache(&self, settings: &ProtocolSettings, snapshot: &DataCache, header_cache: &HeaderCache) -> bool {
        let prev = header_cache.last();
        if prev.is_none() {
            return self.verify(settings, snapshot);
        }
        let prev = prev.unwrap();
        if self.primary_index >= settings.validators_count() {
            return false;
        }
        if prev.hash() != self.prev_hash {
            return false;
        }
        if prev.index() + 1 != self.index {
            return false;
        }
        if prev.timestamp >= self.timestamp {
            return false;
        }
        self.verify_witness(settings, snapshot, &prev.next_consensus, &self.witness, 3_00000000).is_ok()
    }
}

impl PartialEq for Header {
    fn eq(&self, other: &Self) -> bool {
        self.hash() == other.hash()
    }
}

impl Eq for Header {}
