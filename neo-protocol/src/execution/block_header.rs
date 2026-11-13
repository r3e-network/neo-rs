// List-witness execution header; shares only unsigned bytes with payload Header.
use crate::header_wire::UnsignedHeader;
use crate::witness::Witness;
use neo_crypto::Crypto;
use neo_io::serializable::helper::{deserialize_array, get_var_size, serialize_array};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Execution representation of a block header, with a witness list.
#[derive(Debug, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Format version.
    pub version: u32,
    /// Previous block hash.
    pub previous_hash: UInt256,
    /// Transaction merkle root.
    pub merkle_root: UInt256,
    /// Millisecond timestamp.
    pub timestamp: u64,
    /// Consensus nonce.
    pub nonce: u64,
    /// Block height.
    pub index: u32,
    /// Primary consensus node index.
    pub primary_index: u8,
    /// Next consensus address.
    pub next_consensus: UInt160,
    /// Header witness list (wire decoding requires exactly one).
    pub witnesses: Vec<Witness>,
    #[serde(skip)]
    _hash: Mutex<Option<UInt256>>,
}
impl Clone for BlockHeader {
    fn clone(&self) -> Self {
        Self {
            version: self.version,
            previous_hash: self.previous_hash,
            merkle_root: self.merkle_root,
            timestamp: self.timestamp,
            nonce: self.nonce,
            index: self.index,
            primary_index: self.primary_index,
            next_consensus: self.next_consensus,
            witnesses: self.witnesses.clone(),
            _hash: Mutex::new(*self._hash.lock()),
        }
    }
}
impl PartialEq for BlockHeader {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.previous_hash == other.previous_hash
            && self.merkle_root == other.merkle_root
            && self.timestamp == other.timestamp
            && self.nonce == other.nonce
            && self.index == other.index
            && self.primary_index == other.primary_index
            && self.next_consensus == other.next_consensus
            && self.witnesses == other.witnesses
    }
}
impl Eq for BlockHeader {}
impl BlockHeader {
    /// Creates a header with every normative field explicitly specified.
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
            _hash: Mutex::new(None),
        }
    }
    /// Serializes the common unsigned hash preimage.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.unsigned().write(writer)
    }
    /// Deserializes unsigned fields without witness framing.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let fields = UnsignedHeader::read(reader)?;
        if fields.version > 0 {
            return Err(IoError::invalid_data("Unsupported block header version"));
        }
        Ok(Self {
            version: fields.version,
            previous_hash: fields.previous_hash,
            merkle_root: fields.merkle_root,
            timestamp: fields.timestamp,
            nonce: fields.nonce,
            index: fields.index,
            primary_index: fields.primary_index,
            next_consensus: fields.next_consensus,
            witnesses: Vec::new(),
            _hash: Mutex::new(None),
        })
    }
    fn unsigned(&self) -> UnsignedHeader {
        UnsignedHeader {
            version: self.version,
            previous_hash: self.previous_hash,
            merkle_root: self.merkle_root,
            timestamp: self.timestamp,
            nonce: self.nonce,
            index: self.index,
            primary_index: self.primary_index,
            next_consensus: self.next_consensus,
        }
    }
    /// Gets the hash, failing closed on unsigned wire failure.
    pub fn hash(&self) -> UInt256 {
        self.try_hash()
            .unwrap_or_else(|err| panic!("BlockHeader unsigned serialization failed: {err}"))
    }
    /// Gets the cached single-SHA256 hash with low-level wire errors.
    pub fn try_hash(&self) -> IoResult<UInt256> {
        let mut hash_guard = self._hash.lock();
        if let Some(hash) = *hash_guard {
            return Ok(hash);
        }
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        let hash = UInt256::from(Crypto::sha256(&writer.into_bytes()));
        *hash_guard = Some(hash);
        Ok(hash)
    }
    /// Gets the block height.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Shared unsigned wire width.
    pub const UNSIGNED_WIRE_SIZE: usize = UnsignedHeader::WIRE_SIZE;
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
            _hash: Mutex::new(None),
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
        Self::UNSIGNED_WIRE_SIZE
            + get_var_size(self.witnesses.len() as u64)
            + self
                .witnesses
                .iter()
                .map(super::super::witness::Witness::size)
                .sum::<usize>()
    }
}
