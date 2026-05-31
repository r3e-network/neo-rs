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

use super::witness::Witness;
use crate::error::CoreResult;
use crate::neo_io::Serializable;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use crate::{UInt160, UInt256};
use neo_primitives::error::PrimitiveResult;
use serde::{Deserialize, Serialize};

mod serialization;
mod verification;

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
        match self.try_hash() {
            Ok(hash) => hash,
            Err(err) => {
                tracing::error!("Header unsigned serialization failed: {err}");
                UInt256::zero()
            }
        }
    }

    /// Serializes this header to its canonical Neo RPC JSON object, matching C#
    /// `Header.ToJson` (field set and ordering: hash, size, version,
    /// previousblockhash, merkleroot, time, nonce as `{:016X}`, index, primary,
    /// nextconsensus, witnesses). This is the single source of truth for the
    /// header wire-JSON shared by the RPC server and client; callers that serve
    /// `getblock`/`getblockheader` add the contextual `confirmations` and
    /// optional `nextblockhash` fields on top.
    pub fn to_json(&self, settings: &ProtocolSettings) -> serde_json::Map<String, serde_json::Value> {
        use serde_json::{json, Value};
        let hash = self.clone().hash();
        let mut json = serde_json::Map::new();
        json.insert("hash".to_string(), Value::String(hash.to_string()));
        json.insert("size".to_string(), json!(self.size()));
        json.insert("version".to_string(), json!(self.version()));
        json.insert(
            "previousblockhash".to_string(),
            Value::String(self.prev_hash().to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            Value::String(self.merkle_root().to_string()),
        );
        json.insert("time".to_string(), json!(self.timestamp()));
        json.insert(
            "nonce".to_string(),
            Value::String(format!("{:016X}", self.nonce())),
        );
        json.insert("index".to_string(), json!(self.index()));
        json.insert("primary".to_string(), json!(self.primary_index()));
        json.insert(
            "nextconsensus".to_string(),
            Value::String(
                self.next_consensus()
                    .to_address_with_version(settings.address_version),
            ),
        );
        json.insert(
            "witnesses".to_string(),
            Value::Array(vec![self.witness.to_json()]),
        );
        json
    }

    /// Gets the hash of the header, failing closed if unsigned serialization
    /// fails.
    pub fn try_hash(&mut self) -> CoreResult<UInt256> {
        if let Some(hash) = self._hash {
            return Ok(hash);
        }

        let hash_data = self.try_get_hash_data()?;
        // Neo N3 block hashes use single SHA-256 over the unsigned header payload.
        let hash = UInt256::from(neo_crypto::Crypto::sha256(&hash_data));
        self._hash = Some(hash);
        Ok(hash)
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(Header);

impl neo_primitives::Verifiable for Header {
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        let mut clone = self.clone();
        clone.try_hash().map_err(|e| neo_primitives::error::PrimitiveError::invalid_data(e.to_string()))
    }

    fn hash_data(&self) -> Vec<u8> {
        Header::hash_data(self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::VerifiableExt for Header {
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        if self.prev_hash == UInt256::default() {
            return vec![self.witness.script_hash()];
        }

        let ledger = LedgerContract::new();
        let prev = match ledger.get_trimmed_block(snapshot, &self.prev_hash) {
            Ok(Some(prev)) => prev,
            Ok(None) => {
                tracing::warn!(
                    prev_hash = %self.prev_hash,
                    "previous header not found when verifying header"
                );
                return Vec::new();
            }
            Err(err) => {
                tracing::warn!(
                    prev_hash = %self.prev_hash,
                    error = %err,
                    "failed to fetch previous header when verifying header"
                );
                return Vec::new();
            }
        };

        vec![prev.header.next_consensus]
    }

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

#[cfg(test)]
mod tests;
