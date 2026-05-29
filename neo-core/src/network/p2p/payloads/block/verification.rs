use super::Block;
use crate::ledger::HeaderCache;
use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::{CoreResult, UInt256};

#[derive(Clone, Copy)]
enum BlockVerifyMode<'a> {
    Full,
    Cached(&'a HeaderCache),
}

impl Block {
    /// Verifies the block. Matches C# `Block.Verify`, which is defined as
    /// `return Header.Verify(...)` — i.e. block verification is exactly header
    /// verification (primary index, chain continuity, strictly-increasing
    /// timestamp, and the consensus witness).
    ///
    /// Transaction integrity is enforced where C# enforces it:
    /// - merkle-root match and no-duplicate-tx-hash at deserialization time
    ///   (see `Block::deserialize`), mirroring C# `DeserializeTransactions`;
    /// - per-transaction validity is guaranteed by consensus pre-verification
    ///   (the header witness proves a valid validator quorum signed the block),
    ///   so C# does NOT re-verify each transaction on block import — and neither
    ///   do we. Transactions are still executed during persistence.
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        self.verify_internal(settings, store_cache, BlockVerifyMode::Full)
    }

    /// Verifies the block using persisted state and cached headers.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        self.verify_internal(settings, store_cache, BlockVerifyMode::Cached(header_cache))
    }

    fn verify_internal(
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

    /// Verifies the merkle root in the header matches the computed root of the
    /// transactions. Authoritative gate runs at deserialization (C# parity).
    pub(super) fn verify_merkle_root(&self) -> bool {
        // Empty transactions => zero merkle root (matches C# MerkleTree.ComputeRoot([])).
        if self.transactions.is_empty() {
            return *self.header.merkle_root() == UInt256::default();
        }

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
            None => false,
        }
    }

    /// Verifies there are no duplicate transaction hashes in the block.
    /// Authoritative gate runs at deserialization (C# parity).
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
}
