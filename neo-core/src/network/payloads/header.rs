use std::io::{self, Write};
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::network::payloads::Witness;
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;
use neo_type::H256;
use getset::{CopyGetters, Getters, MutGetters, Setters};
use serde::Serialize;
use neo_json::jtoken::JToken;
use crate::ledger::header_cache::HeaderCache;
use crate::persistence::DataCache;
use crate::tx::Witnesses;

/// Represents a block header.
#[derive(Getters, Setters, MutGetters, CopyGetters, Clone, Default, Debug)]
pub struct Header {
    /// the hash of this block header
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    #[bin(ignore)]
    #[serde(rename = "hash")]
    pub hash: Option<H256>,

    /// the version of this block header
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    pub version: u32,

    /// the hash of the previous block.
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    #[serde(rename = "previousblockhash")]
    pub prev_hash: H256,

    /// the root hash of a transaction list.
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    #[serde(rename = "merkleroot")]
    pub merkle_root: H256,

    /// unix timestamp in milliseconds, i.e. timestamp
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    #[serde(rename = "time")]
    pub timestamp: u64,

    /// a random number
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    #[serde(serialize_with = "encode_hex_u64", deserialize_with = "decode_hex_u64")]
    pub nonce: u64,

    /// index/height of the block
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    pub index: u32,

    /// the index of the primary consensus node for this block.
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    pub primary: u8,

    /// contract address of the next miner
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    #[serde(rename = "nextconsensus")]
    pub next_consensus: H160,

    /// Script used to validate the block. Only one is supported at now.
    #[getset(get = "pub", set = "pub", get_mut = "pub")]
    pub witnesses: Witnesses,
}

impl Header {
    pub fn new() -> Self {
        Header {
            version: 0,
            prev_hash: H256::default(),
            merkle_root: H256::default(),
            nonce: 0,
            index: 0,
            next_consensus: H160::default(),
            hash: None,
            timestamp: 0,
            primary: 0,
            witnesses: Default::default(),
        }
    }

    pub fn size(&self) -> usize {
        std::mem::size_of::<u32>() +
        H256::LEN +
        H256::LEN +
        std::mem::size_of::<u64>() +
        std::mem::size_of::<u64>() +
        std::mem::size_of::<u32>() +
        std::mem::size_of::<u8>() +
        H160::LEN +
            1usize + self.witnesses.size() as usize
    }

    pub fn hash(&mut self) -> H256 {
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
        self.witnesses = witnesses[0].clone();
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
        self.primary = reader.read_u8()?;
        self.next_consensus = reader.read_serializable()?;
        Ok(())
    }

    pub fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.serialize_unsigned(writer)?;
        writer.write_all(&[1])?; // Write witness count
        self.witnesses.serialize(writer)?;
        Ok(())
    }

    pub fn serialize_unsigned(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.version.to_le_bytes())?;
        self.prev_hash.serialize(writer)?;
        self.merkle_root.serialize(writer)?;
        writer.write_all(&self.timestamp.to_le_bytes())?;
        writer.write_all(&self.nonce.to_le_bytes())?;
        writer.write_all(&self.index.to_le_bytes())?;
        writer.write_all(&[self.primary])?;
        self.next_consensus.serialize(writer)?;
        Ok(())
    }

    pub fn to_json(&self, settings: &ProtocolSettings) -> JToken {
        let mut json = JToken::new_object();
        json.insert("hash".to_string(), self.hash().to_string().into())
        .unwrap()
        .insert("size".to_string(), self.size().into())
        .unwrap()
        .insert("version".to_string(), self.version.into())
        .unwrap()
        .insert("previousblockhash".to_string(), self.prev_hash.to_string().into())
        .unwrap()
        .insert("merkleroot".to_string(), self.merkle_root.to_string())
        .unwrap()
        .insert("time".to_string(), self.timestamp as i64)
        .unwrap()
        .insert("nonce".to_string(), format!("{:016X}", self.nonce))
        .unwrap()
        .insert("index".to_string(), self.index as i64)
        .unwrap()
        .insert("primary".to_string(), self.primary as i64)
        .unwrap()
        .insert("nextconsensus".to_string(), self.next_consensus.to_address(settings.address_version()))
        .unwrap()
        .insert("witnesses".to_string(),  JToken::from(vec![self.witnesses.to_json()]))
        .unwrap();
        json
    }

    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache) -> bool {
        if self.primary >= settings.validators_count() {
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

    pub fn verify_with_cache(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, header_cache: &HeaderCache) -> bool {
        let prev = header_cache.last();
        if prev.is_none() {
            return self.verify(settings, snapshot);
        }
        let prev = prev.unwrap();
        if self.primary >= settings.validators_count() {
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
        self.verify_witness(settings, snapshot, &prev.next_consensus, &self.witnesses, 3_00000000).is_ok()
    }
}



impl PartialEq for Header {
    fn eq(&self, other: &Self) -> bool {
        self.hash() == other.hash()
    }
}

impl Eq for Header {}
