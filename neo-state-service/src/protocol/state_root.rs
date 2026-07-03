//! [`StateRoot`] - a state-root snapshot for a single block.
//!
//! Mirrors the C# `StateService.Network.StateRoot` record: a
//! `(version, block_index, root_hash, optional witness)` tuple that
//! validators publish alongside blocks to attest to the state
//! Merkle Patricia trie.

use neo_crypto::Crypto;
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader};
use neo_payloads::Witness;
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Current state-root wire format version (matches C#
/// `StateService.Network.StateRoot.Version`).
pub const CURRENT_VERSION: u8 = 0x00;

/// A state-root snapshot for a single block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRoot {
    /// Wire format version.
    pub version: u8,
    /// Block index this state root corresponds to.
    pub index: u32,
    /// Root hash of the state Merkle Patricia trie.
    pub root_hash: UInt256,
    /// The StateValidators multisig witness attesting to this root, or `None` for
    /// a locally-computed (unsigned) root. Matches C# `StateRoot.Witness` — a
    /// single witness serialized as a 0-or-1-element var-array.
    pub witness: Option<Witness>,
    /// Cached SHA-256 hash of the unsigned state root.
    #[serde(skip)]
    cached_hash: Option<UInt256>,
}

impl StateRoot {
    /// Constructs a new `StateRoot` with explicit version.
    pub fn new(version: u8, index: u32, root_hash: UInt256) -> Self {
        Self {
            version,
            index,
            root_hash,
            witness: None,
            cached_hash: None,
        }
    }

    /// Constructs a new `StateRoot` using the current wire format
    /// version.
    pub fn new_current(index: u32, root_hash: UInt256) -> Self {
        Self::new(CURRENT_VERSION, index, root_hash)
    }

    /// Returns the wire-format version.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Returns the block index this state root corresponds to.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Returns the state Merkle Patricia trie root hash.
    pub fn root_hash(&self) -> &UInt256 {
        &self.root_hash
    }

    /// Computes (and caches) the SHA-256 hash of the unsigned
    /// state root bytes.
    pub fn hash(&mut self) -> UInt256 {
        if let Some(hash) = &self.cached_hash {
            return *hash;
        }
        let bytes = self.unsigned_bytes();
        let hash = UInt256::from(Crypto::sha256(&bytes));
        self.cached_hash = Some(hash);
        hash
    }

    /// Returns the unsigned wire-format bytes (no witness) used for
    /// the state-root hash.
    pub fn unsigned_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if self.serialize_unsigned(&mut writer).is_err() {
            tracing::warn!("StateRoot unsigned serialization failed");
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Serialises the unsigned state root (no witness) into the
    /// supplied writer.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.version)?;
        writer.write_u32(self.index)?;
        writer.write_bytes(&self.root_hash.to_bytes())?;
        Ok(())
    }

    /// Returns the attached StateValidators witness, if this root is signed.
    pub fn witness(&self) -> Option<&Witness> {
        self.witness.as_ref()
    }

    /// Attaches a StateValidators witness (the signed form).
    #[must_use]
    pub fn with_witness(mut self, witness: Witness) -> Self {
        self.witness = Some(witness);
        self
    }

    /// The bytes StateValidators sign over: `network magic (u32 LE) || Hash`,
    /// matching C# `IVerifiable.GetSignData(network)`.
    pub fn get_sign_data(&mut self, network: u32) -> Vec<u8> {
        let hash = self.hash();
        let mut data = Vec::with_capacity(4 + 32);
        data.extend_from_slice(&network.to_le_bytes());
        data.extend_from_slice(&hash.to_bytes());
        data
    }

    /// Full wire serialization: the unsigned fields followed by the witness as a
    /// 0-or-1-element var-array. Matches C# `StateRoot.Serialize`
    /// (`WriteVarInt(0)` when unsigned, else `Write(new[] { Witness })`).
    pub fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        match &self.witness {
            None => SerializeHelper::serialize_array::<Witness>(&[], writer)?,
            Some(w) => SerializeHelper::serialize_array(std::slice::from_ref(w), writer)?,
        }
        Ok(())
    }

    /// Full wire bytes (unsigned fields + witness var-array).
    pub fn to_array(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if self.serialize(&mut writer).is_err() {
            tracing::warn!("StateRoot serialization failed");
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Deserializes a full state root (unsigned fields + 0-or-1 witness),
    /// matching C# `StateRoot.Deserialize` (`ReadSerializableArray<Witness>(1)`).
    pub fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u8()?;
        let index = reader.read_u32()?;
        let root_hash = UInt256::from_bytes(&reader.read_bytes(32)?)
            .map_err(|_| IoError::invalid_data("StateRoot root hash"))?;
        let witnesses = SerializeHelper::deserialize_array::<Witness>(reader, 1)?;
        let witness = match witnesses.len() {
            0 => None,
            1 => witnesses.into_iter().next(),
            _ => return Err(IoError::invalid_data("StateRoot expects 0 or 1 witness")),
        };
        Ok(Self {
            version,
            index,
            root_hash,
            witness,
            cached_hash: None,
        })
    }
}

#[cfg(test)]
#[path = "../tests/protocol/state_root.rs"]
mod tests;
