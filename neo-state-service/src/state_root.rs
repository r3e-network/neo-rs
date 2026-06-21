//! [`StateRoot`] - a state-root snapshot for a single block.
//!
//! Mirrors the C# `StateService.Network.StateRoot` record: a
//! `(version, block_index, root_hash, optional witness)` tuple that
//! validators publish alongside blocks to attest to the state
//! Merkle Patricia trie.

use neo_crypto::Crypto;
use neo_io::{BinaryWriter, IoResult};
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
}

#[cfg(test)]
#[path = "tests/state_root.rs"]
mod tests;
