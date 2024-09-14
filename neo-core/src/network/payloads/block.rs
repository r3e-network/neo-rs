use std::io::Write;
use crate::network::payloads::{Header, Transaction};
use crate::uint256::UInt256;
use getset::{CopyGetters, Getters, MutGetters, Setters};
use crate::cryptography::MerkleTree;
use crate::io::memory_reader::MemoryReader;
use crate::ledger::header_cache::HeaderCache;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;

/// Represents a block.
#[derive(Clone, Getters, Setters, MutGetters, CopyGetters, Default)]
pub struct Block {
    /// The header of the block.
    #[getset(get, set, get_mut)]
    pub header: Header,

    /// The transaction list of the block.
    #[getset(get, set, get_mut)]
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn hash(&self) -> UInt256 {
        self.header.hash()
    }

    pub fn version(&self) -> u32 {
        self.header.version()
    }

    pub fn prev_hash(&self) -> UInt256 {
        self.header.prev_hash()
    }

    pub fn merkle_root(&self) -> UInt256 {
        self.header.merkle_root()
    }

    pub fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }

    pub fn nonce(&self) -> u64 {
        self.header.nonce()
    }

    pub fn index(&self) -> u32 {
        self.header.index()
    }

    pub fn primary_index(&self) -> u8 {
        self.header.primary_index()
    }

    pub fn next_consensus(&self) -> UInt160 {
        self.header.next_consensus()
    }

    pub fn witness(&self) -> &Witness {
        self.header.witness()
    }

    pub fn size(&self) -> usize {
        self.header.size() + self.transactions.len() * std::mem::size_of::<Transaction>()
    }

    pub fn deserialize(&mut self, reader: &mut MemoryReader) -> Result<(), std::io::Error> {
        self.header = Header::deserialize(reader)?;
        self.transactions = reader.read_serializable_array(u16::MAX as usize)?;
        
        if self.transactions.iter().collect::<std::collections::HashSet<_>>().len() != self.transactions.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Duplicate transactions"));
        }

        let computed_root = MerkleTree::compute_root(&self.transactions.iter().map(|tx| tx.hash()).collect::<Vec<_>>());
        if computed_root != self.header.merkle_root() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid merkle root"));
        }

        Ok(())
    }

    pub fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), std::io::Error> {
        self.header.serialize(writer)?;
        for tx in &self.transactions {
            tx.serialize(writer)?;
        }
    }

    pub fn to_json(&self, settings: &ProtocolSettings) -> JObject {
        let mut json = self.header.to_json(settings);
        json.insert("size", self.size() as i64);
        json.insert("tx", self.transactions.iter().map(|tx| tx.to_json(settings)).collect::<Vec<_>>());
        json
    }

    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache) -> bool {
        self.header.verify(settings, snapshot)
    }

    pub fn verify_with_header_cache(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, header_cache: &HeaderCache) -> bool {
        self.header.verify_with_header_cache(settings, snapshot, header_cache)
    }
}



impl PartialEq for Block {
    fn eq(&mut self, other: &mut Self) -> bool {
        self.hash() == other.hash()
    }
}

impl Eq for Block {}

impl std::hash::Hash for Block {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash().hash(state);
    }
}

impl IInventory for Block {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Block
    }
}

impl IVerifiable for Block {
    fn witnesses(&self) -> &[Witness] {
        self.header.witnesses()
    }

    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        self.header.get_script_hashes_for_verifying(snapshot)
    }
}
