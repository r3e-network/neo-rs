//! State Root Implementation
//!
//! Matches C# Neo.Plugins.StateService.Network.StateRoot exactly.

use crate::cryptography::NeoHash;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::p2p::payloads::Witness;
use crate::UInt256;
use serde::{Deserialize, Serialize};

/// Current version of state root format.
pub const CURRENT_VERSION: u8 = 0x00;

/// Represents a state root for block state verification.
/// This matches the C# StateRoot class exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRoot {
    /// Version of the state root format.
    pub version: u8,
    /// Block index this state root corresponds to.
    pub index: u32,
    /// Root hash of the state Merkle trie.
    pub root_hash: UInt256,
    /// Witness for verification (optional until validated).
    #[serde(skip)]
    pub witness: Option<Witness>,
    /// Cached hash of the state root.
    #[serde(skip)]
    cached_hash: Option<UInt256>,
}

impl StateRoot {
    /// Creates a new state root.
    pub fn new(version: u8, index: u32, root_hash: UInt256) -> Self {
        Self {
            version,
            index,
            root_hash,
            witness: None,
            cached_hash: None,
        }
    }

    /// Creates a new state root with current version.
    pub fn new_current(index: u32, root_hash: UInt256) -> Self {
        Self::new(CURRENT_VERSION, index, root_hash)
    }

    /// Gets the hash of this state root (excluding witness).
    pub fn hash(&mut self) -> UInt256 {
        if let Some(hash) = &self.cached_hash {
            return *hash;
        }

        let unsigned_data = self.get_unsigned_data();
        let hash = UInt256::from_bytes(&NeoHash::hash256(&unsigned_data)).expect("Valid hash");
        self.cached_hash = Some(hash);
        hash
    }

    /// Gets the unsigned serialized data (for hashing).
    pub fn get_unsigned_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)
            .expect("Serialization should succeed");
        writer.into_bytes()
    }

    /// Serializes the unsigned portion of the state root.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_byte(self.version)?;
        writer.write_u32(self.index)?;
        writer.write_serializable(&self.root_hash)?;
        Ok(())
    }

    /// Deserializes the unsigned portion of the state root.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_byte()?;
        let index = reader.read_u32()?;
        let root_hash = <UInt256 as Serializable>::deserialize(reader)?;

        Ok(Self {
            version,
            index,
            root_hash,
            witness: None,
            cached_hash: None,
        })
    }

    /// Converts to JSON representation.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": self.version,
            "index": self.index,
            "roothash": self.root_hash.to_string(),
            "witnesses": match &self.witness {
                Some(w) => serde_json::json!([{
                    "invocation": hex::encode(&w.invocation_script),
                    "verification": hex::encode(&w.verification_script)
                }]),
                None => serde_json::json!([]),
            }
        })
    }
}

impl Serializable for StateRoot {
    fn size(&self) -> usize {
        1 + // version
        4 + // index
        32 + // root_hash
        match &self.witness {
            Some(w) => 1 + w.size(), // array prefix + witness
            None => 1, // empty array
        }
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        match &self.witness {
            Some(w) => {
                writer.write_var_uint(1)?;
                <Witness as Serializable>::serialize(w, writer)?;
            }
            None => {
                writer.write_var_uint(0)?;
            }
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut root = Self::deserialize_unsigned(reader)?;

        let witness_count = reader.read_var_int(1)?;
        root.witness = if witness_count == 0 {
            None
        } else if witness_count == 1 {
            Some(<Witness as Serializable>::deserialize(reader)?)
        } else {
            return Err(IoError::invalid_data(format!(
                "Expected 0 or 1 witness, got {}",
                witness_count
            )));
        };

        Ok(root)
    }
}

impl Default for StateRoot {
    fn default() -> Self {
        Self::new(CURRENT_VERSION, 0, UInt256::zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_root_creation() {
        let root_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let state_root = StateRoot::new_current(100, root_hash);

        assert_eq!(state_root.version, CURRENT_VERSION);
        assert_eq!(state_root.index, 100);
        assert_eq!(state_root.root_hash, root_hash);
        assert!(state_root.witness.is_none());
    }

    #[test]
    fn test_state_root_serialization() {
        use crate::neo_io::Serializable;
        let root_hash = UInt256::from_bytes(&[2u8; 32]).unwrap();
        let state_root = StateRoot::new_current(12345, root_hash);

        let mut writer = BinaryWriter::new();
        Serializable::serialize(&state_root, &mut writer).unwrap();
        let data = writer.into_bytes();

        let mut reader = MemoryReader::new(&data);
        let deserialized: StateRoot = Serializable::deserialize(&mut reader).unwrap();

        assert_eq!(deserialized.version, state_root.version);
        assert_eq!(deserialized.index, state_root.index);
        assert_eq!(deserialized.root_hash, state_root.root_hash);
    }
}
