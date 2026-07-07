//! Store-backed ledger reads for blockchain service queries.
//!
//! The service root owns construction and shared loop state; command dispatch
//! owns request routing. This module keeps the durable fallback read path
//! separate and forces it through the same [`StorageLedgerProvider`] used by
//! other ledger readers.

use super::{BlockchainService, MempoolLike};
use crate::ledger_provider::BlockProvider;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Resolve a block hash from the durable store for a height, when a
    /// store snapshot is available (cold read after LRU eviction).
    ///
    /// Routes through [`crate::ledger_provider::StorageLedgerProvider`] (the
    /// crate's sole ledger read path) instead of hand-rolling the
    /// native-contract call. The provider's `block_hash_by_index` is a direct
    /// `LedgerContract::get_block_hash` forward, so collapsing its `CoreResult`
    /// with `.ok().flatten()` preserves the prior "error becomes `None`"
    /// semantics byte-for-byte.
    pub(super) fn block_hash_from_store(&self, height: u32) -> Option<neo_primitives::UInt256> {
        let snapshot = self.system.store_snapshot()?;
        crate::ledger_provider::StorageLedgerProvider::new(snapshot.as_ref())
            .block_hash_by_index(height)
            .ok()
            .flatten()
    }

    /// Reconstruct a full block from the durable `LedgerContract` trimmed
    /// block plus its per-transaction records (C# `LedgerContract.GetBlock`),
    /// used when the in-memory LRU has evicted the body. Returns `None` when
    /// there is no store, no trimmed block, or any referenced transaction is
    /// missing.
    ///
    /// Routes through [`crate::ledger_provider::StorageLedgerProvider::block_by_hash`],
    /// which performs the identical trimmed-block + per-transaction
    /// reconstruction. A missing referenced transaction makes the provider
    /// return `Err`; collapsing with `.ok().flatten()` maps that to `None`,
    /// matching the prior behaviour where the `?` on the missing transaction
    /// short-circuited to `None`.
    pub(super) fn full_block_from_store(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::block::Block> {
        let snapshot = self.system.store_snapshot()?;
        crate::ledger_provider::StorageLedgerProvider::new(snapshot.as_ref())
            .block_by_hash(hash)
            .ok()
            .flatten()
    }
}
