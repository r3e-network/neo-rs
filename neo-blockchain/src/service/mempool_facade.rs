//! Minimal mempool adapter boundary for the blockchain service.
//!
//! The blockchain service only needs a narrow admission/reverification facade.
//! The full mempool implementation remains in `neo-mempool`; this module keeps
//! the service construction root focused on command-loop state while preserving
//! a small mockable boundary for pipeline and service tests.

use neo_primitives::verify_result::VerifyResult;

/// Minimal mempool facade used by the high-level service API.
///
/// The trait exists so the blockchain service can be unit-tested with a mock
/// mempool; the production implementation forwards to the real `MemoryPool`
/// type. The shape is intentionally tiny: verification context, conflict
/// attribute detection, and reverify queues stay in `neo-mempool` and are
/// exposed to the service through [`crate::service_context::SystemContext`].
pub trait MempoolLike: std::fmt::Debug + Send + Sync {
    /// Try to add a transaction to the mempool. Returns the verify result.
    fn try_add(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult;

    /// Try to add a transaction using a cached state-independent verification
    /// result.
    ///
    /// When `cached_state_independent` is `Some(VerifyResult::Succeed)`, the
    /// mempool skips redundant signature verification and only performs
    /// state-dependent checks. Use this only when the caller already verified
    /// the transaction signatures, for example via `TransactionRouter::preverify`.
    fn try_add_cached(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        settings: &neo_config::ProtocolSettings,
        cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult;

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
    fn reverify_top_unverified(
        &self,
        _snapshot: &neo_storage::DataCache,
        _max_count: usize,
    ) -> bool {
        false
    }
}

impl MempoolLike for neo_mempool::MemoryPool {
    fn try_add(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        neo_mempool::MemoryPool::try_add(self, tx.clone(), snapshot)
    }

    fn try_add_cached(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
        cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        neo_mempool::MemoryPool::try_add_cached(
            self,
            tx.clone(),
            snapshot,
            cached_state_independent,
        )
    }

    fn block_persisted(&self, block: &neo_payloads::Block) {
        let _ = self.update_pool_for_block_persisted(&block.transactions);
    }

    fn has_unverified_transactions(&self) -> bool {
        self.unverified_count() > 0
    }

    fn reverify_top_unverified(&self, snapshot: &neo_storage::DataCache, max_count: usize) -> bool {
        neo_mempool::MemoryPool::reverify_top_unverified(self, snapshot, max_count)
    }
}
