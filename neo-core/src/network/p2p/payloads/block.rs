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
    header::Header, i_inventory::IInventory, inventory_type::InventoryType,
    transaction::Transaction, witness::Witness,
};
use crate::ledger::HeaderCache;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::{CoreResult, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::any::Any;

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
        if self.transactions.is_empty() {
            self.header.set_merkle_root(UInt256::default());
            return;
        }
        let payload_hashes: Vec<UInt256> =
            self.transactions.iter_mut().map(|tx| tx.hash()).collect();
        if let Some(root) = crate::neo_cryptography::MerkleTree::compute_root(&payload_hashes) {
            self.header.set_merkle_root(root);
        }
    }

    /// Verifies the block using persisted state.
    ///
    /// Performs the following validations (matches C# Block.Verify):
    /// 1. Header validation (timestamp, consensus, witness, etc.)
    /// 2. Merkle root validation - ensures transactions haven't been tampered
    /// 3. Transaction uniqueness - no duplicate transaction hashes
    /// 4. Per-transaction state-independent validation (size, script, attributes, etc.)
    ///
    /// # Security Note
    /// This method now includes per-transaction validation to prevent blocks with
    /// invalid transactions from being accepted. Previously, only header/merkle/duplicate
    /// checks were performed, allowing malformed transactions to pass verification.
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        // Step 1: Verify header first
        if !self.header.verify(settings, store_cache) {
            return false;
        }

        // Step 2: Verify Merkle Root matches transactions
        if !self.verify_merkle_root() {
            return false;
        }

        // Step 3: Verify no duplicate transactions
        if !self.verify_no_duplicate_transactions() {
            return false;
        }

        // Step 4: SECURITY FIX - Verify each transaction (state-independent checks)
        // This prevents blocks with malformed transactions from being accepted.
        // State-independent checks include: size limits, script validity, attribute
        // validity, signer/witness count matching, and other structural checks.
        if !self.verify_transactions_state_independent(settings) {
            return false;
        }

        true
    }

    /// Verifies all transactions in the block using state-independent checks.
    ///
    /// # Security Note
    /// This method validates each transaction's structure without requiring
    /// blockchain state. It catches malformed transactions that could otherwise
    /// be included in blocks by malicious peers.
    ///
    /// # Checks Performed
    /// - Transaction size limits
    /// - Script validity
    /// - Attribute validity
    /// - Signer/witness count matching
    /// - Fee validation
    /// - Validity period checks
    fn verify_transactions_state_independent(&self, settings: &ProtocolSettings) -> bool {
        use crate::ledger::verify_result::VerifyResult;

        for (index, tx) in self.transactions.iter().enumerate() {
            let result = tx.verify_state_independent(settings);
            if result != VerifyResult::Succeed {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    tx_index = index,
                    tx_hash = %tx.hash(),
                    result = ?result,
                    "Transaction failed state-independent verification"
                );
                return false;
            }
        }
        true
    }

    /// Verifies that the merkle root in the header matches the computed merkle root of transactions.
    /// This prevents transaction list tampering attacks.
    ///
    /// Performance: Uses cached transaction hashes via interior mutability (Mutex) to avoid
    /// redundant hash computations. No cloning required.
    fn verify_merkle_root(&self) -> bool {
        // Empty transactions should have zero merkle root
        if self.transactions.is_empty() {
            return *self.header.merkle_root() == UInt256::default();
        }

        // Compute merkle root from transaction hashes.
        // Transaction::hash() uses interior mutability (Mutex) to cache the hash,
        // so we can call it on &self without cloning.
        let tx_hashes: Vec<UInt256> = self.transactions.iter().map(|tx| tx.hash()).collect();

        match crate::neo_cryptography::MerkleTree::compute_root(&tx_hashes) {
            Some(computed_root) => computed_root == *self.header.merkle_root(),
            None => false, // Should not happen with non-empty transactions
        }
    }

    /// Verifies that there are no duplicate transaction hashes in the block.
    ///
    /// Performance: Uses cached transaction hashes via interior mutability (Mutex).
    /// No cloning required.
    fn verify_no_duplicate_transactions(&self) -> bool {
        let mut seen = std::collections::HashSet::with_capacity(self.transactions.len());
        for tx in &self.transactions {
            // Transaction::hash() uses interior mutability to cache the hash.
            if !seen.insert(tx.hash()) {
                return false; // Duplicate transaction found
            }
        }
        true
    }

    /// Verifies the block using persisted state and cached headers.
    ///
    /// Performs the same validations as `verify` but uses the header cache for efficiency.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        // Step 1: Verify header with cache
        if !self
            .header
            .verify_with_cache(settings, store_cache, header_cache)
        {
            return false;
        }

        // Step 2: Verify Merkle Root matches transactions
        if !self.verify_merkle_root() {
            return false;
        }

        // Step 3: Verify no duplicate transactions
        if !self.verify_no_duplicate_transactions() {
            return false;
        }

        // Step 4: SECURITY FIX - Verify each transaction (state-independent checks)
        if !self.verify_transactions_state_independent(settings) {
            return false;
        }

        true
    }
}

impl IInventory for Block {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Block
    }

    fn hash(&mut self) -> UInt256 {
        Header::hash(&mut self.header)
    }
}

impl crate::IVerifiable for Block {
    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        self.header.get_script_hashes_for_verifying(snapshot)
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        self.header.get_witnesses()
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.header.get_witnesses_mut()
    }

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

    fn hash(&self) -> CoreResult<UInt256> {
        let mut clone = self.clone();
        Ok(Block::hash(&mut clone))
    }

    fn get_hash_data(&self) -> Vec<u8> {
        self.header.get_hash_data()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Serializable for Block {
    fn size(&self) -> usize {
        self.header.size()
            + get_var_size(self.transactions.len() as u64)
            + self.transactions.iter().map(|tx| tx.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        const MAX_TRANSACTIONS: u64 = u16::MAX as u64;
        if self.transactions.len() as u64 > MAX_TRANSACTIONS {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        writer.write_var_uint(self.transactions.len() as u64)?;

        // Write transactions
        for tx in &self.transactions {
            writer.write_serializable(tx)?;
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        use crate::constants::MAX_BLOCK_SIZE;

        let header = <Header as Serializable>::deserialize(reader)?;
        let header_size = header.size();

        // Read transaction count
        const MAX_TRANSACTIONS: u64 = u16::MAX as u64;
        let tx_count = reader.read_var_int(MAX_TRANSACTIONS)? as usize;
        if tx_count as u64 > MAX_TRANSACTIONS {
            return Err(IoError::invalid_data("Too many transactions"));
        }

        // Track cumulative size to prevent DoS attacks
        // MAX_BLOCK_SIZE is 2MB (2,097,152 bytes)
        let mut cumulative_size = header_size + get_var_size(tx_count as u64);
        if cumulative_size > MAX_BLOCK_SIZE {
            return Err(IoError::invalid_data("Block size exceeds maximum"));
        }

        let mut transactions = Vec::with_capacity(tx_count.min(512)); // Cap initial capacity
        for _ in 0..tx_count {
            let tx = <Transaction as Serializable>::deserialize(reader)?;
            cumulative_size += tx.size();

            // Check cumulative size before accepting transaction
            if cumulative_size > MAX_BLOCK_SIZE {
                return Err(IoError::invalid_data("Block size exceeds maximum"));
            }

            transactions.push(tx);
        }

        Ok(Self {
            header,
            transactions,
        })
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(Block);
