//! Store-backed ledger reads for blockchain service queries.
//!
//! The service root owns construction and shared loop state; command dispatch
//! owns request routing. This module keeps the durable fallback read path
//! separate and forces it through the same provider factory shape used by
//! other ledger readers.

use super::BlockchainService;
use crate::ledger_provider::{
    BlockProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
};
use crate::service::MempoolLike;

const STORE_READ_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Resolve a block hash from the durable store for a height, when a
    /// store snapshot is available (cold read after LRU eviction).
    ///
    /// Routes through the hot/cold ledger provider shape instead of
    /// hand-rolling the native-contract call. With [`EmptyLedgerProvider`] as
    /// the cold side, `block_hash_by_index` is still the hot native Ledger
    /// lookup, so collapsing its `CoreResult` with `.ok().flatten()` preserves
    /// the prior "error becomes `None`" semantics byte-for-byte.
    pub(super) fn block_hash_from_store(&self, height: u32) -> Option<neo_primitives::UInt256> {
        let snapshot = self.system.store_snapshot()?;
        STORE_READ_LEDGER_PROVIDER_FACTORY
            .provider(snapshot.as_ref())
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
    /// Routes through the hot/cold ledger provider shape, which currently uses
    /// the hot native Ledger reconstruction and an explicit clean-miss cold
    /// side. A missing referenced transaction makes the provider return `Err`;
    /// collapsing with `.ok().flatten()` maps that to `None`, matching the
    /// prior behaviour where the `?` on the missing transaction short-circuited
    /// to `None`.
    pub(super) fn full_block_from_store(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::block::Block> {
        let snapshot = self.system.store_snapshot()?;
        STORE_READ_LEDGER_PROVIDER_FACTORY
            .provider(snapshot.as_ref())
            .block_by_hash(hash)
            .ok()
            .flatten()
    }
}
