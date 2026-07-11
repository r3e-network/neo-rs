//! Store-backed ledger reads for blockchain service queries.
//!
//! The service root owns construction and shared loop state; command dispatch
//! owns request routing. This module keeps the durable fallback read path
//! separate and forces it through the same provider factory shape used by
//! other ledger readers.

use super::BlockchainService;
use crate::ledger_provider::BlockProvider;
use crate::service::MempoolLike;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Resolve a block hash from the durable store for a height, when a
    /// store snapshot is available (cold read after LRU eviction).
    ///
    /// Routes through the provider selected by [`SystemContext`], so a
    /// configured immutable archive remains visible after hot-row pruning.
    /// Collapsing its `CoreResult` with `.ok().flatten()` preserves the command
    /// API's existing error-to-miss behavior.
    pub(super) fn block_hash_from_store(&self, height: u32) -> Option<neo_primitives::UInt256> {
        let snapshot = self.system.store_snapshot()?;
        self.system
            .ledger_provider(snapshot.as_ref())
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
    /// A missing referenced transaction makes the selected provider return
    /// `Err`; collapsing with `.ok().flatten()` maps that to `None`, matching
    /// the command API's established behavior.
    pub(super) fn full_block_from_store(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::block::Block> {
        let snapshot = self.system.store_snapshot()?;
        self.system
            .ledger_provider(snapshot.as_ref())
            .block_by_hash(hash)
            .ok()
            .flatten()
    }
}
