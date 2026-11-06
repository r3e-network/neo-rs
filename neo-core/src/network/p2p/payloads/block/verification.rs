use super::Block;
use crate::ledger::HeaderCache;
use crate::network::p2p::payloads::header::HeaderExt;
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;

/// Extension trait providing verification methods for [`Block`].
pub trait BlockExt {
    /// Verifies the block using persisted state.
    ///
    /// Matches C# Block.Verify, which delegates entirely to Header.Verify.
    /// Transaction Merkle-root and duplicate checks are performed while
    /// deserializing a block, not by this method.
    fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool;

    /// Verifies that the merkle root in the header matches the computed merkle root of transactions.
    /// This prevents transaction list tampering attacks.
    ///
    /// Returns `true` if:
    /// - The block has no transactions and merkle root is zero
    /// - The computed merkle root matches the header's merkle root
    ///
    /// Returns `false` if:
    /// - Any transaction hash cannot be computed (fail-closed)
    /// - The computed root doesn't match the header's root
    fn verify_merkle_root_consistency(&self) -> bool;

    /// Calculates the total network fee for all transactions in the block.
    fn calculate_network_fee(&self, snapshot: &DataCache) -> i64;

    /// Verifies the block using persisted state and cached headers.
    ///
    /// Matches C# Block.Verify's cached overload by delegating to Header.Verify.
    fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool;
}

impl BlockExt for Block {
    fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        self.header.verify(settings, store_cache)
    }

    fn verify_merkle_root_consistency(&self) -> bool {
        self.verify_merkle_root()
    }

    fn calculate_network_fee(&self, _snapshot: &DataCache) -> i64 {
        self.transactions.iter().map(|tx| tx.network_fee()).sum()
    }

    fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        self.header
            .verify_with_cache(settings, store_cache, header_cache)
    }
}
