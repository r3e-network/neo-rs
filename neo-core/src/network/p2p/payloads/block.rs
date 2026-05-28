// Copyright (C) 2015-2025 The Neo Project.
//
// block.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    header::Header, inventory::Inventory, transaction::Transaction, witness::Witness, InventoryType,
};
use crate::neo_io::Serializable;
use crate::persistence::DataCache;
use crate::{CoreResult, UInt160, UInt256};
use neo_primitives::error::PrimitiveResult;
use serde::{Deserialize, Serialize};
use std::any::Any;

mod serialization;
mod verification;

/// Represents a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// The header of the block.
    pub header: Header,

    /// The transaction list of the block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Creates a new block.
    pub fn new() -> Self {
        Self {
            header: Header::new(),
            transactions: Vec::new(),
        }
    }

    /// Gets the hash of the block.
    pub fn hash(&mut self) -> UInt256 {
        Header::hash(&mut self.header)
    }

    /// Gets the hash of the block, failing closed if the header cannot be
    /// serialized.
    pub fn try_hash(&mut self) -> CoreResult<UInt256> {
        self.header.try_hash()
    }

    /// Returns the unsigned header serialization used for block hashing.
    pub fn try_get_hash_data(&self) -> CoreResult<Vec<u8>> {
        self.header.try_get_hash_data()
    }

    /// Gets the version of the block.
    pub fn version(&self) -> u32 {
        self.header.version()
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        self.header.prev_hash()
    }

    /// Gets the merkle root of the transactions.
    pub fn merkle_root(&self) -> &UInt256 {
        self.header.merkle_root()
    }

    /// Gets the timestamp of the block.
    pub fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }

    /// Gets the nonce of the block.
    pub fn nonce(&self) -> u64 {
        self.header.nonce()
    }

    /// Gets the index of the block.
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    /// Gets the primary index of the consensus node.
    pub fn primary_index(&self) -> u8 {
        self.header.primary_index()
    }

    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        self.header.next_consensus()
    }

    /// Gets the witness of the block.
    pub fn witness(&self) -> &Witness {
        &self.header.witness
    }

    /// Calculates the network fee for the block.
    pub fn calculate_network_fee(&self, _snapshot: &DataCache) -> i64 {
        // Sum of all transaction network fees
        self.transactions.iter().map(|tx| tx.network_fee()).sum()
    }

    /// Rebuilds the merkle root.
    pub fn rebuild_merkle_root(&mut self) {
        if let Err(error) = self.try_rebuild_merkle_root() {
            tracing::error!(
                target: "neo::block",
                error = %error,
                "Failed to rebuild block merkle root"
            );
        }
    }

    /// Rebuilds the merkle root, failing closed if any transaction hash cannot
    /// be represented on the wire.
    pub fn try_rebuild_merkle_root(&mut self) -> CoreResult<()> {
        if self.transactions.is_empty() {
            self.header.set_merkle_root(UInt256::default());
            return Ok(());
        }
        let payload_hashes = self.transaction_hashes()?;
        if let Some(root) = crate::cryptography::MerkleTree::compute_root(&payload_hashes) {
            self.header.set_merkle_root(root);
        }
        Ok(())
    }
}

impl neo_primitives::BlockLike for Block {
    type Transaction = Transaction;

    fn hash(&self) -> UInt256 {
        let mut clone = self.clone();
        clone.try_hash().unwrap_or_default()
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
    /// Performs basic structural validation of the block.
    ///
    /// # Security Note
    /// This method performs basic structural checks only. For full cryptographic
    /// verification including witness validation and consensus checks, use the
    /// `verify()` method on the Block struct directly.
    ///
    /// # Checks Performed
    /// - Block has a valid header
    /// - Merkle root matches transactions
    /// - No duplicate transactions
    fn verify(&self) -> bool {
        // Basic structural validation (state-independent checks only)
        // Note: Full header verification requires ProtocolSettings and StoreCache,
        // which is done via Header::verify() separately.

        // 1. Basic header structural checks
        if self.header.version() > 0 {
            // Currently only version 0 is supported
            return false;
        }

        // 2. Verify merkle root matches transactions
        if !self.verify_merkle_root() {
            return false;
        }

        // 3. Verify no duplicate transactions
        if !self.verify_no_duplicate_transactions() {
            return false;
        }

        true
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        let mut clone = self.clone();
        clone.try_hash().map_err(|e| neo_primitives::error::PrimitiveError::invalid_data(e.to_string()))
    }

    fn hash_data(&self) -> Vec<u8> {
        self.header.hash_data()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl crate::VerifiableExt for Block {
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        self.header.script_hashes_for_verifying(snapshot)
    }

    fn witnesses(&self) -> Vec<&Witness> {
        self.header.witnesses()
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.header.witnesses_mut()
    }
}

impl neo_primitives::SerializablePayload for Block {
    fn hash_data(&self) -> Vec<u8> {
        self.header.hash_data()
    }

    fn witness_count(&self) -> usize {
        // Header witness + all transaction witnesses
        1 + self.transactions.iter().map(|t| t.witness_count()).sum::<usize>()
    }

    fn invocation_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            return self.header.invocation_script(0);
        }
        let mut offset = 1;
        for tx in &self.transactions {
            let tx_count = tx.witness_count();
            if index < offset + tx_count {
                return tx.invocation_script(index - offset);
            }
            offset += tx_count;
        }
        &[]
    }

    fn verification_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            return self.header.verification_script(0);
        }
        let mut offset = 1;
        for tx in &self.transactions {
            let tx_count = tx.witness_count();
            if index < offset + tx_count {
                return tx.verification_script(index - offset);
            }
            offset += tx_count;
        }
        &[]
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(Block);

#[cfg(test)]
mod tests {
    use super::super::signer::Signer;
    use super::*;
    use crate::WitnessScope;
    use neo_vm_rs::OpCode;

    fn sample_header() -> Header {
        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(UInt256::from_bytes(&[1; 32]).expect("prev hash"));
        header.set_merkle_root(UInt256::from_bytes(&[2; 32]).expect("merkle root"));
        header.set_timestamp(1_700_000_000_000);
        header.set_nonce(0x0102_0304_0506_0708);
        header.set_index(42);
        header.set_primary_index(1);
        header.set_next_consensus(UInt160::from_bytes(&[3; 20]).expect("next consensus"));
        header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
        header
    }

    fn sample_block() -> Block {
        Block {
            header: sample_header(),
            transactions: Vec::new(),
        }
    }

    fn transaction_with_oversized_script() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(100_000_000);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn block_try_hash_delegates_to_header() {
        let mut block = sample_block();
        let mut header = block.header.clone();

        assert_eq!(
            block.try_hash().expect("block hash"),
            header.try_hash().unwrap()
        );
    }

    #[test]
    fn iverifiable_block_hash_uses_try_hash() {
        let block = sample_block();
        let mut expected_source = block.clone();
        let expected = expected_source.try_hash().expect("try hash");

        assert_eq!(
            <Block as neo_primitives::Verifiable>::hash(&block).unwrap(),
            expected
        );
    }

    #[test]
    fn block_try_get_hash_data_matches_header_hash_data() {
        let block = sample_block();

        assert_eq!(
            block.try_get_hash_data().expect("block hash data"),
            block.header.try_get_hash_data().expect("header hash data")
        );
    }

    #[test]
    fn try_rebuild_merkle_root_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());

        assert!(block.try_rebuild_merkle_root().is_err());
    }

    #[test]
    fn verify_merkle_root_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());
        block.header.set_merkle_root(UInt256::default());

        assert!(!block.verify_merkle_root());
    }

    #[test]
    fn duplicate_transaction_check_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());

        assert!(!block.verify_no_duplicate_transactions());
    }
}
