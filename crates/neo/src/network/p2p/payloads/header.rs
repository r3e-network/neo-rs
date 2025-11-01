// Copyright (C) 2015-2025 The Neo Project.
//
// header.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::i_verifiable::IVerifiable;
use super::witness::Witness;
use crate::ledger::HeaderCache;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::{UInt160, UInt256};
use serde::{Deserialize, Serialize};

/// Represents the header of a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    version: u32,
    prev_hash: UInt256,
    merkle_root: UInt256,
    timestamp: u64,
    nonce: u64,
    index: u32,
    primary_index: u8,
    next_consensus: UInt160,

    /// The witness of the block.
    pub witness: Witness,

    #[serde(skip)]
    _hash: Option<UInt256>,
}

impl Header {
    /// Creates a new header.
    pub fn new() -> Self {
        Self {
            version: 0,
            prev_hash: UInt256::default(),
            merkle_root: UInt256::default(),
            timestamp: 0,
            nonce: 0,
            index: 0,
            primary_index: 0,
            next_consensus: UInt160::default(),
            witness: Witness::new(),
            _hash: None,
        }
    }

    /// Gets the version of the block.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Sets the version of the block.
    pub fn set_version(&mut self, value: u32) {
        self.version = value;
        self._hash = None;
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        &self.prev_hash
    }

    /// Sets the hash of the previous block.
    pub fn set_prev_hash(&mut self, value: UInt256) {
        self.prev_hash = value;
        self._hash = None;
    }

    /// Gets the merkle root of the transactions.
    pub fn merkle_root(&self) -> &UInt256 {
        &self.merkle_root
    }

    /// Sets the merkle root of the transactions.
    pub fn set_merkle_root(&mut self, value: UInt256) {
        self.merkle_root = value;
        self._hash = None;
    }

    /// Gets the timestamp of the block.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Sets the timestamp of the block.
    pub fn set_timestamp(&mut self, value: u64) {
        self.timestamp = value;
        self._hash = None;
    }

    /// Gets the nonce of the block.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Sets the nonce of the block.
    pub fn set_nonce(&mut self, value: u64) {
        self.nonce = value;
        self._hash = None;
    }

    /// Gets the index of the block.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Sets the index of the block.
    pub fn set_index(&mut self, value: u32) {
        self.index = value;
        self._hash = None;
    }

    /// Gets the primary index of the consensus node.
    pub fn primary_index(&self) -> u8 {
        self.primary_index
    }

    /// Sets the primary index of the consensus node.
    pub fn set_primary_index(&mut self, value: u8) {
        self.primary_index = value;
        self._hash = None;
    }

    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        &self.next_consensus
    }

    /// Sets the next consensus address.
    pub fn set_next_consensus(&mut self, value: UInt160) {
        self.next_consensus = value;
        self._hash = None;
    }

    /// Gets the hash of the header.
    pub fn hash(&mut self) -> UInt256 {
        if let Some(hash) = self._hash {
            return hash;
        }

        // Calculate hash from serialized data
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)
            .expect("header serialization should not fail");
        let hash = UInt256::from(crate::neo_crypto::sha256(&writer.into_bytes()));
        self._hash = Some(hash);
        hash
    }

    /// Serialize without witness
    fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        let prev_hash = self.prev_hash.as_bytes();
        writer.write_bytes(&prev_hash)?;
        let merkle_root = self.merkle_root.as_bytes();
        writer.write_bytes(&merkle_root)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        let next_consensus = self.next_consensus.as_bytes();
        writer.write_bytes(&next_consensus)?;
        Ok(())
    }
}

impl Header {
    /// Verifies the header using the provided store cache.
    pub fn verify(&self, _settings: &ProtocolSettings, _store_cache: &StoreCache) -> bool {
        // TODO: Port full header verification logic from C# implementation.
        true
    }

    /// Verifies the header using persisted state and cached headers.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        _header_cache: &HeaderCache,
    ) -> bool {
        self.verify(settings, store_cache)
    }
}

impl Serializable for Header {
    fn size(&self) -> usize {
        4 + 32
            + 32
            + 8
            + 8
            + 4
            + 1
            + 20
            + crate::neo_io::serializable::helper::get_var_size(1)
            + self.witness.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        // Write witness count (always 1 for header)
        writer.write_var_uint(1)?;
        writer.write_serializable(&self.witness)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u32()?;
        let prev_hash = <UInt256 as Serializable>::deserialize(reader)?;
        let merkle_root = <UInt256 as Serializable>::deserialize(reader)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary_index = reader.read_u8()?;
        let next_consensus = <UInt160 as Serializable>::deserialize(reader)?;

        // Read witness count (should be 1)
        let witness_count = reader.read_var_uint()?;
        if witness_count != 1 {
            return Err(IoError::invalid_data("Invalid witness count for header"));
        }

        let witness = <Witness as Serializable>::deserialize(reader)?;

        Ok(Self {
            version,
            prev_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witness,
            _hash: None,
        })
    }
}

impl IVerifiable for Header {
    fn get_script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        if self.prev_hash == UInt256::default() {
            return vec![self.witness.script_hash()];
        }

        // Get previous header and return its next_consensus
        // This would require access to the blockchain state
        vec![self.next_consensus]
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        vec![&self.witness]
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        vec![&mut self.witness]
    }
}

impl Default for Header {
    fn default() -> Self {
        Self::new()
    }
}
