// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use crate::witness::{Witness, WitnessData};
use neo_crypto::Crypto;
use neo_io::IoResult;
use neo_primitives::error::{PrimitiveError, PrimitiveResult};
use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
mod serialization;

/// A payload block header with exactly one witness.
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
    /// The header witness.
    pub witness: Witness,
    #[serde(skip)]
    _hash: Option<UInt256>,
}
impl Header {
    /// Creates a default header.
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
    /// Gets the format version.
    pub fn version(&self) -> u32 {
        self.version
    }
    /// Sets the version and invalidates the hash.
    pub fn set_version(&mut self, value: u32) {
        self.version = value;
        self._hash = None;
    }
    /// Gets the previous block hash.
    pub fn prev_hash(&self) -> &UInt256 {
        &self.prev_hash
    }
    /// Sets the previous hash and invalidates the hash.
    pub fn set_prev_hash(&mut self, value: UInt256) {
        self.prev_hash = value;
        self._hash = None;
    }
    /// Gets the transaction merkle root.
    pub fn merkle_root(&self) -> &UInt256 {
        &self.merkle_root
    }
    /// Sets the merkle root and invalidates the hash.
    pub fn set_merkle_root(&mut self, value: UInt256) {
        self.merkle_root = value;
        self._hash = None;
    }
    /// Gets the millisecond timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
    /// Sets the timestamp and invalidates the hash.
    pub fn set_timestamp(&mut self, value: u64) {
        self.timestamp = value;
        self._hash = None;
    }
    /// Gets the consensus nonce.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }
    /// Sets the nonce and invalidates the hash.
    pub fn set_nonce(&mut self, value: u64) {
        self.nonce = value;
        self._hash = None;
    }
    /// Gets the height.
    pub fn index(&self) -> u32 {
        self.index
    }
    /// Sets the height and invalidates the hash.
    pub fn set_index(&mut self, value: u32) {
        self.index = value;
        self._hash = None;
    }
    /// Gets the primary consensus node index.
    pub fn primary_index(&self) -> u8 {
        self.primary_index
    }
    /// Sets the primary index and invalidates the hash.
    pub fn set_primary_index(&mut self, value: u8) {
        self.primary_index = value;
        self._hash = None;
    }
    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        &self.next_consensus
    }
    /// Sets the next consensus address and invalidates the hash.
    pub fn set_next_consensus(&mut self, value: UInt160) {
        self.next_consensus = value;
        self._hash = None;
    }
    /// Gets the hash, failing closed on unsigned encoding failure.
    pub fn hash(&mut self) -> UInt256 {
        match self.try_hash() {
            Ok(hash) => hash,
            Err(err) => panic!("Header unsigned serialization failed: {err}"),
        }
    }
    /// Gets the cached single-SHA256 hash.
    pub fn try_hash(&mut self) -> IoResult<UInt256> {
        if let Some(hash) = self._hash {
            return Ok(hash);
        }
        let hash_data = self.try_get_hash_data()?;
        let hash = UInt256::from(Crypto::sha256(&hash_data));
        self._hash = Some(hash);
        Ok(hash)
    }
}
impl Default for Header {
    fn default() -> Self {
        Self::new()
    }
}
impl neo_primitives::Verifiable for Header {
    fn verify(&self) -> bool {
        true
    }
    fn hash(&self) -> PrimitiveResult<UInt256> {
        self.clone()
            .try_hash()
            .map_err(|e| PrimitiveError::invalid_data(format!("Codec error: {e}")))
    }
    fn hash_data(&self) -> Vec<u8> {
        Header::hash_data(self)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl WitnessData for Header {
    fn witnesses(&self) -> Vec<&Witness> {
        vec![&self.witness]
    }
    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        vec![&mut self.witness]
    }
}
impl neo_primitives::SerializablePayload for Header {
    fn hash_data(&self) -> Vec<u8> {
        Header::hash_data(self)
    }
    fn witness_count(&self) -> usize {
        1
    }
    fn invocation_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            self.witness.invocation_script.as_slice()
        } else {
            &[]
        }
    }
    fn verification_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            self.witness.verification_script.as_slice()
        } else {
            &[]
        }
    }
}

impl From<Header> for crate::execution::BlockHeader {
    fn from(header: Header) -> Self {
        crate::execution::BlockHeader::new(
            header.version,
            header.prev_hash,
            header.merkle_root,
            header.timestamp,
            header.nonce,
            header.index,
            header.primary_index,
            header.next_consensus,
            vec![header.witness],
        )
    }
}
