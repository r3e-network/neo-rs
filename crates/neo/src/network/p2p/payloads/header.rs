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
use crate::neo_io::{MemoryReader, Serializable};
use crate::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Represents the header of a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        let mut data = Vec::new();
        self.serialize_unsigned(&mut data).unwrap();
        let hash = UInt256::from(neo_crypto::sha256(&data));
        self._hash = Some(hash);
        hash
    }

    /// Serialize without witness
    fn serialize_unsigned(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(self.prev_hash.as_bytes())?;
        writer.write_all(self.merkle_root.as_bytes())?;
        writer.write_all(&self.timestamp.to_le_bytes())?;
        writer.write_all(&self.nonce.to_le_bytes())?;
        writer.write_all(&self.index.to_le_bytes())?;
        writer.write_all(&[self.primary_index])?;
        writer.write_all(self.next_consensus.as_bytes())?;
        Ok(())
    }
}

impl Serializable for Header {
    fn size(&self) -> usize {
        4 + 32 + 32 + 8 + 8 + 4 + 1 + 20 + 1 + self.witness.size()
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.serialize_unsigned(writer)?;
        // Write witness count (always 1 for header)
        writer.write_all(&[1u8])?;
        self.witness.serialize(writer)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let version = reader.read_u32().map_err(|e| e.to_string())?;
        let prev_hash = UInt256::deserialize(reader)?;
        let merkle_root = UInt256::deserialize(reader)?;
        let timestamp = reader.read_u64().map_err(|e| e.to_string())?;
        let nonce = reader.read_u64().map_err(|e| e.to_string())?;
        let index = reader.read_u32().map_err(|e| e.to_string())?;
        let primary_index = reader.read_u8().map_err(|e| e.to_string())?;
        let next_consensus = UInt160::deserialize(reader)?;

        // Read witness count (should be 1)
        let witness_count = reader.read_var_int().map_err(|e| e.to_string())?;
        if witness_count != 1 {
            return Err("Invalid witness count for header".to_string());
        }

        let witness = Witness::deserialize(reader)?;

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
    fn get_script_hashes_for_verifying(&self, snapshot: &dyn crate::DataCache) -> Vec<UInt160> {
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
