use super::Block;
use crate::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use crate::ledger::{HeaderCache, TransactionVerificationContext, VerifyResult};
use crate::neo_io::Serializable;
use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::validation::{
    validate_block_size, validate_timestamp_bounds, validate_transaction_count,
    validate_witness_scripts,
};
use crate::{CoreResult, UInt256};

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

    fn transaction_hash_for_log(tx: &super::super::transaction::Transaction) -> String {
        tx.try_hash()
            .map(|hash| hash.to_string())
            .unwrap_or_else(|error| format!("<unhashable: {error}>"))
    }

    /// Verifies that the merkle root in the header matches the computed merkle root of transactions.
    /// This prevents transaction list tampering attacks.
    ///
    /// Performance: Uses cached transaction hashes via interior mutability (Mutex) to avoid
    /// redundant hash computations. No cloning required.
    pub(super) fn verify_merkle_root(&self) -> bool {
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

        match neo_crypto::MerkleTree::compute_root(&tx_hashes) {
            Some(computed_root) => computed_root == *self.header.merkle_root(),
            None => false, // Should not happen with non-empty transactions
        }
    }

    /// Verifies that there are no duplicate transaction hashes in the block.
    ///
    /// Performance: Uses cached transaction hashes via interior mutability (Mutex).
    /// No cloning required.
    pub(super) fn verify_no_duplicate_transactions(&self) -> bool {
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

    pub(super) fn transaction_hashes(&self) -> CoreResult<Vec<UInt256>> {
        self.transactions.iter().map(|tx| tx.try_hash()).collect()
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
