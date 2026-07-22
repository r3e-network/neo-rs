//! Minimal mempool adapter boundary for the blockchain service.
//!
//! The blockchain service only needs a narrow admission/reverification facade.
//! The full mempool implementation remains in `neo-mempool`; this module keeps
//! the service construction root focused on command-loop state while preserving
//! a small mockable boundary for pipeline and service tests.

use neo_execution::native_contract_provider::NativeContractProvider;

/// Minimal mempool facade used by the high-level service API.
///
/// The trait exists so the blockchain service can be unit-tested with a mock
/// mempool; the production implementation forwards to the real `MemoryPool`
/// type. The shape is intentionally tiny: verification context, conflict
/// attribute detection, and reverify queues stay in `neo-mempool` and are
/// exposed to the service through [`crate::service_context::SystemContext`].
pub trait MempoolLike: std::fmt::Debug + Send + Sync {
    /// Validate and atomically add a transaction to the mempool.
    fn add_transaction<B, L>(
        &self,
        origin: neo_mempool::TransactionOrigin,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache<B>,
        ledger_provider: &L,
    ) -> neo_mempool::TransactionAdmissionOutcome
    where
        B: neo_storage::CacheRead,
        L: neo_mempool::AdmissionLedgerProvider;

    /// Update the pool after `block` is persisted.
    ///
    /// Mirrors C# `MemoryPool.UpdatePoolForBlockPersisted`: remove the block's
    /// transactions and evict pooled transactions that conflict with persisted
    /// ones. Test doubles without a real pool can keep the default no-op.
    fn block_persisted(&self, _block: &neo_payloads::Block) {}

    /// Returns whether the pool has unverified transactions that could be
    /// promoted after a post-persist snapshot becomes available.
    fn has_unverified_transactions(&self) -> bool {
        false
    }

    /// Reverify the highest-priority unverified transactions against the live
    /// post-persist snapshot. Returns `true` when unverified transactions remain.
    fn reverify_top_unverified<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _max_count: usize,
    ) -> bool {
        false
    }
}

impl<P> MempoolLike for neo_mempool::MemoryPool<P>
where
    P: NativeContractProvider + 'static,
{
    fn add_transaction<B, L>(
        &self,
        origin: neo_mempool::TransactionOrigin,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache<B>,
        ledger_provider: &L,
    ) -> neo_mempool::TransactionAdmissionOutcome
    where
        B: neo_storage::CacheRead,
        L: neo_mempool::AdmissionLedgerProvider,
    {
        neo_mempool::MemoryPool::add_transaction(
            self,
            origin,
            tx.clone(),
            snapshot,
            ledger_provider,
        )
    }

    fn block_persisted(&self, block: &neo_payloads::Block) {
        let _ = self.update_pool_for_block_persisted(&block.transactions);
    }

    fn has_unverified_transactions(&self) -> bool {
        self.unverified_count() > 0
    }

    fn reverify_top_unverified<B: neo_storage::CacheRead>(
        &self,
        snapshot: &neo_storage::DataCache<B>,
        max_count: usize,
    ) -> bool {
        neo_mempool::MemoryPool::reverify_top_unverified(self, snapshot, max_count)
    }
}
