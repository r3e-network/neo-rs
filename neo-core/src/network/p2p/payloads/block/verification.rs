use super::Block;
use crate::ledger::HeaderCache;
use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::{CoreResult, UInt256};

impl Block {
    /// Verifies the block using persisted state.
    ///
    /// Matches C# Block.Verify, which delegates entirely to Header.Verify.
    /// Transaction Merkle-root and duplicate checks are performed while
    /// deserializing a block, not by this method.
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        self.header.verify(settings, store_cache)
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

        match crate::cryptography::MerkleTree::compute_root(&tx_hashes) {
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
    /// Matches C# Block.Verify's cached overload by delegating to Header.Verify.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        self.header
            .verify_with_cache(settings, store_cache, header_cache)
    }
}
