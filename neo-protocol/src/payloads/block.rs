// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use super::{Header, Transaction};
use crate::witness::{Witness, WitnessData};
use neo_crypto::MerkleTree;
use neo_io::{IoResult, Serializable};
use neo_primitives::error::{PrimitiveError, PrimitiveResult};
use neo_primitives::{Inventory, InventoryType, SerializablePayload, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::any::Any;
mod serialization;
mod structural;
pub use serialization::BLOCK_MAX_TX_WIRE_LIMIT;

/// A network payload block: a single-witness header and transaction list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// The payload header.
    pub header: Header,
    /// Included transactions.
    pub transactions: Vec<Transaction>,
}
impl Block {
    /// Creates a default block.
    pub fn new() -> Self {
        Self {
            header: Header::new(),
            transactions: Vec::new(),
        }
    }
    /// Gets the header hash.
    pub fn hash(&mut self) -> UInt256 {
        Header::hash(&mut self.header)
    }
    /// Gets the hash with low-level wire errors.
    pub fn try_hash(&mut self) -> IoResult<UInt256> {
        self.header.try_hash()
    }
    /// Gets the unsigned header hash preimage.
    pub fn try_get_hash_data(&self) -> IoResult<Vec<u8>> {
        self.header.try_get_hash_data()
    }
    /// Gets the block version.
    pub fn version(&self) -> u32 {
        self.header.version()
    }
    /// Gets the previous block hash.
    pub fn prev_hash(&self) -> &UInt256 {
        self.header.prev_hash()
    }
    /// Gets the transaction merkle root.
    pub fn merkle_root(&self) -> &UInt256 {
        self.header.merkle_root()
    }
    /// Gets the block timestamp.
    pub fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }
    /// Gets the consensus nonce.
    pub fn nonce(&self) -> u64 {
        self.header.nonce()
    }
    /// Gets the block height.
    pub fn index(&self) -> u32 {
        self.header.index()
    }
    /// Gets the primary consensus node index.
    pub fn primary_index(&self) -> u8 {
        self.header.primary_index()
    }
    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        self.header.next_consensus()
    }
    /// Gets the header witness.
    pub fn witness(&self) -> &Witness {
        &self.header.witness
    }
    /// Rebuilds the merkle root with the historical logging fallback.
    pub fn rebuild_merkle_root(&mut self) {
        if let Err(error) = self.try_rebuild_merkle_root() {
            tracing::error!(target: "neo::block", error = %error, "Failed to rebuild block merkle root");
        }
    }
    /// Rebuilds the merkle root, rejecting unserializable transaction hashes.
    pub fn try_rebuild_merkle_root(&mut self) -> IoResult<()> {
        if self.transactions.is_empty() {
            self.header.set_merkle_root(UInt256::default());
            return Ok(());
        }
        let payload_hashes = self.transaction_hashes()?;
        if let Some(root) = MerkleTree::compute_root(&payload_hashes) {
            self.header.set_merkle_root(root);
        }
        Ok(())
    }
}
impl neo_primitives::BlockLike for Block {
    type Transaction = Transaction;
    fn hash(&self) -> UInt256 {
        self.clone()
            .try_hash()
            .unwrap_or_else(|err| panic!("Block hash serialization failed (fail-closed): {err}"))
    }
    fn index(&self) -> u32 {
        self.header.index()
    }
    fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }
    fn prev_hash(&self) -> UInt256 {
        *self.header.prev_hash()
    }
    fn merkle_root(&self) -> UInt256 {
        *self.header.merkle_root()
    }
    fn transaction_count(&self) -> usize {
        self.transactions.len()
    }
    fn size(&self) -> usize {
        <Self as Serializable>::size(self)
    }
}
impl Inventory for Block {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Block
    }
}
impl neo_primitives::Verifiable for Block {
    fn verify(&self) -> bool {
        if self.header.version() > 0 {
            return false;
        }
        if !self.verify_merkle_root() {
            return false;
        }
        if !self.verify_no_duplicate_transactions() {
            return false;
        }
        true
    }
    fn hash(&self) -> PrimitiveResult<UInt256> {
        self.clone()
            .try_hash()
            .map_err(|e| PrimitiveError::invalid_data(format!("Codec error: {e}")))
    }
    fn hash_data(&self) -> Vec<u8> {
        self.header.hash_data()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl WitnessData for Block {
    // Header-only verification container, not aggregate SerializablePayload access.
    fn witnesses(&self) -> Vec<&Witness> {
        WitnessData::witnesses(&self.header)
    }
    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        WitnessData::witnesses_mut(&mut self.header)
    }
}
impl SerializablePayload for Block {
    fn hash_data(&self) -> Vec<u8> {
        self.header.hash_data()
    }
    fn witness_count(&self) -> usize {
        1 + self
            .transactions
            .iter()
            .map(|t| t.witness_count())
            .sum::<usize>()
    }
    fn invocation_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            return self.header.invocation_script(0);
        }
        let mut offset = 1;
        for tx in &self.transactions {
            let count = tx.witness_count();
            if index < offset + count {
                return tx.invocation_script(index - offset);
            }
            offset += count;
        }
        &[]
    }
    fn verification_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            return self.header.verification_script(0);
        }
        let mut offset = 1;
        for tx in &self.transactions {
            let count = tx.witness_count();
            if index < offset + count {
                return tx.verification_script(index - offset);
            }
            offset += count;
        }
        &[]
    }
}
impl Default for Block {
    fn default() -> Self {
        Self::new()
    }
}
