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
    header::Header, inventory::Inventory, transaction::Transaction, witness::Witness,
    InventoryType,
};
use crate::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use crate::ledger::{HeaderCache, TransactionVerificationContext, VerifyResult};
use crate::neo_io::Serializable;
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::validation::{
    validate_block_size, validate_timestamp_bounds, validate_transaction_count,
    validate_witness_scripts,
};
use crate::{CoreResult, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::any::Any;

mod serialization;

/// Represents a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// The header of the block.
    pub header: Header,

    /// The transaction list of the block.
    pub transactions: Vec<Transaction>,
}

#[derive(Clone, Copy)]
enum BlockVerifyMode<'a> {
    Full,
    Cached(&'a HeaderCache),
}

impl BlockVerifyMode<'_> {
    fn is_cached(self) -> bool {
        matches!(self, Self::Cached(_))
    }
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

    /// Verifies the block using persisted state with comprehensive security checks.
    ///
    /// Performs the following validations (matches C# Block.Verify):
    /// 1. Block size validation (max 4 MB)
    /// 2. Transaction count validation (max 65535)
    /// 3. Timestamp bounds validation (within 15 minutes of current time)
    /// 4. Header validation (timestamp, consensus, witness, etc.)
    /// 5. Witness script validation
    /// 6. Merkle root validation - ensures transactions haven't been tampered
    /// 7. Transaction uniqueness - no duplicate transaction hashes
    /// 8. Per-transaction validation (structural + state-dependent) against ledger snapshot
    ///
    /// # Security Note
    /// This method includes comprehensive validation to prevent blocks with
    /// invalid transactions, oversized data, or malicious timestamps from being accepted.
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        self.verify_internal(settings, store_cache, BlockVerifyMode::Full)
    }

    fn verify_internal(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        mode: BlockVerifyMode<'_>,
    ) -> bool {
        if !self.verify_block_size(mode.is_cached()) {
            return false;
        }

        if !self.verify_transaction_count(mode.is_cached()) {
            return false;
        }

        if !self.verify_timestamp_bounds(mode.is_cached()) {
            return false;
        }

        if !self.verify_header_for_mode(settings, store_cache, mode) {
            return false;
        }

        if !self.verify_witness_scripts(mode.is_cached()) {
            return false;
        }

        if !self.verify_merkle_root() {
            self.log_verify_failure("Merkle root validation failed", mode.is_cached());
            return false;
        }

        if !self.verify_no_duplicate_transactions() {
            self.log_verify_failure("Duplicate transaction check failed", mode.is_cached());
            return false;
        }

        self.verify_transactions(settings, store_cache)
    }

    fn verify_block_size(&self, cached: bool) -> bool {
        if validate_block_size(self).is_err() {
            if cached {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    block_size = self.size(),
                    max_size = MAX_BLOCK_SIZE,
                    "Block size validation failed (cached)"
                );
            } else {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    block_size = self.size(),
                    max_size = MAX_BLOCK_SIZE,
                    "Block size validation failed"
                );
            }
            return false;
        }
        true
    }

    fn verify_transaction_count(&self, cached: bool) -> bool {
        if validate_transaction_count(self).is_err() {
            if cached {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    tx_count = self.transactions.len(),
                    max_count = MAX_TRANSACTIONS_PER_BLOCK,
                    "Transaction count validation failed (cached)"
                );
            } else {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    tx_count = self.transactions.len(),
                    max_count = MAX_TRANSACTIONS_PER_BLOCK,
                    "Transaction count validation failed"
                );
            }
            return false;
        }
        true
    }

    fn verify_timestamp_bounds(&self, cached: bool) -> bool {
        if validate_timestamp_bounds(self.timestamp()).is_err() {
            if cached {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    timestamp = self.timestamp(),
                    "Timestamp bounds validation failed (cached)"
                );
            } else {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    timestamp = self.timestamp(),
                    "Timestamp bounds validation failed"
                );
            }
            return false;
        }
        true
    }

    fn verify_header_for_mode(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        mode: BlockVerifyMode<'_>,
    ) -> bool {
        match mode {
            BlockVerifyMode::Full => self.header.verify(settings, store_cache),
            BlockVerifyMode::Cached(header_cache) => {
                self.header
                    .verify_with_cache(settings, store_cache, header_cache)
            }
        }
    }

    fn verify_witness_scripts(&self, cached: bool) -> bool {
        if validate_witness_scripts(&self.header).is_err() {
            self.log_verify_failure("Witness script validation failed", cached);
            return false;
        }
        true
    }

    fn log_verify_failure(&self, message: &'static str, cached: bool) {
        if cached {
            tracing::warn!(
                target: "neo::block",
                block_index = self.header.index(),
                "{} (cached)",
                message
            );
        } else {
            tracing::warn!(
                target: "neo::block",
                block_index = self.header.index(),
                "{}",
                message
            );
        }
    }

    /// Verifies all transactions in the block using full validation (state-independent
    /// and state-dependent) against the current ledger snapshot.
    fn verify_transactions(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        use rayon::prelude::*;

        // Phase 1: parallel state-independent verification (includes signatures).
        // This is pure computation with no shared mutable state.
        let block_index = self.header.index();
        let failed = self
            .transactions
            .par_iter()
            .enumerate()
            .find_map_any(|(index, tx)| {
                let result = tx.verify_state_independent(settings);
                (result != VerifyResult::Succeed).then_some((index, tx, result))
            });

        if let Some((index, tx, result)) = failed {
            tracing::warn!(
                target: "neo::block",
                block_index,
                tx_index = index,
                tx_hash = %Self::transaction_hash_for_log(tx),
                result = ?result,
                "Transaction failed state-independent verification"
            );
            return false;
        }

        // Phase 2: sequential state-dependent verification (needs shared context).
        // Pass `block.index() - 1` as the explicit current_height to avoid the
        // fast-sync snapshot bug where `Ledger.current_index(snapshot)` spuriously
        // returns 0, wrongly rejecting valid txs as Expired.
        let snapshot = store_cache.data_cache();
        let block_height_for_expiry = self.header.index().saturating_sub(1);
        let mut context = TransactionVerificationContext::new();
        for (index, tx) in self.transactions.iter().enumerate() {
            let result = tx.verify_state_dependent_at_height(
                settings,
                snapshot,
                block_height_for_expiry,
                Some(&context),
                &[],
            );
            if result != VerifyResult::Succeed {
                tracing::warn!(
                    target: "neo::block",
                    block_index,
                    tx_index = index,
                    tx_hash = %Self::transaction_hash_for_log(tx),
                    result = ?result,
                    "Transaction failed state-dependent verification"
                );
                return false;
            }
            context.add_transaction(tx);
        }
        true
    }

    fn transaction_hash_for_log(tx: &Transaction) -> String {
        tx.try_hash()
            .map(|hash| hash.to_string())
            .unwrap_or_else(|error| format!("<unhashable: {error}>"))
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
        let tx_hashes = match self.transaction_hashes() {
            Ok(hashes) => hashes,
            Err(error) => {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    error = %error,
                    "Failed to compute transaction hashes for merkle root"
                );
                return false;
            }
        };

        match crate::cryptography::MerkleTree::compute_root(&tx_hashes) {
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
            let hash = match tx.try_hash() {
                Ok(hash) => hash,
                Err(error) => {
                    tracing::warn!(
                        target: "neo::block",
                        block_index = self.header.index(),
                        error = %error,
                        "Failed to compute transaction hash for duplicate check"
                    );
                    return false;
                }
            };
            if !seen.insert(hash) {
                return false; // Duplicate transaction found
            }
        }
        true
    }

    fn transaction_hashes(&self) -> CoreResult<Vec<UInt256>> {
        self.transactions
            .iter()
            .map(Transaction::try_hash)
            .collect()
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
        self.verify_internal(settings, store_cache, BlockVerifyMode::Cached(header_cache))
    }
}

impl crate::validation::BlockLike for Block {
    fn size(&self) -> usize {
        <Self as Serializable>::size(self)
    }

    fn transaction_count(&self) -> usize {
        self.transactions.len()
    }
}

impl Inventory for Block {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Block
    }

    fn hash(&mut self) -> UInt256 {
        Header::hash(&mut self.header)
    }
}

impl crate::Verifiable for Block {
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
        clone.try_hash()
    }

    fn get_hash_data(&self) -> Vec<u8> {
        self.header.get_hash_data()
    }

    fn as_any(&self) -> &dyn Any {
        self
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
            <Block as crate::Verifiable>::hash(&block).unwrap(),
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
